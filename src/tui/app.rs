use crate::tui::{BackendCmd, UiEvent, LOGO, SLASH_COMMANDS, matching_hints};
use crate::types::{AgentMode, SessionConfig};
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::layout::Rect;
use std::sync::mpsc;
use std::time::Instant;

// ═══ App ═══

pub struct App {
    pub output: Vec<String>,
    pub input: String,
    pub cursor: usize,
    pub thinking: bool,
    pub thinking_start: Instant,
    pub thinking_elapsed: u64,
    pub thinking_frame: usize,
    pub hints: Vec<(String, String)>,
    pub hint_selected: Option<usize>,
    pub hint_page: usize,
    pub scroll: f32,
    pub stick_to_bottom: bool,
    scroll_velocity: f32,
    pub mode: AgentMode,
    pub model: String,
    pub provider_name: String,
    pub cmd_tx: mpsc::Sender<BackendCmd>,
    pub ui_rx: mpsc::Receiver<UiEvent>,
    pub should_quit: bool,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    history_draft: String,
    pub welcome: bool,
    pub show_help_overlay: bool,
    pub show_model_picker: bool,
    pub cod_enabled: bool,
    pub thinking_effort: String,
    pub full_reasoning: Vec<String>,
    pub show_full_reasoning: bool,
    pub help_board: crate::board::BoardState,
    pub model_board: crate::board::BoardState,
    pub model_picker: crate::board::ListBoard,
    pub confirm: crate::board::ConfirmBoard,
    pub dashboard: crate::dashboard::Dashboard,
    pub progress: crate::board::ProgressBoard,
    pub plan_board: crate::board::BoardState,
    pub toasts: Vec<crate::board::Toast>,
    pub available_models: Vec<String>,
    pub selection: Option<(usize, usize)>,
    pub selecting: bool,
}

