use crate::tui::{BackendCmd, UiEvent, LOGO};
use radiumical_core::types::{AgentMode, SessionConfig};
use ratatui::text::Line;
use std::collections::{HashMap, VecDeque};

pub mod commands;
pub mod dashboard;
pub mod events;
pub mod input;
pub mod mouse;
pub mod render;
pub mod state;

pub use state::{InputState, OverlayState, ThinkingState, ViewportState};

// ═══ App ═══

pub struct App {
    pub output: Vec<String>,
    pub overlays: OverlayState,
    pub input: InputState,
    pub thinking: ThinkingState,
    pub viewport: ViewportState,
    pub mode: AgentMode,
    pub model: String,
    pub provider_name: String,
    pub cmd_tx: tokio::sync::mpsc::Sender<BackendCmd>,
    pub ui_rx: tokio::sync::mpsc::Receiver<UiEvent>,
    pub session_items: Vec<radiumical_core::session::SessionItem>,
    pub session_pool: radiumical_core::session::SessionPool,
    pub memory: radiumical_core::memory::Memory,
    pub choice_panel: crate::choice_panel::ChoicePanel,
    pub should_quit: bool,
    pub welcome: bool,
    pub settings_board: crate::settings::SettingsBoard,
    pub help_board: crate::board::BoardState,
    pub provider_picker: crate::board::ProviderPicker,
    pub confirm: crate::board::ConfirmBoard,
    pub dashboard: crate::dashboard::Dashboard,
    pub session_tui: crate::session_tui::SessionTui,
    pub skill_registry: radiumical_core::skill::SkillRegistry,
    pub panels: crate::panel::PanelManager,
    pub session_title: Option<String>,
    pub blocks: Vec<crate::layout::Block>,
    pub render_cache: HashMap<u64, Vec<Line<'static>>>,
    pub render_cache_order: VecDeque<u64>,
    pub markdown: crate::markdown::MarkdownRenderer,
    pub progress: crate::board::ProgressBoard,
    #[allow(dead_code)]
    pub plan_board: crate::board::BoardState,
    pub toasts: Vec<crate::board::Toast>,
    pub available_models: Vec<String>,
    pub tool_expanded: HashMap<String, bool>,
    pub tool_result_scroll: HashMap<String, usize>,
    pub next_tool_id: usize,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    pub hovered_block: Option<usize>,
    pub mcp_servers: Vec<crate::panels::mcp_status::McpServerStatus>,
    pub agent_role: String,
    pub tip_state: crate::tips::TipState,
}

impl App {
    pub fn new(
        cmd_tx: tokio::sync::mpsc::Sender<BackendCmd>,
        ui_rx: tokio::sync::mpsc::Receiver<UiEvent>,
        config: &SessionConfig,
        workspace: &str,
    ) -> Self {
        let mut out = vec![format!("Radiumical — {} @ {}", config.model, workspace)];
        out.push(String::new());
        for line in LOGO {
            out.push(format!("  {line}"));
        }
        out.push(String::new());
        out.push("  lean CLI coding agent".into());
        out.push(String::new());
        out.push("  Type a task to get started, or use:".into());
        out.push("    //        — open dashboard".into());
        out.push("    /help     — show all commands".into());
        out.push("    /provider — switch model".into());
        out.push("    /sessions — manage sessions".into());
        out.push(String::new());
        out.push("  Ctrl+C cancel  |  Esc close overlay  |  ↑↓ history".into());
        out.push(String::new());
        Self {
            output: out,
            overlays: OverlayState::new(),
            input: InputState::new(),
            thinking: ThinkingState::new(),
            viewport: ViewportState::new(),
            mode: config.mode.clone(),
            model: config.model.clone(),
            provider_name: config.provider.name().to_string(),
            cmd_tx,
            ui_rx,
            should_quit: false,
            welcome: true,
            settings_board: crate::settings::SettingsBoard::from_config(
                &radiumical_core::config::Config::load().unwrap_or(radiumical_core::config::Config {
                    model: None,
                    provider: None,
                    api_key: None,
                    api_base: None,
                    heartbeat_secs: None,
                    llm_timeout_secs: None,
                    max_iterations: None,
                    reasoning_effort: None,
                    mode: None,
                    max_context_tokens: None,
                    context_compress_ratio: None,
                }),
                &config.mode,
            ),
            help_board: crate::board::BoardState::new(
                " Help ",
                36,
                18,
                crate::board::Corner::BottomRight,
            ),
            provider_picker: crate::board::ProviderPicker::new(" Providers "),
            toasts: Vec::new(),
            confirm: crate::board::ConfirmBoard::new("Are you sure?"),
            dashboard: crate::dashboard::Dashboard::new(),
            session_tui: crate::session_tui::SessionTui::new(),
            skill_registry: radiumical_core::skill::SkillRegistry::new(),
            panels: crate::panel::PanelManager::new(),
            session_title: None,
            blocks: Vec::new(),
            render_cache: HashMap::new(),
            render_cache_order: VecDeque::new(),
            markdown: crate::markdown::MarkdownRenderer::new(),
            progress: crate::board::ProgressBoard::new("Working"),
            plan_board: crate::board::BoardState::new(
                " Plan ",
                30,
                8,
                crate::board::Corner::TopRight,
            ),
            available_models: vec![config.model.clone()],
            tool_expanded: HashMap::new(),
            tool_result_scroll: HashMap::new(),
            next_tool_id: 1,
            last_click: None,
            hovered_block: None,
            mcp_servers: Vec::new(),
            agent_role: "coder".into(),

            session_items: Vec::new(),
            session_pool: radiumical_core::session::SessionPool::for_workspace(workspace),
            memory: radiumical_core::memory::Memory::for_workspace(workspace),
            choice_panel: crate::choice_panel::ChoicePanel::new(),
            tip_state: crate::tips::TipState::new(),
        }
    }

