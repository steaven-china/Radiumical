//! Dashboard action dispatch — maps each [`DashAction`] to the corresponding
//! state mutation or backend command (open settings, switch mode, manage
//! sessions, etc.).

use crate::dashboard::DashAction;
use crate::panel::PanelId;
use crate::tui::app::App;
use crate::tui::BackendCmd;

impl App {
    pub(crate) fn dispatch_dash_action(&mut self, action: DashAction) {
        match action {
            DashAction::ShowModels => {
                self.overlays.model_picker = true;
                self.provider_picker.visible = true;
                self.panels.open(PanelId::ProviderPicker);
                let _ = self.cmd_tx.blocking_send(BackendCmd::FetchProviders);
            }
            DashAction::ShowSettings => {
                self.overlays.settings = true;
                self.settings_board.visible = true;
                self.panels.open(PanelId::Settings);
            }
            DashAction::ShowHelp => {
                self.overlays.help = !self.overlays.help;
                self.panels.toggle(PanelId::Help);
                self.viewport.stick_to_bottom = true;
            }
            DashAction::SetMode(mode) => {
                self.mode = mode.clone();
                let _ = self.cmd_tx.blocking_send(BackendCmd::SetMode(mode));
                self.output.push(format!("  {:?} mode", self.mode));
                self.output.push(String::new());
                self.viewport.stick_to_bottom = true;
            }
            DashAction::ToggleReasoning => {
                self.thinking.show_full_reasoning = !self.thinking.show_full_reasoning;
                self.viewport.stick_to_bottom = true;
            }
            DashAction::SessionNew => {
                self.handle_command("/new");
            }
            DashAction::SessionSave => {
                if let Ok(sessions) = self.session_pool.list() {
                    self.session_tui.open(sessions, None, None);
                    // Pre-fill with first history entry as name suggestion
                    if let Some(first) = self.input.history.first() {
                        self.session_tui.name_buffer = first.clone();
                    }
                    // Focus name input for immediate typing
                    self.session_tui.focus = crate::session_tui::SessionFocus::NameEdit;
                }
            }
            DashAction::SessionLoad => {
                if let Ok(sessions) = self.session_pool.list() {
                    self.session_tui.open(sessions, None, None);
                    if let Some(first) = self.session_tui.sessions.first() {
                        self.session_tui.name_buffer = first.name.clone();
                        self.session_tui.desc_buffer = first.description.clone();
                    }
                }
            }
            DashAction::SessionList => {
                if let Ok(sessions) = self.session_pool.list() {
                    self.session_tui.open(sessions, None, None);
                }
            }
            DashAction::SessionDelete => {
                if let Ok(sessions) = self.session_pool.list() {
                    self.session_tui.open(sessions, None, None);
                    self.session_tui.focus = crate::session_tui::SessionFocus::ConfirmDelete;
                }
            }
            DashAction::Diagnostics => {
                self.output
                    .push("  Diagnostics is not yet implemented.".into());
                self.output.push(String::new());
                self.viewport.stick_to_bottom = true;
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
                self.viewport.stick_to_bottom = true;
            }
            DashAction::About => {
                self.output
                    .push("  Radiumical — lean CLI coding agent".into());
                self.output.push("  https://radiumical.dev".into());
                self.output.push(String::new());
                self.viewport.stick_to_bottom = true;
            }
        }
    }
}
