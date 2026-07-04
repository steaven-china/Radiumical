//! Value-type structs that hold discrete slices of [`App`](crate::tui::app::App) state,
//! grouped by concern to keep `mod.rs` manageable.

use std::time::Instant;

/// Visibility flags for every overlay / floating panel in the TUI.
#[derive(Debug)]
pub struct OverlayState {
    pub help: bool,
    pub model_picker: bool,
    pub settings: bool,
    pub perf: bool,
    pub subagents: bool,
    pub mcp: bool,
    pub mcp_selected: usize,
    pub outline: bool,
    #[allow(dead_code)]
    pub outline_data: Option<String>,
    #[allow(dead_code)]
    pub outline_scroll: usize,
    pub diagnostics: bool,
    #[allow(dead_code)]
    pub diagnostics_items: Vec<crate::panels::diagnostics::DiagnosticItem>,
    #[allow(dead_code)]
    pub diagnostics_scroll: usize,
    pub memory: bool,
    #[allow(dead_code)]
    pub memory_state: crate::panels::memory::MemoryPanelState,
    pub plan: bool,
    pub plan_title: String,
    pub plan_tasks: Vec<crate::panels::plan::PlanTask>,
    pub agents: bool,
    pub agents_list: Vec<radiumical_core::agent_pool::AgentDef>,
}

/// Single-line text input buffer with cursor position, autocomplete hints,
/// and scrollable command history.
#[derive(Debug)]
pub struct InputState {
    pub text: String,
    pub cursor: usize,
    pub hints: Vec<(String, String)>,
    pub hint_selected: Option<usize>,
    pub hint_page: usize,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub(crate) history_draft: String,
    pub(crate) history_filter_prefix: Option<String>,
}

/// Tracks the "thinking" animation state, reasoning-effort level, and
/// optional chain-of-draft reasoning buffer.
#[derive(Debug)]
pub struct ThinkingState {
    pub active: bool,
    pub cancelled: bool,
    pub start: Instant,
    pub elapsed: u64,
    pub frame: usize,
    pub effort: String,
    pub cod_enabled: bool,
    pub full_reasoning: Vec<String>,
    pub show_full_reasoning: bool,
}

/// Scroll position, velocity, and layout metrics for the main output viewport.
#[derive(Debug)]
pub struct ViewportState {
    pub scroll: f32,
    pub stick_to_bottom: bool,
    pub(crate) scroll_velocity: f32,
    pub visible_lines: usize,
    pub width: usize,
    pub rendered_total: usize,
    pub scrollbar_dragging: bool,
}

impl OverlayState {
    pub fn new() -> Self {
        Self {
            help: true,
            model_picker: false,
            settings: false,
            perf: false,
            subagents: false,
            mcp: false,
            mcp_selected: 0,
            outline: false,
            outline_data: None,
            outline_scroll: 0,
            diagnostics: false,
            diagnostics_items: Vec::new(),
            diagnostics_scroll: 0,
            memory: false,
            memory_state: crate::panels::memory::MemoryPanelState::default(),
            plan: false,
            plan_title: String::new(),
            plan_tasks: Vec::new(),
            agents: false,
            agents_list: radiumical_core::agent_pool::load_agents(),
        }
    }
}

impl InputState {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            hints: Vec::new(),
            hint_selected: None,
            hint_page: 0,
            history: Vec::new(),
            history_idx: None,
            history_draft: String::new(),
            history_filter_prefix: None,
        }
    }
}

impl ThinkingState {
    pub fn new() -> Self {
        Self {
            active: false,
            cancelled: false,
            start: Instant::now(),
            elapsed: 0,
            frame: 0,
            effort: "max".into(),
            cod_enabled: false,
            full_reasoning: Vec::new(),
            show_full_reasoning: false,
        }
    }
}

impl ViewportState {
    pub fn new() -> Self {
        Self {
            scroll: 0.0,
            stick_to_bottom: true,
            scroll_velocity: 0.0,
            visible_lines: 20,
            width: 80,
            rendered_total: 0,
            scrollbar_dragging: false,
        }
    }
}
