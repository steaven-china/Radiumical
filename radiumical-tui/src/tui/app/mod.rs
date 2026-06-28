use crate::tui::{BackendCmd, UiEvent, LOGO};
use radiumical_core::types::{AgentMode, SessionConfig};
use ratatui::text::Line;
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Instant;

pub mod commands;
pub mod dashboard;
pub mod events;
pub mod input;
pub mod mouse;
pub mod render;

// ═══ App ═══

pub struct App {
    pub output: Vec<String>,
    pub input: String,
    pub cursor: usize,
    pub thinking: bool,
    pub thinking_cancelled: bool,
    pub thinking_start: Instant,
    pub thinking_elapsed: u64,
    pub thinking_frame: usize,
    pub hints: Vec<(String, String)>,
    pub hint_selected: Option<usize>,
    pub hint_page: usize,
    pub scroll: f32,
    pub stick_to_bottom: bool,
    pub(crate) scroll_velocity: f32,
    pub mode: AgentMode,
    pub model: String,
    pub provider_name: String,
    pub cmd_tx: tokio::sync::mpsc::Sender<BackendCmd>,
    pub ui_rx: mpsc::Receiver<UiEvent>,
    pub session_items: Vec<radiumical_core::session::SessionItem>,
    pub pending_choice: Option<(String, String, Vec<String>)>,
    pub should_quit: bool,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub(crate) history_draft: String,
    pub welcome: bool,
    pub show_help_overlay: bool,
    pub show_model_picker: bool,
    pub settings_visible: bool,
    pub settings_board: crate::settings::SettingsBoard,
    pub cod_enabled: bool,
    pub thinking_effort: String,
    pub full_reasoning: Vec<String>,
    pub show_full_reasoning: bool,
    pub help_board: crate::board::BoardState,
    pub provider_picker: crate::board::ProviderPicker,
    pub confirm: crate::board::ConfirmBoard,
    pub dashboard: crate::dashboard::Dashboard,
    pub session_list_visible: bool,
    pub session_list: crate::board::ListBoard,
    pub blocks: Vec<crate::layout::Block>,
    pub render_cache: HashMap<u64, Vec<Line<'static>>>,
    pub markdown: crate::markdown::MarkdownRenderer,
    pub perf_visible: bool,
    pub output_vis: usize,
    pub output_width: usize,
    pub rendered_total: usize,
    pub progress: crate::board::ProgressBoard,
    #[allow(dead_code)]
    pub plan_board: crate::board::BoardState,
    pub toasts: Vec<crate::board::Toast>,
    pub available_models: Vec<String>,
    pub scrollbar_dragging: bool,
    pub tool_expanded: std::collections::HashMap<u64, bool>,
    pub tool_result_scroll: std::collections::HashMap<u64, usize>,
    pub last_click: Option<(std::time::Instant, u16, u16)>,
    pub hovered_block: Option<usize>,
}

