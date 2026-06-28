use crate::tui::app::App;
use crate::tui::UiEvent;
use std::time::Instant;

impl App {
    pub fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::LlmChunk(chunk) => {
                let chunk = chunk.replace("\r\n", "\n").replace('\r', "");
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
            }
            UiEvent::LlmReasoning(rc) => {
                if let Some(last) = self.output.last_mut() {
                    if last.starts_with("\x01") {
                        last.push_str(&rc);
                        return;
                    }
                }
                self.output.push(format!("\x01{rc}"));
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
                let w = 56usize;
                let header = if total > 1 {
                    format!("{} ({}/{})", name, index + 1, total)
                } else {
                    name
                };
                self.output.push(format!("  ┌─ {header}"));
                let sa: String = args.chars().take(w.saturating_sub(2)).collect();
                let dots = if args.chars().count() > w.saturating_sub(2) {
                    "…"
                } else {
                    ""
                };
                self.output.push(format!("  │  {sa}{dots}"));
            }
            UiEvent::ToolDone => {
                self.progress.visible = false;
            }
            UiEvent::ToolResult { content } => {
                let w = 56usize;
                for line in content.lines() {
                    self.output.push(format!("  │ {line}"));
                }
                self.output.push(format!("  └{}┘", "─".repeat(w)));
                self.output.push(String::new());
            }
            UiEvent::Error(e) => {
                self.output.push(format!("  {e}"));
                self.thinking = false;
            }
            UiEvent::ThinkingDone => {
                self.thinking = false;
            }
        }
    }
}
