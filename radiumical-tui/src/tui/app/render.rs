use crate::tui::app::events::{box_args_line, box_bottom, box_result_line, box_top, box_width};
use crate::tui::app::App;
use crate::tui::{LOGO, SLASH_COMMANDS};
use radiumical_core::session::SessionItem;

impl App {
    pub(crate) fn show_help(&mut self) {
        self.output.push("".into());
        self.output.push("  Commands:".into());
        let max_w = SLASH_COMMANDS
            .iter()
            .map(|(n, _)| n.len())
            .max()
            .unwrap_or(10);
        for (n, d) in SLASH_COMMANDS {
            self.output.push(format!("  {n:<w$}  {d}", w = max_w));
        }
        self.output.push("".into());
        self.output.push("  Keys:".into());
        self.output
            .push("  PgUp/PgDn    Scroll".into());
        self.output
            .push("  Up/Down       History / Hint select".into());
        self.output
            .push("  Ctrl+W        Delete word".into());
        self.output
            .push("  Ctrl+A/E      Jump to line start/end".into());
        self.output
            .push("  Ctrl+L        Clear screen".into());
        self.output
            .push("  Ctrl+O        Toggle reasoning".into());
        self.output
            .push("  Ctrl+C        Cancel / Quit".into());
        self.output
            .push("  Shift+Enter   Newline in input".into());
        self.output
            .push("  End           Jump to bottom (empty input)".into());
        self.output
            .push("  Tab           Autocomplete slash command".into());
        self.output
            .push("  Mouse drag    Scroll | Double-click tool call".into());
        self.output.push(String::new());
    }

    #[allow(dead_code)]
    pub(crate) fn show_settings(&mut self) {
        self.output.push("".into());
        self.output
            .push(format!("  Provider : {}", self.provider_name));
        self.output.push(format!("  Model    : {}", self.model));
        self.output.push(format!("  Mode     : {:?}", self.mode));
        self.output
            .push(format!("  History  : {} items", self.input.history.len()));
        self.output.push(String::new());
    }

    pub(crate) fn show_debug(&mut self, topic: &str) {
        self.output.push(format!("> /debug {topic}"));
        self.output.push(String::new());
        match topic {
            "logo" => {
                for line in LOGO {
                    self.output
                        .push(format!("  [{:>2}] {line}", line.chars().count()));
                }
            }
            "output" => {
                self.output.push(format!(
                    "  Lines: {} | Scroll: {:.1} | Stick: {}",
                    self.output.len(),
                    self.viewport.scroll,
                    self.viewport.stick_to_bottom
                ));
            }
            "blocks" => {
                let blocks =
                    crate::layout::measure_blocks(&self.output, 80, self.thinking.show_full_reasoning);
                self.output.push(format!("  Blocks: {}", blocks.len()));
                for (i, b) in blocks.iter().enumerate() {
                    self.output
                        .push(format!("    [{i}] {:?} h={}", b.kind, b.height));
                }
            }
            "" | "help" => {
                self.output.push("  logo | output | blocks".into());
            }
            _ => {
                self.output.push(format!("  Unknown: {topic}"));
            }
        }
        self.output.push(String::new());
    }

    pub(crate) fn render_session_items_to_output(&mut self) {
        self.output.clear();
        for item in &self.session_items {
            match item {
                SessionItem::Meta { .. } => {}
                SessionItem::User { content } => {
                    for line in content.lines() {
                        self.output.push(format!("> {line}"));
                    }
                }
                SessionItem::Assistant { content } => {
                    if content.is_empty() {
                        continue;
                    }
                    for line in content.lines() {
                        self.output.push(line.to_string());
                    }
                    self.output.push(String::new());
                }
                SessionItem::Reasoning { content } => {
                    if content.is_empty() {
                        continue;
                    }
                    self.output.push(format!("\x01[thinking] {content}"));
                }
                SessionItem::Tool {
                    id,
                    name,
                    args,
                    result,
                } => {
                    let header = name.clone();
                    let width = box_width(header.len(), args.chars().count(), result.as_deref());
                    // Embed the tool-call id at the end of the top line so
                    // mouse hit-testing and expansion state remain valid after
                    // a session reload, matching the runtime path in events.rs.
                    self.output
                        .push(format!("{}\x02{}", box_top(&header, width), id));
                    self.output.push(box_args_line(args, width));
                    if let Some(result) = result {
                        for line in result.lines() {
                            self.output.push(box_result_line(line, width));
                        }
                    }
                    self.output.push(box_bottom(width));
                    self.output.push(String::new());
                }
                SessionItem::Raw { lines } => {
                    for line in lines {
                        self.output.push(line.clone());
                    }
                }
            }
        }
        if self.output.is_empty() {
            self.output.push(String::new());
        }
        self.viewport.stick_to_bottom = true;
        self.welcome = false;
    }
}