    pub fn tick(&mut self, _visible_lines: usize) {
        self.viewport.visible_lines = _visible_lines;
        if self.thinking.active {
            self.thinking.elapsed = self.thinking.start.elapsed().as_secs();
            self.thinking.frame = (self.thinking.start.elapsed().as_millis() / 150) as usize;
        }
        if self.viewport.scroll_velocity.abs() > 0.01 && !self.viewport.stick_to_bottom {
            self.viewport.scroll += self.viewport.scroll_velocity;
            self.viewport.scroll = self.viewport.scroll.max(0.0);
            self.viewport.scroll_velocity *= 0.85;
        }
        let max = (self.viewport.rendered_total.saturating_sub(_visible_lines)) as f32;
        self.viewport.scroll = self.viewport.scroll.clamp(0.0, max.max(0.0));
        if self.tip_state.should_rotate() {
            self.tip_state.rotate();
        }
    }

    /// Save settings board, apply changes, and sync mode/thinking effort to backend.
    pub(crate) fn commit_settings(&mut self) {
        self.settings_board.save();
        let board = self.settings_board.clone();
        let old_mode = self.mode.clone();
        let old_effort = self.thinking.effort.clone();
        board.apply_to_app(self);
        if self.mode != old_mode {
            let _ = self
                .cmd_tx
                .blocking_send(crate::tui::BackendCmd::SetMode(self.mode.clone()));
        }
        if self.thinking.effort != old_effort {
            let _ = self
                .cmd_tx
                .blocking_send(crate::tui::BackendCmd::SetThinkingEffort(
                    self.thinking.effort.clone(),
                ));
        }
    }

    pub fn scroll_up(&mut self, lines: f32) {
        let lines = lines.max(0.0);
        let max = self.viewport.rendered_total.saturating_sub(self.viewport.visible_lines.max(1)) as f32;
        if self.viewport.stick_to_bottom {
            self.viewport.stick_to_bottom = false;
            // scroll is already at max, just unset stick — next scroll will move
        }
        self.viewport.scroll = (self.viewport.scroll + lines).min(max.max(0.0)).max(0.0);
        if lines > 0.0 {
            self.viewport.scroll_velocity = lines;
        }
    }

    pub fn scroll_down(&mut self, lines: f32) {
        let lines = lines.max(0.0);
        let max = self.viewport.rendered_total.saturating_sub(self.viewport.visible_lines.max(1)) as f32;
        if self.viewport.stick_to_bottom {
            self.viewport.stick_to_bottom = false;
        }
        self.viewport.scroll = (self.viewport.scroll - lines).max(0.0);
        self.viewport.scroll = self.viewport.scroll.min(max.max(0.0));
        if lines > 0.0 {
            self.viewport.scroll_velocity = -lines;
        }
    }

    /// Compute the top visible content-line index from current scroll state.
    /// Shared by draw.rs (rendering) and mouse.rs (hit-testing) so they never diverge.
    pub fn scroll_start(&self, total: usize, vis: usize) -> usize {
        let vis = vis.max(1);
        if self.viewport.stick_to_bottom {
            total.saturating_sub(vis)
        } else {
            (self.viewport.scroll as usize).min(total.saturating_sub(vis))
        }
    }
}
