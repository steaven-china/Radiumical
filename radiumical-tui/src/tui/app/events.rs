use crate::tui::app::App;
use crate::tui::BackendCmd;
use crate::tui::UiEvent;
use radiumical_core::session::SessionItem;
use std::time::Instant;

/// Strip MCP/assistant metadata tags that should never be visible in the TUI.
/// Currently removes `<environment_details>...</environment_details>` blocks.
pub(crate) fn strip_metadata_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let open = "<environment_details>";
    let close = "</environment_details>";
    while i < s.len() {
        if let Some(start) = s[i..].find(open) {
            out.push_str(&s[i..i + start]);
            let after_open = i + start + open.len();
            if let Some(end) = s[after_open..].find(close) {
                i = after_open + end + close.len();
            } else {
                // Incomplete tag at the end: drop the remainder.
                break;
            }
        } else {
            out.push_str(&s[i..]);
            break;
        }
    }
    // Also drop any stray closing tag fragment.
    out.split(close)
        .collect::<Vec<_>>()
        .join("")
        .replace(open, "")
}

pub(crate) fn box_width(name_len: usize, args_len: usize, result: Option<&str>) -> usize {
    let result_max = result
        .map(|r| r.lines().map(|l| l.chars().count()).max().unwrap_or(0))
        .unwrap_or(0);
    let content_max = args_len.max(result_max);
    (content_max + 4).max(name_len + 7).max(56)
}

pub(crate) fn box_top(name: &str, width: usize) -> String {
    let fill = width.saturating_sub(name.len() + 5);
    format!("  ┌─ {name} {}┐", "─".repeat(fill))
}

pub(crate) fn box_args_line(args: &str, width: usize) -> String {
    let inner = width.saturating_sub(4);
    let visible = args
        .chars()
        .take(inner.saturating_sub(2))
        .collect::<String>();
    let dots = if args.chars().count() > inner.saturating_sub(2) {
        "…"
    } else {
        ""
    };
    let text = format!("{visible}{dots}");
    let pad = inner.saturating_sub(text.chars().count() + 2);
    format!("  │  {text}{}│", " ".repeat(pad))
}

pub(crate) fn box_result_line(line: &str, width: usize) -> String {
    let inner = width.saturating_sub(4);
    let visible = line
        .chars()
        .take(inner.saturating_sub(1))
        .collect::<String>();
    let pad = inner.saturating_sub(visible.chars().count() + 1);
    format!("  │ {visible}{}│", " ".repeat(pad))
}

pub(crate) fn box_bottom(width: usize) -> String {
    let inner = width.saturating_sub(4);
    format!("  └{}┘", "─".repeat(inner))
}

