//! Agent-mode and reasoning-effort slash commands (`/plan`, `/exec`, `/auto`,
//! `/review`, `/think`, `/agents`).

use crate::tui::app::App;
use crate::tui::BackendCmd;

impl App {
    pub(super) fn cmd_plan(&mut self) -> bool {
        self.mode = radiumical_core::types::AgentMode::Plan;
        let _ = self
            .cmd_tx
            .blocking_send(BackendCmd::SetMode(radiumical_core::types::AgentMode::Plan));
        self.output.push("  Plan mode".into());
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_plan_vis(&mut self) -> bool {
        self.overlays.plan = !self.overlays.plan;
        if self.overlays.plan {
            self.panels.open(crate::panel::PanelId::Plan);
            self.output.push("> /plan vis".into());
            self.output.push("  Plan panel opened".into());
        } else {
            self.panels.close(crate::panel::PanelId::Plan);
            self.output.push("> /plan vis".into());
            self.output.push("  Plan panel closed".into());
        }
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_plan_show(&mut self) -> bool {
        self.output.push("> /plan show".into());
        if self.overlays.plan_tasks.is_empty() {
            self.output.push("  No plan active.".into());
        } else {
            self.output.push(format!("# {}", self.overlays.plan_title));
            let total = self.overlays.plan_tasks.len();
            let done = self
                .overlays
                .plan_tasks
                .iter()
                .filter(|t| t.status == radiumical_core::orchestrator::TaskStatus::Done)
                .count();
            self.output
                .push(format!("  progress: {}/{} done", done, total));
            self.output.push(String::new());
            for task in &self.overlays.plan_tasks {
                let icon = task.status.icon();
                self.output
                    .push(format!("  {} #{} {}", icon, task.id, task.title));
            }
        }
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_agents(&mut self) -> bool {
        self.overlays.agents = !self.overlays.agents;
        if self.overlays.agents {
            self.overlays.agents_list = radiumical_core::agent_pool::load_agents();
            self.panels.open(crate::panel::PanelId::Agents);
            self.output.push("> /agents".into());
            self.output.push("  Agent roles panel opened".into());
        } else {
            self.panels.close(crate::panel::PanelId::Agents);
            self.output.push("> /agents".into());
            self.output.push("  Agent roles panel closed".into());
        }
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_agents_name(&mut self, task: &str) -> bool {
        let name = task[8..].trim();
        match radiumical_core::agent_pool::get_agent(name) {
            Some(agent) => {
                self.agent_role = agent.name.clone();
                let agent_mode = agent.mode.to_agent_mode();
                self.mode = agent_mode.clone();
                let _ = self.cmd_tx.blocking_send(BackendCmd::SetMode(agent_mode));
                self.toasts.push(crate::board::Toast::new(
                    format!("Switched to: {}", agent.name),
                    crate::board::ToastLevel::Info,
                    std::time::Duration::from_secs(3),
                ));
                self.output.push(format!("> /agents {}", name));
                self.output
                    .push(format!("  Role: {} ({})", agent.name, agent.description));
            }
            None => {
                self.output.push(format!("  Agent not found: {name}"));
                let available = radiumical_core::agent_pool::load_agents();
                let names: Vec<&str> = available.iter().map(|a| a.name.as_str()).collect();
                self.output
                    .push(format!("  Available: {}", names.join(", ")));
            }
        }
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_exec(&mut self) -> bool {
        self.mode = radiumical_core::types::AgentMode::Exec;
        let _ = self
            .cmd_tx
            .blocking_send(BackendCmd::SetMode(radiumical_core::types::AgentMode::Exec));
        self.output.push("  Exec mode".into());
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_auto(&mut self) -> bool {
        self.mode = radiumical_core::types::AgentMode::Auto;
        let _ = self
            .cmd_tx
            .blocking_send(BackendCmd::SetMode(radiumical_core::types::AgentMode::Auto));
        self.output.push("  Auto mode".into());
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_review(&mut self) -> bool {
        let has_history = !self.session_items.is_empty();
        if has_history {
            let prompt = "Review the changes made in this session and suggest improvements. Check for bugs, style issues, missing tests, and dead code. Report findings concisely.";
            let _ = self
                .cmd_tx
                .blocking_send(BackendCmd::RunTask(prompt.into()));
            self.output.push("> /review".into());
            self.output.push("  Reviewing session…".into());
        } else {
            self.toasts.push(crate::board::Toast::new(
                "Nothing to review yet",
                crate::board::ToastLevel::Warn,
                std::time::Duration::from_secs(3),
            ));
            self.output.push("> /review".into());
            self.output.push("  No session history to review.".into());
        }
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_think_high(&mut self) -> bool {
        self.thinking.effort = "high".into();
        let _ = self
            .cmd_tx
            .blocking_send(BackendCmd::SetThinkingEffort("high".into()));
        self.output.push("> /think high".into());
        self.output.push("  Reasoning: high".into());
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_think_max(&mut self) -> bool {
        self.thinking.effort = "max".into();
        let _ = self
            .cmd_tx
            .blocking_send(BackendCmd::SetThinkingEffort("max".into()));
        self.output.push("> /think max".into());
        self.output.push("  Reasoning: max".into());
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }

    pub(super) fn cmd_think(&mut self, task: &str) -> bool {
        if task == "/think" {
            self.output.push("> /think".into());
            self.output
                .push(format!("  Current effort: {}", self.thinking.effort));
            self.output
                .push("  Options: /think low | /think high | /think max".into());
        } else {
            self.thinking.effort = "low".into();
            let _ = self
                .cmd_tx
                .blocking_send(BackendCmd::SetThinkingEffort("low".into()));
            self.output.push("> /think low".into());
            self.output.push("  Reasoning: low".into());
        }
        self.output.push(String::new());
        self.input.text.clear();
        self.input.cursor = 0;
        self.viewport.stick_to_bottom = true;
        true
    }
}