impl App {
    pub fn new(
        cmd_tx: tokio::sync::mpsc::Sender<BackendCmd>,
        ui_rx: mpsc::Receiver<UiEvent>,
        config: &SessionConfig,
    ) -> Self {
        let mut out = vec![format!("Radiumical — {} @ {}", config.model, ".")];
        out.push(String::new());
        for line in LOGO {
            out.push(format!("  {line}"));
        }
        out.push(String::new());
        out.push("  lean CLI coding agent".into());
        out.push(String::new());
        out.push("Type a task or /help for commands.".into());
        out.push(String::new());
        Self {
            output: out,
            input: String::new(),
            cursor: 0,
            thinking: false,
            thinking_cancelled: false,
            thinking_start: Instant::now(),
            thinking_elapsed: 0,
            thinking_frame: 0,
            hints: Vec::new(),
            hint_selected: None,
            hint_page: 0,
            scroll: 0.0,
            stick_to_bottom: true,
            scroll_velocity: 0.0,
            mode: config.mode.clone(),
            model: config.model.clone(),
            provider_name: config.provider.name().to_string(),
            cmd_tx,
            ui_rx,
            should_quit: false,
            history: Vec::new(),
            history_idx: None,
            history_draft: String::new(),
            welcome: true,
            show_help_overlay: true,
            show_model_picker: false,
            settings_visible: false,
            settings_board: crate::settings::SettingsBoard::from_config(
                &radiumical_core::config::Config::load().unwrap_or_else(|_| radiumical_core::config::Config {
                    model: None,
                    provider: None,
                    api_key: None,
                    api_base: None,
                    heartbeat_secs: None,
                    llm_timeout_secs: None,
                    max_iterations: None,
                    reasoning_effort: None,
                    mode: None,
                }),
                &config.mode,
            ),
            cod_enabled: false,
            thinking_effort: "max".into(),
            full_reasoning: Vec::new(),
            show_full_reasoning: false,
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
            session_list_visible: false,
            session_list: crate::board::ListBoard::new(" Sessions "),
            blocks: Vec::new(),
            render_cache: HashMap::new(),
            markdown: crate::markdown::MarkdownRenderer::new(),
            perf_visible: false,
            progress: crate::board::ProgressBoard::new("Working"),
            output_vis: 20,
            output_width: 80,
            rendered_total: 0,
            plan_board: crate::board::BoardState::new(
                " Plan ",
                30,
                8,
                crate::board::Corner::TopRight,
            ),
            available_models: vec![config.model.clone()],
            scrollbar_dragging: false,
            tool_expanded: std::collections::HashMap::new(),
            tool_result_scroll: std::collections::HashMap::new(),
            last_click: None,
            hovered_block: None,
            session_items: Vec::new(),
            pending_choice: None,
        }
    }

    pub fn tick(&mut self, _visible_lines: usize) {
        self.output_vis = _visible_lines;
        if self.thinking {
            self.thinking_elapsed = self.thinking_start.elapsed().as_secs();
            self.thinking_frame = (self.thinking_start.elapsed().as_millis() / 150) as usize;
        }
        if self.scroll_velocity.abs() > 0.01 && !self.stick_to_bottom {
            self.scroll += self.scroll_velocity;
            self.scroll = self.scroll.max(0.0);
            self.scroll_velocity *= 0.85;
        }
        let max = (self.rendered_total.saturating_sub(_visible_lines)) as f32;
        self.scroll = self.scroll.clamp(0.0, max.max(0.0));
    }

    /// Save settings board, apply changes, and sync mode/thinking effort to backend.
    pub(crate) fn commit_settings(&mut self) {
        self.settings_board.save();
        let board = self.settings_board.clone();
        let old_mode = self.mode.clone();
        let old_effort = self.thinking_effort.clone();
        board.apply_to_app(self);
        if self.mode != old_mode {
            let _ = self
                .cmd_tx
                .blocking_send(crate::tui::BackendCmd::SetMode(self.mode.clone()));
        }
        if self.thinking_effort != old_effort {
            let _ = self
                .cmd_tx
                .blocking_send(crate::tui::BackendCmd::SetThinkingEffort(
                    self.thinking_effort.clone(),
                ));
        }
    }

    pub fn scroll_up(&mut self, lines: f32) {
        let lines = lines.max(0.0);
        let max = self.rendered_total.saturating_sub(self.output_vis.max(1)) as f32;
        if self.stick_to_bottom {
            self.scroll = max.max(0.0);
            self.stick_to_bottom = false;
        }
        self.scroll = (self.scroll + lines).min(max.max(0.0)).max(0.0);
        if lines > 0.0 {
            self.scroll_velocity = lines;
        }
    }

    pub fn scroll_down(&mut self, lines: f32) {
        let lines = lines.max(0.0);
        let max = self.rendered_total.saturating_sub(self.output_vis.max(1)) as f32;
        self.scroll = (self.scroll - lines).max(0.0);
        self.scroll = self.scroll.min(max.max(0.0));
        if lines > 0.0 {
            self.scroll_velocity = -lines;
        }
    }

    /// Compute the top visible content-line index from current scroll state.
    /// Shared by draw.rs (rendering) and mouse.rs (hit-testing) so they never diverge.
    pub fn scroll_start(&self, total: usize, vis: usize) -> usize {
        let vis = vis.max(1);
        if self.stick_to_bottom {
            total.saturating_sub(vis)
        } else {
            (self.scroll as usize).min(total.saturating_sub(vis))
        }
    }
}
