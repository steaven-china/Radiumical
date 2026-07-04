use crate::tui::app::App;
use crate::tui::BackendCmd;

impl App {
    pub(super) fn cmd_help(&mut self) -> bool {
        self.overlays.help = !self.overlays.help;
        if !self.welcome {
            self.output.push("> /help".into());
            self.show_help();
        }
        self.input.text.clear();
        self.input.cursor = 0;
        self.input.hints.clear();
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_settings(&mut self) -> bool {
        if !self.settings_board.visible {
            self.settings_board.visible = true;
            self.overlays.settings = true;
            self.panels.open(crate::panel::PanelId::Settings);
        } else {
            self.commit_settings();
            self.settings_board.visible = false;
            self.overlays.settings = false;
            self.panels.close(crate::panel::PanelId::Settings);
        }
        self.input.text.clear();
        self.input.cursor = 0;
        self.input.hints.clear();
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_provider(&mut self) -> bool {
        self.overlays.model_picker = self.provider_picker.toggle(&self.cmd_tx);
        if self.overlays.model_picker {
            self.panels.open(crate::panel::PanelId::ProviderPicker);
        } else {
            self.panels.close(crate::panel::PanelId::ProviderPicker);
        }
        self.input.text.clear();
        self.input.cursor = 0;
        true
    }

    pub(super) fn cmd_models(&mut self) -> bool {
        self.overlays.model_picker = self.provider_picker.toggle(&self.cmd_tx);
        if self.overlays.model_picker {
            self.panels.open(crate::panel::PanelId::ProviderPicker);
        } else {
            self.panels.close(crate::panel::PanelId::ProviderPicker);
        }
        if self.overlays.model_picker && self.available_models.len() <= 1 {
            let _ = self.cmd_tx.blocking_send(BackendCmd::RefreshModels);
            self.output.push("> /models".into());
            self.output.push("  Refreshing models…".into());
            self.output.push(String::new());
        }
        self.input.text.clear();
        self.input.cursor = 0;
        true
    }

    pub(super) fn cmd_model(&mut self, task: &str) -> bool {
        let m = task[7..].trim().to_string();
        self.model = m.clone();
        let _ = self.cmd_tx.blocking_send(BackendCmd::SetModel(m.clone()));
        self.toasts.push(crate::board::Toast::new(
            format!("Model: {m}"),
            crate::board::ToastLevel::Info,
            std::time::Duration::from_secs(3),
        ));
        self.output.push(format!(" > /model {m}"));
        self.output.push(format!("  Model -> {m}"));
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_tools(&mut self) -> bool {
        self.output.push("> /tools".into());
        let tools = radiumical_core::tools::all_tools();
        self.output
            .push(format!("  Available tools ({}):", tools.len()));
        for t in tools {
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
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_skills(&mut self) -> bool {
        self.output.push("> /skills".into());
        let metas = self.skill_registry.all_meta();
        if metas.is_empty() {
            self.output.push("  No skills installed.".into());
            self.output
                .push("  Place skills in ~/.radi/skills/{name}/SKILL.md".into());
        } else {
            self.output
                .push(format!("  Available skills ({}):", metas.len()));
            for m in metas {
                let marker = if self.skill_registry.get(&m.name).is_some() {
                    "●"
                } else {
                    "○"
                };
                self.output
                    .push(format!("  {} {:<16} {}", marker, m.name, m.description));
            }
            self.output.push(String::new());
            self.output.push("  ● = active  ○ = inactive".into());
            self.output
                .push("  /skill <name> to activate, /skill off <name> to deactivate".into());
        }
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_skill(&mut self, task: &str) -> bool {
        let rest = task[7..].trim();
        if rest == "off" {
            self.skill_registry.deactivate_all();
            self.output.push("  All skills deactivated.".into());
        } else if let Some(name) = rest.strip_prefix("off ") {
            let name = name.trim();
            self.skill_registry.deactivate(name);
            self.output.push(format!("  Deactivated: {name}"));
        } else if rest == "list" {
            self.output.push("> /skill list".into());
            let activated = self.skill_registry.activated();
            if activated.is_empty() {
                self.output.push("  No active skills.".into());
            } else {
                self.output.push("  Active skills:".into());
                for s in &activated {
                    self.output
                        .push(format!("  ● {:<16} {}", s.name, s.description));
                }
            }
        } else {
            let name = rest.to_string();
            if self.skill_registry.activate(&name).is_some() {
                self.output
                    .push(format!("  Activated skill: {name}"));
            } else {
                let matched = radiumical_core::skill::match_by_input(&name);
                if matched.is_empty() {
                    self.output
                        .push(format!("  Skill not found: {name}"));
                } else {
                    let m = &matched[0];
                    if self.skill_registry.activate(&m.name).is_some() {
                        self.output.push(format!(
                            "  Auto-matched and activated: {}",
                            m.name
                        ));
                    }
                }
            }
        }
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_perf(&mut self) -> bool {
        self.overlays.perf = !self.overlays.perf;
        self.output.push("> /perf".into());
        self.output.push(radiumical_core::perf::report());
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_debug_linevis(&mut self) -> bool {
        self.output.push("> /debug linevis".into());
        self.output.push(format!(
            "  total: {} | vis: {} | scroll: {:.0} | stick: {}",
            self.output.len(),
            self.viewport.visible_lines,
            self.viewport.scroll,
            self.viewport.stick_to_bottom
        ));
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_debug(&mut self, task: &str) -> bool {
        let topic = task[6..].trim();
        self.show_debug(topic);
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_diagnostics(&mut self) -> bool {
        self.output.push("> /diagnostics".into());
        let _ = self.cmd_tx.blocking_send(BackendCmd::RunTask(
            "Run LSP diagnostics and lint checks on the workspace. Report findings concisely.".into(),
        ));
        self.output.push("  Running diagnostics…".into());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        self.output.push(String::new());
        true
    }
}
