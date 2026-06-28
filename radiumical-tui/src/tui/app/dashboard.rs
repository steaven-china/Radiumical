use crate::dashboard::DashAction;
use crate::tui::app::App;

impl App {
    pub(crate) fn dispatch_dash_action(&mut self, action: DashAction) {
        match action {
            DashAction::ShowModels => self.show_model_picker = true,
            DashAction::ShowSettings => {
                self.output.push("> /settings".into());
                self.show_settings();
                self.output.push(String::new());
                self.stick_to_bottom = true;
            }
            DashAction::ShowHelp => {
                self.show_help_overlay = true;
                self.stick_to_bottom = true;
            }
            DashAction::ToggleThinking => {
                self.show_full_reasoning = !self.show_full_reasoning;
                self.stick_to_bottom = true;
            }
            DashAction::SessionNew => {
                self.output.clear();
                self.output.push(String::new());
                self.stick_to_bottom = true;
                self.welcome = true;
            }
            DashAction::SessionSave => {
                let summary = self
                    .history
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "session".into());
                self.input = format!("/session save {summary}");
                self.cursor = self.input.len();
                self.update_hints();
            }
            DashAction::SessionLoad => {
                if let Ok(sessions) = radiumical_core::session::Session::list() {
                    let names: Vec<String> = sessions
                        .iter()
                        .map(|s| format!("{} ({} msgs, {})", s.name, s.message_count, s.created))
                        .collect();
                    self.session_list.set_items(names);
                    self.session_list_visible = true;
                }
            }
            DashAction::SessionList => {
                self.input = "/session list".into();
                self.cursor = self.input.len();
                self.update_hints();
            }
            DashAction::SessionDelete => {
                self.input = "/session delete ".into();
                self.cursor = self.input.len();
                self.update_hints();
            }
            DashAction::Diagnostics => {
                self.input = "Run diagnostics on the workspace".into();
                self.cursor = self.input.len();
            }
            DashAction::ShowTools => {
                self.output.push("  Tools:".into());
                for t in radiumical_core::tools::all_tools() {
                    self.output
                        .push(format!("  - {}", t.definition().function.name));
                }
                self.output.push(String::new());
                self.stick_to_bottom = true;
            }
            DashAction::About => {
                self.output
                    .push("  Radiumical — lean CLI coding agent".into());
                self.output.push("  https://radiumical.dev".into());
                self.output.push(String::new());
                self.stick_to_bottom = true;
            }
        }
    }
}
