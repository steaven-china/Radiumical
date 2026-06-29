use crate::dashboard::DashAction;
use crate::tui::app::App;
use crate::tui::BackendCmd;

impl App {
    pub(crate) fn dispatch_dash_action(&mut self, action: DashAction) {
        match action {
            DashAction::ShowModels => {
                self.show_model_picker = true;
                self.provider_picker.visible = true;
                let _ = self.cmd_tx.blocking_send(BackendCmd::FetchProviders);
            }
            DashAction::ShowSettings => {
                self.output.push("> /settings".into());
                self.show_settings();
                if self.settings_board.visible {
                    self.commit_settings();
                    self.settings_board.visible = false;
                    self.settings_visible = false;
                } else {
                    self.settings_board.visible = true;
                    self.settings_visible = true;
                }
                self.output.push(String::new());
                self.stick_to_bottom = true;
            }
            DashAction::ShowHelp => {
                self.show_help_overlay = !self.show_help_overlay;
                self.stick_to_bottom = true;
            }
            DashAction::SetMode(mode) => {
                self.mode = mode.clone();
                let _ = self.cmd_tx.blocking_send(BackendCmd::SetMode(mode));
                self.output.push(format!("  {:?} mode", self.mode));
                self.output.push(String::new());
                self.stick_to_bottom = true;
            }
            DashAction::ToggleReasoning => {
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
                self.perf_visible = !self.perf_visible;
                self.output.push("> /perf".into());
                self.output.push(radiumical_core::perf::report());
                self.output.push(String::new());
                self.stick_to_bottom = true;
            }
            DashAction::ShowTools => {
                self.output.push("> /tools".into());
                self.output.push(format!(
                    "  Available tools ({}):",
                    radiumical_core::tools::all_tools().len()
                ));
                for t in radiumical_core::tools::all_tools() {
                    let def = t.definition();
                    let marker = match self.mode {
                        radiumical_core::types::AgentMode::Plan => {
                            if matches!(
                                def.function.name.as_str(),
                                "read_file" | "search_code" | "find_files"
                            ) {
                                "✅"
                            } else {
                                "🔒"
                            }
                        }
                        _ => "✅",
                    };
                    self.output.push(format!(
                        "  {} {:<14} {}",
                        marker, def.function.name, def.function.description
                    ));
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