impl App {
    pub fn new(cmd_tx: mpsc::Sender<BackendCmd>, ui_rx: mpsc::Receiver<UiEvent>, config: &SessionConfig) -> Self {
        let mut out = vec![format!("Radiumical — {} @ {}", config.model, ".")];
        out.push(String::new());
        for line in LOGO { out.push(format!("  {line}")); }
        out.push(String::new());
        out.push("  lean CLI coding agent".into());
        out.push(String::new());
        out.push("Type a task or /help for commands.".into());
        out.push(String::new());
        Self {
            output: out, input: String::new(), cursor: 0,
            thinking: false, thinking_start: Instant::now(), thinking_elapsed: 0, thinking_frame: 0,
            hints: Vec::new(), hint_selected: None, hint_page: 0, scroll: 0.0, stick_to_bottom: true, scroll_velocity: 0.0,
            mode: config.mode.clone(), model: config.model.clone(),
            provider_name: config.provider.name().to_string(),
            cmd_tx, ui_rx, should_quit: false,
            history: Vec::new(), history_idx: None, history_draft: String::new(),
            welcome: true, show_help_overlay: true, show_model_picker: false, cod_enabled: false, thinking_effort: "max".into(), full_reasoning: Vec::new(), show_full_reasoning: false,
            help_board: crate::board::BoardState::new(" Help ", 36, 18, crate::board::Corner::BottomRight),
            model_board: crate::board::BoardState::new(" Models ", 30, 10, crate::board::Corner::BottomRight),
            model_picker: crate::board::ListBoard::new(" Models "),
            toasts: Vec::new(),
            confirm: crate::board::ConfirmBoard::new("Are you sure?"),
            dashboard: crate::dashboard::Dashboard::new(),
            progress: crate::board::ProgressBoard::new("Working"),
            plan_board: crate::board::BoardState::new(" Plan ", 30, 8, crate::board::Corner::TopRight),
            available_models: vec![config.model.clone()],
            selection: None, selecting: false,
        }
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        if key.kind == KeyEventKind::Release { return; }
        match (key.code, key.modifiers) {
            (KeyCode::Char('o'), KeyModifiers::CONTROL) => { self.show_full_reasoning = !self.show_full_reasoning; self.stick_to_bottom = true; }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => self.should_quit = true,
            (KeyCode::Char('C'), mods) if mods.contains(KeyModifiers::CONTROL) && mods.contains(KeyModifiers::SHIFT) => {
                let text = if let Some((s, e)) = self.selection { self.output[s..=e].join("\n") } else { self.output.join("\n") };
                if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); self.output.push("  [Copied]".into()); self.stick_to_bottom = true; }
                self.selection = None;
            }
            (KeyCode::PageUp, _) => { if self.hint_selected.is_some() { self.hint_page = self.hint_page.saturating_sub(1); self.hint_selected = Some(0); } else if !self.welcome { self.scroll_up(12.0); } }
            (KeyCode::PageDown, _) => { if self.hint_selected.is_some() { let max_page = self.hints.len().saturating_sub(1) / 8; self.hint_page = (self.hint_page + 1).min(max_page); self.hint_selected = Some(0); } else if !self.welcome { self.scroll_down(12.0); } }
            (KeyCode::Up, _) => {
                if self.dashboard.visible { self.dashboard.up(); return; }
                if self.input.starts_with('/') && self.hint_selected.is_some() {
                    let max = self.hints.len().saturating_sub(1);
                    self.hint_selected = Some(self.hint_selected.unwrap_or(0).saturating_sub(1).min(max));
                    self.sync_hint_page();
                } else if self.input.starts_with('/') && !self.hints.is_empty() {
                    self.hint_selected = Some(self.hints.len() - 1);
                    self.sync_hint_page();
                } else if !self.history.is_empty() {
                    if self.history_idx.is_none() { self.history_draft = self.input.clone(); }
                    let i = self.history_idx.map_or(self.history.len() - 1, |i| i.saturating_sub(1));
                    self.history_idx = Some(i); self.input = self.history[i].clone(); self.cursor = self.input.len(); self.hints.clear();
                }
            }
            (KeyCode::Down, _) => {
                if self.dashboard.visible { self.dashboard.down(); return; }
                if self.input.starts_with('/') && self.hint_selected.is_some() {
                    let max = self.hints.len().saturating_sub(1);
                    let next = (self.hint_selected.unwrap_or(0) + 1).min(max);
                    self.hint_selected = Some(next);
                    self.sync_hint_page();
                } else if self.input.starts_with('/') && !self.hints.is_empty() {
                    self.hint_selected = Some(0);
                } else if let Some(i) = self.history_idx {
                    let next = i + 1;
                    if next >= self.history.len() { self.input = self.history_draft.clone(); self.history_idx = None; }
                    else { self.input = self.history[next].clone(); self.history_idx = Some(next); }
                    self.cursor = self.input.len(); self.hints.clear();
                }
            }
            (KeyCode::Enter, KeyModifiers::SHIFT) => { self.history_idx = None; self.input.insert(self.cursor, '\n'); self.cursor += 1; self.update_hints(); }
            (KeyCode::Enter, _) => {
                if self.input.trim() == "//" { self.dashboard.toggle(); self.input.clear(); self.cursor = 0; return; }
                if self.confirm.visible { if self.confirm.yes_selected { if self.confirm.message.contains("Exit") { self.should_quit = true; } else if self.confirm.message.contains("Clear") { self.output.clear(); self.input.clear(); self.cursor = 0; self.hints.clear(); self.scroll = 0.0; self.stick_to_bottom = true; } } self.confirm.visible = false; return; }
                if self.show_model_picker { if let Some(m) = self.model_picker.current() { self.model = m.to_string(); self.toasts.push(crate::board::Toast::new(format!("Model: {m}"), crate::board::ToastLevel::Info, std::time::Duration::from_secs(3))); } self.show_model_picker = false; return; }
                if self.dashboard.visible { if let Some(cmd) = self.dashboard.selected_command() { self.input = format!("{cmd} "); self.cursor = self.input.len(); self.update_hints(); } self.dashboard.visible = false; return; }
                // If hint selection active, confirm it instead of submitting
                if let Some(idx) = self.hint_selected {
                    if let Some((name, _)) = self.hints.get(idx) {
                        self.input = format!("{} ", name);
                        self.cursor = self.input.len();
                    }
                    self.hint_selected = None;
                    self.update_hints();
                    return;
                }
                let task = self.input.trim().to_string();
                match task.as_str() {
                    "/exit" | "/quit" | "/q" => { self.should_quit = true; return; }
                    "/clear" | "/cls" => { self.output.clear(); self.output.push(String::new()); self.input.clear(); self.cursor = 0; self.hints.clear(); self.scroll = 0.0; self.stick_to_bottom = true; self.welcome = false; self.show_help_overlay = false; self.show_model_picker = false; self.hint_selected = None; self.help_board.visible = false; self.model_board.visible = false; return; }
                    "/end" | "/bottom" => { self.stick_to_bottom = true; self.scroll = 0.0; self.input.clear(); self.cursor = 0; return; }
                    "/help" | "/?" => { self.show_help_overlay = !self.show_help_overlay; if !self.welcome { self.output.push("> /help".into()); self.show_help(); } self.input.clear(); self.cursor = 0; self.hints.clear(); self.stick_to_bottom = true; return; }
                    "/settings" | "/config" => { self.output.push("> /settings".into()); self.show_settings(); self.input.clear(); self.cursor = 0; self.hints.clear(); self.stick_to_bottom = true; return; }
                    "/plan" => { self.mode = AgentMode::Plan; self.output.push("  Plan mode".into()); self.output.push(String::new()); self.input.clear(); self.cursor = 0; self.stick_to_bottom = true; return; }
                    "/exec" => { self.mode = AgentMode::Exec; self.output.push("  Exec mode".into()); self.output.push(String::new()); self.input.clear(); self.cursor = 0; self.stick_to_bottom = true; return; }
                    "/auto" => { self.mode = AgentMode::Auto; self.output.push("  Auto mode".into()); self.output.push(String::new()); self.input.clear(); self.cursor = 0; self.stick_to_bottom = true; return; }
                    _ if task.starts_with("/session") => {
                        let args: Vec<&str> = task[8..].trim().splitn(2, ' ').collect();
                        match args.get(0).copied().unwrap_or("") {
                            "save" => {
                                let name = args.get(1).copied().unwrap_or("default");
                                let jsonl = self.output.join("\n");
                                match crate::session::Session::save(name, &jsonl, &self.model) {
                                    Ok(()) => self.output.push(format!("  Session saved: {name}")),
                                    Err(e) => self.output.push(format!("  Save failed: {e}")),
                                }
                            }
                            "load" => {
                                let name = args.get(1).copied().unwrap_or("default");
                                match crate::session::Session::load(name) {
                                    Ok(Some(s)) => { self.output.clear(); for line in s.messages_jsonl.lines() { self.output.push(line.to_string()); } self.output.push(format!("  Loaded: {name}")); self.stick_to_bottom = true; }
                                    Ok(None) => self.output.push(format!("  Session not found: {name}")),
                                    Err(e) => self.output.push(format!("  Load failed: {e}")),
                                }
                            }
                            "list" => {
                                match crate::session::Session::list() {
                                    Ok(sessions) => {
                                        if sessions.is_empty() { self.output.push("  No saved sessions".into()); }
                                        else { for s in &sessions { self.output.push(format!("  {} — {} messages | {}", s.name, s.message_count, s.created)); } }
                                    }
                                    Err(e) => self.output.push(format!("  List failed: {e}")),
                                }
                            }
                            "delete" => {
                                let name = args.get(1).copied().unwrap_or("");
                                match crate::session::Session::delete(name) {
                                    Ok(true) => self.output.push(format!("  Deleted: {name}")),
                                    Ok(false) => self.output.push(format!("  Not found: {name}")),
                                    Err(e) => self.output.push(format!("  Delete failed: {e}")),
                                }
                            }
                            _ => { self.output.push("  /session save <name> | load <name> | list | delete <name>".into()); }
                        }
                        self.input.clear(); self.cursor = 0; self.stick_to_bottom = true;
                        self.output.push(String::new());
                        return;
                    }
                    _ if task.starts_with("/debug") => { let topic = task[6..].trim(); self.show_debug(topic); self.input.clear(); self.cursor = 0; self.stick_to_bottom = true; return; }
                    _ if task == "/models" => { self.show_model_picker = !self.show_model_picker; self.input.clear(); self.cursor = 0; return; }
                    _ if task == "/cod on" => { self.cod_enabled = true; self.output.push("> /cod on".into()); self.output.push("  Chain of Draft enabled".into()); self.output.push(String::new()); self.input.clear(); self.cursor = 0; self.stick_to_bottom = true; return; }
                    _ if task == "/cod off" => { self.cod_enabled = false; self.output.push("> /cod off".into()); self.output.push("  Chain of Draft disabled".into()); self.output.push(String::new()); self.input.clear(); self.cursor = 0; self.stick_to_bottom = true; return; }
                    "/think high" => { self.thinking_effort = "high".into(); self.output.push("> /think high".into()); self.output.push("  Reasoning: high".into()); self.output.push(String::new()); self.input.clear(); self.cursor = 0; self.stick_to_bottom = true; return; }
                    "/think max" | "/think xhigh" => { self.thinking_effort = "max".into(); self.output.push("> /think max".into()); self.output.push("  Reasoning: max".into()); self.output.push(String::new()); self.input.clear(); self.cursor = 0; self.stick_to_bottom = true; return; }
                    _ if task.starts_with("/model ") => { let m = task[7..].trim().to_string(); self.model = m.clone(); self.toasts.push(crate::board::Toast::new(format!("Model: {m}"), crate::board::ToastLevel::Info, std::time::Duration::from_secs(3))); self.output.push(format!("> /model {m}")); self.output.push(format!("  Model -> {m}")); self.output.push(String::new()); self.input.clear(); self.cursor = 0; self.stick_to_bottom = true; return; }
                    _ => {}
                }
                self.input.clear(); self.cursor = 0; self.hints.clear(); self.history_idx = None; self.welcome = false; self.show_help_overlay = false;
                if !task.is_empty() { self.history.push(task.clone()); self.output.push(format!("> {task}")); self.output.push(String::new()); self.stick_to_bottom = true; self.full_reasoning.clear(); self.show_full_reasoning = false;
                    let final_task = if self.cod_enabled { format!("{task}\n\n[Chain of Draft: think in <=5 word steps, be terse. Output reasoning as brief fragments, then final answer.]") } else { task };
                    let _ = self.cmd_tx.send(BackendCmd::RunTask(final_task)); }
            }
            (KeyCode::Char(ch), mods) => {
                self.history_idx = None;
                if mods.contains(KeyModifiers::CONTROL) {
                    match ch { 'w' if self.cursor > 0 => { self.delete_word_before(); } 'u' => { self.input.drain(..self.cursor); self.cursor = 0; } _ => {} }
                } else { self.input.insert(self.cursor, ch); self.cursor += ch.len_utf8(); }
                self.update_hints();
            }
            (KeyCode::Backspace, _) if self.cursor > 0 => { self.history_idx = None; let prev = self.prev_char_boundary(self.cursor); self.input.drain(prev..self.cursor); self.cursor = prev; self.update_hints(); }
            (KeyCode::Delete, _) if self.cursor < self.input.len() => { self.history_idx = None; let next = self.next_char_boundary(self.cursor); self.input.drain(self.cursor..next); self.update_hints(); }
            (KeyCode::Left, _) => {
                if self.dashboard.visible { self.dashboard.left(); return; }
                if self.cursor > 0 { self.history_idx = None; self.cursor = self.prev_char_boundary(self.cursor); }
            }
            (KeyCode::Right, _) => {
                if self.dashboard.visible { self.dashboard.left(); return; }
                if self.cursor < self.input.len() { self.history_idx = None; self.cursor = self.next_char_boundary(self.cursor); }
            }
            (KeyCode::Home, _) => { self.history_idx = None; self.cursor = 0; }
            (KeyCode::End, _) => { if self.input.is_empty() { self.stick_to_bottom = true; self.scroll = 0.0; } else { self.history_idx = None; self.cursor = self.input.len(); } }
            (KeyCode::Tab, _) => {
                if self.confirm.visible { self.confirm.toggle(); return; }
                if self.input.starts_with('/') {
                    if let Some(idx) = self.hint_selected {
                        let max = self.hints.len().saturating_sub(1);
                        self.hint_selected = Some((idx + 1).min(max));
                        self.sync_hint_page();
                    } else if !self.hints.is_empty() {
                        self.hint_selected = Some(0);
                        self.sync_hint_page();
                    }
                }
            }
            (KeyCode::BackTab, _) => {
                if self.hint_selected.is_some() {
                    self.hint_selected = Some(self.hint_selected.unwrap_or(0).saturating_sub(1));
                }
            }
            (KeyCode::Esc, _) => { if self.dashboard.visible { self.dashboard.visible = false; return; } if self.confirm.visible { self.confirm.visible = false; return; } if self.thinking { let _ = self.cmd_tx.send(BackendCmd::Cancel); self.thinking = false; } self.show_help_overlay = false; self.show_model_picker = false; self.hint_selected = None; self.hint_page = 0; self.help_board.visible = false; self.model_board.visible = false; }
            _ => {}
        }
    }

    fn prev_char_boundary(&self, pos: usize) -> usize { self.input[..pos].char_indices().next_back().map(|(i,_)| i).unwrap_or(pos.saturating_sub(1)) }
    fn next_char_boundary(&self, pos: usize) -> usize { self.input[pos..].char_indices().nth(1).map(|(i,_)| pos + i).unwrap_or(self.input.len()) }
    fn delete_word_before(&mut self) { let before = &self.input[..self.cursor]; let cut = before.char_indices().rev().find(|(_,c)| c.is_whitespace()).map(|(i,_)| i+1).unwrap_or(0); self.input.drain(cut..self.cursor); self.cursor = cut; }
    fn update_hints(&mut self) { if self.input.starts_with('/') && self.input.len() <= 30 { self.hints = matching_hints(&self.input).into_iter().map(|(n,d)| (n.to_string(), d.to_string())).collect(); } else { self.hints.clear(); } self.hint_page = 0; self.hint_selected = None; }

    fn sync_hint_page(&mut self) {
        if let Some(sel) = self.hint_selected {
            self.hint_page = sel / 8;
        }
    }


    pub fn handle_mouse(&mut self, kind: MouseEventKind, row: u16, _col: u16, output_top: u16) {
        if self.welcome { return; }
        match kind {
            MouseEventKind::ScrollDown => self.scroll_up(1.0),
            MouseEventKind::ScrollUp => self.scroll_down(1.0),
            MouseEventKind::Down(_) => {
                // Check if clicking on help board border for resize
                if self.help_board.hit_border(row, _col, Rect { x: 0, y: output_top, width: 80, height: 24 }) {
                    self.help_board.start_drag(_col, row);
                    return;
                }
                if row >= output_top { let line = self.screen_to_output(row, output_top); self.selection = Some((line, line)); self.selecting = true; }
            }
            MouseEventKind::Drag(_) if self.selecting => {
                if self.help_board.dragging {
                    self.help_board.drag_to(_col, row, Rect { x: 0, y: 0, width: 80, height: 24 });
                    return;
                } if row >= output_top { let line = self.screen_to_output(row, output_top); if let Some((start, _)) = self.selection { self.selection = Some((start.min(line), start.max(line))); } } }
            MouseEventKind::Up(_) => { self.selecting = false; }
            _ => {}
        }
    }
    fn screen_to_output(&self, screen_row: u16, output_top: u16) -> usize { let rel = screen_row.saturating_sub(output_top) as usize; let total = self.output.len(); let vis = 20; let start = if self.stick_to_bottom { total.saturating_sub(vis) } else { (self.scroll as usize).min(total.saturating_sub(1)) }; (start + rel).min(total.saturating_sub(1)) }

    /// Check if a global line index is within the visible output window.
    pub fn inside_window(&self, global_i: usize, vis: usize) -> bool {
        let total = self.output.len();
        let start = if self.stick_to_bottom { total.saturating_sub(vis) } else { (self.scroll as usize).min(total.saturating_sub(1)) };
        let end = (start + vis).min(total);
        global_i >= start && global_i < end
    }

    pub fn scroll_up(&mut self, lines: f32) { self.scroll += lines; self.stick_to_bottom = false; self.scroll_velocity = lines; }
    pub fn scroll_down(&mut self, lines: f32) { self.scroll = (self.scroll - lines).max(0.0); if self.scroll <= 0.0 { self.scroll = 0.0; self.stick_to_bottom = true; } self.scroll_velocity = -lines; }

    fn show_help(&mut self) {
        self.output.push("".into());
        self.output.push("  Commands:".into());
        let max_w = SLASH_COMMANDS.iter().map(|(n,_)| n.len()).max().unwrap_or(10);
        for (n, d) in SLASH_COMMANDS { self.output.push(format!("  {n:<w$}  {d}", w = max_w)); }
        self.output.push("".into());
        self.output.push("  Keys:".into());
        self.output.push("  PgUp/PgDn  Scroll | Up/Down  History".into());
        self.output.push("  Ctrl+W     Del word | Shift+Enter  Newline".into());
        self.output.push("  End        Jump to bottom (empty input)".into());
        self.output.push("  Mouse drag Select | Ctrl+Shift+C  Copy".into());
        self.output.push("  Ctrl+C     Quit".into());
        self.output.push(String::new());
    }
    fn show_settings(&mut self) { self.output.push("".into()); self.output.push(format!("  Provider : {}", self.provider_name)); self.output.push(format!("  Model    : {}", self.model)); self.output.push(format!("  Mode     : {:?}", self.mode)); self.output.push(format!("  History  : {} items", self.history.len())); self.output.push(String::new()); }
    fn show_debug(&mut self, topic: &str) { self.output.push(format!("> /debug {topic}")); self.output.push(String::new()); match topic { "logo" => { for line in LOGO { self.output.push(format!("  [{:>2}] {line}", line.chars().count())); } } "output" => { self.output.push(format!("  Lines: {} | Scroll: {:.1} | Stick: {}", self.output.len(), self.scroll, self.stick_to_bottom)); } "blocks" => { let blocks = crate::layout::measure_blocks(&self.output); self.output.push(format!("  Blocks: {}", blocks.len())); for (i, b) in blocks.iter().enumerate() { self.output.push(format!("    [{i}] {:?} h={}", b.kind, b.height)); } } "" | "help" => { self.output.push("  logo | output | blocks".into()); } _ => { self.output.push(format!("  Unknown: {topic}").into()); } } self.output.push(String::new()); }

    pub fn handle_ui_event(&mut self, event: UiEvent) {
        match event {
            UiEvent::LlmChunk(chunk) => { let chunk = chunk.replace("\r\n", "\n").replace('\r', ""); if let Some(last) = self.output.last() { if last.starts_with("\x01") { self.output.push(String::new()); } } for ch in chunk.chars() { if ch == '\n' { self.output.push(String::new()); } else { if self.output.is_empty() { self.output.push(String::new()); } self.output.last_mut().unwrap().push(ch); } } }
            UiEvent::LlmReasoning(rc) => { if let Some(last) = self.output.last_mut() { if last.starts_with("\x01") { last.push_str(&rc); return; } } self.output.push(format!("\x01{}", rc)); }
            UiEvent::ThinkingTick => { if !self.thinking { self.thinking_start = Instant::now(); } self.thinking = true; }
            UiEvent::LlmDone => { if self.output.last().map_or(true, |l| !l.is_empty()) { self.output.push(String::new()); } }
            UiEvent::ToolStart { name, index, total, args } => {
                self.progress.visible = true;
                self.progress.label = name.clone();
                self.progress.progress = index as f32 / total.max(1) as f32; let w = 56usize; let header = if total > 1 { format!("{} ({}/{})", name, index + 1, total) } else { name }; let fill = w.saturating_sub(header.len() + 2); self.output.push(format!("  ┌─ {header} {}", "─".repeat(fill))); let sa: String = args.chars().take(w.saturating_sub(2)).collect(); let dots = if args.chars().count() > w.saturating_sub(2) { "…" } else { "" }; self.output.push(format!("  │ {sa}{dots}")); }
            UiEvent::ToolDone => { let w = 56usize; self.output.push(format!("  └{}┘", "─".repeat(w))); self.output.push(String::new()); self.progress.visible = false; }
            UiEvent::Error(e) => { self.output.push(format!("  {e}")); self.thinking = false; }
            UiEvent::ThinkingDone => { self.thinking = false; }
        }
    }

    pub fn tick(&mut self, _visible_lines: usize) { if self.thinking { self.thinking_frame += 1; self.thinking_elapsed = self.thinking_start.elapsed().as_secs(); } if self.scroll_velocity.abs() > 0.01 && !self.stick_to_bottom { self.scroll += self.scroll_velocity; self.scroll = self.scroll.max(0.0); self.scroll_velocity *= 0.85; } if self.stick_to_bottom { self.scroll = 0.0; } if self.output.len() > _visible_lines { let max = (self.output.len() - _visible_lines) as f32; self.scroll = self.scroll.min(max); } }
}