impl App {
    pub fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::LlmChunk(chunk) => {
                let chunk = strip_metadata_tags(&chunk)
                    .replace("\r\n", "\n")
                    .replace('\r', "");
                if chunk.is_empty() {
                    return;
                }
                if let Some(last) = self.output.last() {
                    if last.starts_with("\x01") {
                        self.output.push(String::new());
                    }
                }
                for ch in chunk.chars() {
                    if ch == '\n' {
                        self.output.push(String::new());
                    } else {
                        if self.output.is_empty() {
                            self.output.push(String::new());
                        }
                        self.output.last_mut().unwrap().push(ch);
                    }
                }
                // Aggregate into session assistant item
                if let Some(SessionItem::Assistant { content }) = self.session_items.last_mut() {
                    content.push_str(&chunk);
                } else {
                    self.session_items
                        .push(SessionItem::Assistant { content: chunk });
                }
            }
            UiEvent::LlmReasoning(rc) => {
                let rc = strip_metadata_tags(&rc);
                if rc.is_empty() {
                    return;
                }
                if let Some(last) = self.output.last_mut() {
                    if last.starts_with("\x01") {
                        last.push_str(&rc);
                    } else {
                        self.output.push(format!("\x01[思考] {rc}"));
                    }
                } else {
                    self.output.push(format!("\x01[思考] {rc}"));
                }
                // Aggregate into session reasoning item
                if let Some(SessionItem::Reasoning { content }) = self.session_items.last_mut() {
                    content.push_str(&rc);
                } else {
                    self.session_items
                        .push(SessionItem::Reasoning { content: rc });
                }
            }
            UiEvent::ThinkingTick => {
                if self.thinking_cancelled {
                    return;
                }
                if !self.thinking {
                    self.thinking_start = Instant::now();
                }
                self.thinking = true;
            }
            UiEvent::LlmDone => {
                if self.output.last().is_none_or(|l| !l.is_empty()) {
                    self.output.push(String::new());
                }
            }
            UiEvent::ToolStart {
                name,
                index,
                total,
                args,
            } => {
                self.progress.visible = true;
                self.progress.label = name.clone();
                self.progress.progress = index as f32 / total.max(1) as f32;
                let header = if total > 1 {
                    format!("{} ({}/{})", name, index + 1, total)
                } else {
                    name.clone()
                };
                let id = format!(
                    "tc_{}_{}",
                    self.next_tool_id,
                    self.session_items.len()
                );
                self.next_tool_id += 1;
                let width = box_width(header.len(), args.chars().count(), None);
                // Embed id at the end of the box top line so measure_blocks still
                // sees a normal ┌─ header while mouse hit-testing can recover it.
                self.output.push(format!("{}\x02{}", box_top(&header, width), id));
                self.output.push(box_args_line(&args, width));

                self.session_items.push(SessionItem::Tool {
                    id,
                    name,
                    args,
                    result: None,
                });
            }
            UiEvent::ToolDone => {
                self.progress.visible = false;
            }
            UiEvent::ToolResult { content } => {
                // Pair result with the most recent tool call
                let mut tool_name = String::new();
                let mut tool_args = String::new();
                if let Some(SessionItem::Tool {
                    name, args, result, ..
                }) = self
                    .session_items
                    .iter_mut()
                    .rev()
                    .find(|i| matches!(i, SessionItem::Tool { result: None, .. }))
                {
                    *result = Some(content.clone());
                    tool_name = name.clone();
                    tool_args = args.clone();
                }

                if !tool_name.is_empty() {
                    let width =
                        box_width(tool_name.len(), tool_args.chars().count(), Some(&content));
                    for line in content.lines() {
                        self.output.push(box_result_line(line, width));
                    }
                    self.output.push(box_bottom(width));
                    self.output.push(String::new());
                } else {
                    // Fallback: orphan result
                    for line in content.lines() {
                        self.output.push(format!("  {line}"));
                    }
                    self.output.push(String::new());
                }
            }
            UiEvent::Error(e) => {
                self.output.push(format!("  {e}"));
                self.thinking = false;
                self.session_items.push(SessionItem::Raw { lines: vec![e] });
            }
            UiEvent::ThinkingDone => {
                self.thinking = false;
            }
            UiEvent::Choice { id, mode, options } => {
                self.pending_choice = Some((id, mode.clone(), options.clone()));
                self.output.push(String::new());
                self.output.push(format!("  ◆ Choice ({mode})"));
                if mode == "input" {
                    if let Some(prompt) = options.first() {
                        self.output.push(format!("  {prompt}"));
                    }
                    self.output
                        .push("  Type your answer and press Enter".into());
                } else {
                    for (i, opt) in options.iter().enumerate() {
                        self.output.push(format!("    {}. {opt}", i + 1));
                    }
                    let sep = if mode == "multi" {
                        "comma-separated"
                    } else {
                        "a"
                    };
                    self.output.push(format!("  Enter {sep} number:"));
                }
                self.output.push(String::new());
                self.stick_to_bottom = true;
            }
            UiEvent::ProvidersLoaded(sources) => {
                let first = sources.first().cloned();
                self.provider_picker.set_providers(sources);
                if let Some(source) = first {
                    self.provider_name = source.name.clone();
                    let _ = self.cmd_tx.blocking_send(BackendCmd::FetchModels(source));
                }
            }
            UiEvent::ModelsLoaded(mut models) => {
                if !models.contains(&self.model) {
                    models.push(self.model.clone());
                }
                self.available_models = models.clone();
                self.provider_picker.set_models(models);
            }
        }
    }
}
