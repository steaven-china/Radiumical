//! Backend-to-UI event processing.
//!
//! Maps each [`UiEvent`] variant emitted by the async backend into the
//! corresponding mutation on [`App`] output lines, session items, thinking
//! state, and overlay data.

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
    format!("  │  {text}{}", " ".repeat(pad))
}

pub(crate) fn box_result_line(line: &str, width: usize) -> String {
    let inner = width.saturating_sub(4);
    let visible = line
        .chars()
        .take(inner.saturating_sub(1))
        .collect::<String>();
    let pad = inner.saturating_sub(visible.chars().count() + 1);
    format!("  │ {visible}{}", " ".repeat(pad))
}

pub(crate) fn box_bottom(width: usize) -> String {
    let inner = width.saturating_sub(4);
    format!("  └{}┘", "─".repeat(inner))
}

impl App {
    /// Process a single [`UiEvent`] from the backend, updating output lines,
    /// session history, thinking state, tool-call boxes, and overlay data.
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
                        self.output.push(format!("\x01[thinking] {rc}"));
                    }
                } else {
                    self.output.push(format!("\x01[thinking] {rc}"));
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
                if self.thinking.cancelled {
                    return;
                }
                if !self.thinking.active {
                    self.thinking.start = Instant::now();
                }
                self.thinking.active = true;
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
                let id = format!("tc_{}_{}", self.next_tool_id, self.session_items.len());
                self.next_tool_id += 1;
                let width = box_width(header.len(), args.chars().count(), None);
                // Embed id at the end of the box top line so measure_blocks still
                // sees a normal ┌─ header while mouse hit-testing can recover it.
                self.output
                    .push(format!("{}\x02{}", box_top(&header, width), id));
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
                    // Strip \r and ANSI escapes before storing — these cause
                    // padding miscalculation in box_result_line and layout drift.
                    let clean = crate::layout::strip_ansi_escapes(
                        &content.replace("\r\n", "\n").replace('\r', ""),
                    );
                    let width = box_width(tool_name.len(), tool_args.chars().count(), Some(&clean));
                    for line in clean.lines() {
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
                let hint = if e.contains("timeout") || e.contains("timed out") {
                    " (try /retry or increase timeout in /settings)"
                } else if e.contains("rate limit") || e.contains("429") {
                    " (rate limited — wait a moment, then /retry)"
                } else if e.contains("auth") || e.contains("401") || e.contains("403") {
                    " (check API key: /env list or /settings)"
                } else if e.contains("context") || e.contains("token") {
                    " (context too long — try /new to start fresh)"
                } else {
                    " (try /retry)"
                };
                // Split multi-line errors so each line gets its own block with
                // correct height (measure_blocks gives Text blocks height=1).
                let first_line = format!("\x03  \u{2717} Error: {}", e.lines().next().unwrap_or(&e));
                let rest: Vec<&str> = e.lines().skip(1).collect();
                self.output.push(first_line);
                for line in rest {
                    self.output.push(format!("\x03    {line}"));
                }
                if !hint.is_empty() {
                    self.output.push(format!("\x03    {hint}"));
                }
                self.thinking.active = false;
                self.session_items.push(SessionItem::Raw { lines: vec![e] });
            }
            UiEvent::ThinkingDone => {
                self.thinking.active = false;
            }
            UiEvent::Choice { id, mode, options } => {
                self.choice_panel.open(id, &mode, options);
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
            UiEvent::TitleGenerated(title) => {
                self.session_title = Some(title);
            }
            UiEvent::Toast {
                message,
                level,
                duration_secs,
            } => {
                let lvl = match level.as_str() {
                    "error" => crate::board::ToastLevel::Error,
                    "warn" => crate::board::ToastLevel::Warn,
                    _ => crate::board::ToastLevel::Info,
                };
                self.toasts.push(crate::board::Toast::new(
                    message,
                    lvl,
                    std::time::Duration::from_secs(duration_secs),
                ));
            }
            UiEvent::SubAgentDone { id, success } => {
                let (msg, lvl) = if success {
                    (
                        format!("Sub-agent '{id}' completed"),
                        crate::board::ToastLevel::Info,
                    )
                } else {
                    (
                        format!("Sub-agent '{id}' failed"),
                        crate::board::ToastLevel::Error,
                    )
                };
                self.toasts.push(crate::board::Toast::new(
                    msg,
                    lvl,
                    std::time::Duration::from_secs(5),
                ));
            }
            UiEvent::McpStatus {
                name,
                alive,
                tool_count,
            } => {
                if let Some(s) = self.mcp_servers.iter_mut().find(|s| s.name == name) {
                    s.alive = alive;
                    s.tool_count = tool_count;
                } else {
                    self.mcp_servers
                        .push(crate::panels::mcp_status::McpServerStatus {
                            name,
                            alive,
                            tool_count,
                            enabled: true,
                        });
                }
            }
            UiEvent::PlanUpdated { title, tasks } => {
                self.overlays.plan_title = title;
                self.overlays.plan_tasks = tasks
                    .into_iter()
                    .map(|t| crate::panels::plan::PlanTask {
                        id: t.id,
                        title: t.title,
                        status: t.status,
                    })
                    .collect();
            }
            UiEvent::CheckpointCreated(cp) => {
                self.overlays.timeline_items.insert(0, cp);
                self.overlays.timeline_selected = 0;
            }
        }
    }
}
