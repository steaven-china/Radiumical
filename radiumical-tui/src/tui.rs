//! Ratatui TUI — async frontend/backend.
use crossterm::event::Event;
use radiumical_core::types::SessionConfig;
use std::sync::mpsc;
use std::time::Instant;

// ═══ Channels ═══

pub use radiumical_core::types::{BackendCmd, UiEvent};

// ═══ Slash hints ═══

const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show this help"),
    ("/plan", "Read-only mode"),
    ("/exec", "Write mode"),
    ("/auto", "Full auto mode"),
    ("/review", "Self-review changes"),
    ("/tools", "List available tools"),
    ("/settings", "Show configuration"),
    ("/provider", "Provider/model picker"),
    ("/models", "Model picker panel"),
    ("/model <n>", "Switch model"),
    ("/new", "New session / clear context"),
    ("/session", "Save/load sessions (commands)"),
    ("/sessions", "Session manager TUI"),
    ("/skills", "List available skills"),
    ("/skill <n>", "Activate/deactivate skill"),
    ("/cod on/off", "Chain of Draft experimental"),
    ("/debug <t>", "Debug info"),
    ("/end", "Jump to bottom"),
    ("/clear", "Clear screen"),
    ("/exit", "Quit"),
];

pub fn matching_hints(prefix: &str) -> Vec<(&'static str, &'static str)> {
    SLASH_COMMANDS
        .iter()
        .filter(|(n, _)| n.starts_with(prefix))
        .copied()
        .collect()
}

pub fn _complete_slash(prefix: &str) -> Option<String> {
    let m: Vec<&str> = SLASH_COMMANDS
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| n.starts_with(prefix) && *n != prefix)
        .collect();
    if m.len() == 1 {
        Some(format!("{} ", m[0]))
    } else {
        None
    }
}

// ═══ Pulse ═══

pub const PULSE: &[&str] = &[
    "    ",
    "░   ",
    "▒░  ",
    "▓▒░ ",
    "▓▓▒░",
    "▓▓▓▒",
    "▓▓▓▓",
    "▒▓▓▓",
    "░▒▓▓",
    " ░▒▓",
    "  ░▒",
    "   ░",
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

pub fn run(
    cmd_tx: tokio::sync::mpsc::Sender<BackendCmd>,
    ui_rx: mpsc::Receiver<UiEvent>,
    config: SessionConfig,
) -> anyhow::Result<()> {
    use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::Terminal;
    use std::io;
    use std::time::Duration;

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    // Restore terminal on panic so error messages are visible
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        orig_hook(info);
    }));

    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(cmd_tx, ui_rx, &config);
    // Auto-load previous session if available
    if let Ok(Some((_, items))) = radiumical_core::session::Session::load("autosave") {
        if !items.is_empty() {
            app.session_items = items;
            app.render_session_items_to_output();
        }
    }
    let frame_time = Duration::from_nanos(16_666_667); // 60 FPS
    let mut term_size = terminal.size()?;

    let result = (|| -> anyhow::Result<()> {
        let t0 = Instant::now();
        terminal.draw(|f| draw::draw(f, &mut app))?;
        radiumical_core::perf::tick(t0.elapsed().as_micros() as u64, app.output.len());

        let mut next_frame = Instant::now() + frame_time;
        loop {
            let hint_page_start = app.hint_page * 8;
            let hint_page_end = (hint_page_start + 8).min(app.hints.len());
            let hint_count = hint_page_end.saturating_sub(hint_page_start);
            let input_lines = app.input.split('\n').count().max(1).min(5);
            let bottom_h = ((input_lines + 2) + hint_count + 1)
                .min(term_size.height.saturating_sub(1) as usize) as u16;
            let out_h = term_size.height.saturating_sub(bottom_h).max(1) as usize;

            // Drain ALL pending input events (non-blocking batch)
            while crossterm::event::poll(Duration::ZERO)? {
                match crossterm::event::read()? {
                    Event::Key(key) => app.handle_key(key),
                    Event::Mouse(m) => {
                        let output_top = 0u16;
                        let output_h = out_h as u16;
                        app.handle_mouse(m.kind, m.row, m.column, output_top, output_h);
                    }
                    Event::Resize(w, h) => {
                        term_size = ratatui::layout::Size {
                            width: w,
                            height: h,
                        };
                    }
                    _ => {}
                }
            }
            while let Ok(ev) = app.ui_rx.try_recv() {
                app.handle_ui_event(ev);
            }

            app.tick(out_h);
            let t0 = Instant::now();
            terminal.draw(|f| draw::draw(f, &mut app))?;
            radiumical_core::perf::tick(t0.elapsed().as_micros() as u64, app.output.len());

            if app.should_quit {
                break;
            }
            // Sleep to maintain exactly 60 FPS
            let now = Instant::now();
            if now < next_frame {
                std::thread::sleep(next_frame - now);
            }
            next_frame = Instant::now() + frame_time;
        }
        Ok(())
    })();

    let desc = app.history.first().cloned();
    let mode: radiumical_core::session::SessionMode = app.mode.clone().into();
    let _ = app.session_pool.save(
        "autosave",
        &app.session_items,
        &app.model,
        &app.provider_name,
        mode,
        &app.thinking_effort,
        desc.as_deref(),
    );
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    disable_raw_mode()?;
    result
}
