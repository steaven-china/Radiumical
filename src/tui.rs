//! Ratatui TUI — async frontend/backend.
use crate::types::SessionConfig;
use crossterm::event::Event;
use std::sync::mpsc;
use std::time::Instant;

// ═══ Channels ═══

#[derive(Debug, Clone)]
pub enum UiEvent {
    LlmChunk(String),
    LlmReasoning(String),
    ThinkingTick,
    LlmDone,
    ToolStart { name: String, index: usize, total: usize, args: String },
    ToolDone,
    Error(String),
    ThinkingDone,
}

#[derive(Debug, Clone)]
pub enum BackendCmd {
    RunTask(String),
    Cancel,
}

// ═══ Slash hints ═══

const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show this help"),
    ("/plan", "Read-only mode"),
    ("/exec", "Write mode"),
    ("/auto", "Full auto mode"),
    ("/review", "Self-review changes"),
    ("/tools", "List available tools"),
    ("/settings", "Show configuration"),
    ("/models", "Model picker panel"),
    ("/model <n>", "Switch model"),
    ("/session", "Save/load sessions"),
    ("/cod on/off", "Chain of Draft experimental"),
    ("/debug <t>", "Debug info"),
    ("/end", "Jump to bottom"),
    ("/clear", "Clear screen"),
    ("/exit", "Quit"),
];

pub fn matching_hints(prefix: &str) -> Vec<(&'static str, &'static str)> {
    SLASH_COMMANDS.iter().filter(|(n, _)| n.starts_with(prefix)).copied().collect()
}

pub fn _complete_slash(prefix: &str) -> Option<String> {
    let m: Vec<&str> = SLASH_COMMANDS.iter().map(|(n, _)| *n).filter(|n| n.starts_with(prefix) && *n != prefix).collect();
    if m.len() == 1 { Some(format!("{} ", m[0])) } else { None }
}

// ═══ Pulse ═══

pub const PULSE: &[&str] = &[
    "    ", "░   ", "▒░  ", "▓▒░ ", "▓▓▒░", "▓▓▓▒",
    "▓▓▓▓", "▒▓▓▓", "░▒▓▓", " ░▒▓", "  ░▒", "   ░",
];

// ═══ Logo ═══

pub const LOGO: &[&str] = &[
    "██████╗  █████╗ ██████╗ ██╗██╗   ██╗███╗   ███╗██╗ ██████╗ █████╗ ██╗     ",
    "██╔══██╗██╔══██╗██╔══██╗██║██║   ██║████╗ ████║██║██╔════╝██╔══██╗██║     ",
    "██████╔╝███████║██║  ██║██║██║   ██║██╔████╔██║██║██║     ███████║██║     ",
    "██╔══██╗██╔══██║██║  ██║██║██║   ██║██║╚██╔╝██║██║██║     ██╔══██║██║     ",
    "██║  ██║██║  ██║██████╔╝██║╚██████╔╝██║ ╚═╝ ██║██║╚██████╗██║  ██║███████╗",
    "╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝ ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚═╝ ╚═════╝╚═╝  ╚═╝╚══════╝",
];

pub mod app;
pub mod draw;

use self::app::App;

// ═══ TUI runner ═══

pub fn run(cmd_tx: mpsc::Sender<BackendCmd>, ui_rx: mpsc::Receiver<UiEvent>, config: SessionConfig) -> anyhow::Result<()> {
    use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
    use crossterm::execute;
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
    use ratatui::Terminal;
    use std::io;
    use std::time::Duration;

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(cmd_tx, ui_rx, &config);
    let tick_rate = Duration::from_millis(50);
    let mut last_tick = Instant::now();

    let result = (|| -> anyhow::Result<()> {
        let term_size = terminal.size()?;
        let out_h_init = term_size.height.saturating_sub(5) as usize;
        let t0 = Instant::now();
        terminal.draw(|f| draw::draw(f, &mut app, out_h_init))?;
        crate::perf::tick(t0.elapsed().as_micros() as u64, app.output.len());

        loop {
            let timeout = tick_rate.checked_sub(last_tick.elapsed()).unwrap_or(Duration::ZERO);
            if crossterm::event::poll(timeout)? {
                match crossterm::event::read()? {
                    Event::Key(key) => app.handle_key(key),
                    Event::Mouse(m) => app.handle_mouse(m.kind, m.row, m.column, 0),
                    Event::Resize(_, _) => {}
                    _ => {}
                }
            }
            while let Ok(ev) = app.ui_rx.try_recv() { app.handle_ui_event(ev); }

            let hint_count = app.hints.len().min(8);
            let input_lines = app.input.split('\n').count().max(1).min(5);
            let bottom_h = ((input_lines + 2) + hint_count + 1).min(term_size.height.saturating_sub(2) as usize) as u16;
            let out_h = term_size.height.saturating_sub(bottom_h) as usize;
            app.tick(out_h);
            let t0 = Instant::now();
            terminal.draw(|f| draw::draw(f, &mut app, out_h))?;
            crate::perf::tick(t0.elapsed().as_micros() as u64, app.output.len());

            last_tick = Instant::now();
            if app.should_quit { break; }
        }
        Ok(())
    })();

    let jsonl = app.output.join("\n");
    let _ = crate::session::Session::save("autosave", &jsonl, &app.model);
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    disable_raw_mode()?;
    result
}
