//! Slash-command router for the TUI input line.
//!
//! Dispatches `/`-prefixed commands to dedicated handler methods split
//! across sub-modules by domain (session, agent/mode, view, utility).
//! Unrecognised input is forwarded to the backend as a new task.

mod agent;
mod session;
mod util;
mod view;

use crate::tui::app::App;

fn base64_encode(s: &str) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

impl App {
    pub(crate) fn handle_command(&mut self, task: &str) {
        let handled = match task {
            // ── exit ──
            "/exit" | "/quit" | "/q" => self.cmd_exit(),

            // ── session ──
            "/new" => self.cmd_new(),
            "/clear" | "/cls" => self.cmd_clear(),
            _ if task == "/sessions" || task == "/session tui" => self.cmd_sessions_tui(),
            _ if task == "/ws" || task == "/session ws-tui" => self.cmd_ws_tui(),
            _ if task == "/session" => self.cmd_session_help(),
            _ if task.starts_with("/session") => self.cmd_session(task),

            // ── agent / mode ──
            "/plan" => self.cmd_plan(),
            "/plan vis" => self.cmd_plan_vis(),
            "/plan show" => self.cmd_plan_show(),
            "/agents" => self.cmd_agents(),
            _ if task.starts_with("/agents ") => self.cmd_agents_name(task),
            "/exec" => self.cmd_exec(),
            "/auto" => self.cmd_auto(),
            "/review" => self.cmd_review(),
            "/think high" => self.cmd_think_high(),
            "/think max" | "/think xhigh" => self.cmd_think_max(),
            "/think" | "/think low" => self.cmd_think(task),

            // ── view / config ──
            "/help" | "/?" => self.cmd_help(),
            "/settings" | "/config" => self.cmd_settings(),
            "/provider" => self.cmd_provider(),
            _ if task == "/models" => self.cmd_models(),
            _ if task.starts_with("/model ") => self.cmd_model(task),
            "/tools" => self.cmd_tools(),
            "/skills" => self.cmd_skills(),
            _ if task.starts_with("/skill ") => self.cmd_skill(task),
            "/perf" => self.cmd_perf(),
            "/debug linevis" => self.cmd_debug_linevis(),
            _ if task.starts_with("/debug") => self.cmd_debug(task),
            "/outline" | "/lint" | "/diagnostics" => self.cmd_diagnostics(),
            "/timeline" => self.cmd_timeline(),
            _ if task.starts_with("/image") => self.cmd_image(task),

            // ── utility ──
            "/end" | "/bottom" => self.cmd_end(),
            _ if task == "/memory" => self.cmd_memory(),
            _ if task.starts_with("/memory search ") => self.cmd_memory_search(task),
            _ if task.starts_with("/memory clear ") => self.cmd_memory_clear(task),
            _ if task.starts_with("/remember ") => self.cmd_remember(task),
            "/subagents" => self.cmd_subagents(),
            "/mcp" => self.cmd_mcp(),
            _ if task.starts_with("/mcp ") => self.cmd_mcp_toggle(task),
            _ if task.starts_with("/env") => self.cmd_env(task),
            _ if task == "/cod on" => self.cmd_cod_on(),
            _ if task == "/cod off" => self.cmd_cod_off(),
            "/status" | "/info" => self.cmd_status(),
            "/retry" | "/r" => self.cmd_retry(),
            "/copy" => self.cmd_copy(),
            "/tips" => self.cmd_tips(),
            "/tip next" | "/tip" => self.cmd_tip(),

            _ => false,
        };

        if handled {
            return;
        }

        self.input.text.clear();
        self.input.cursor = 0;
        self.input.hints.clear();
        self.input.history_idx = None;
        self.input.history_filter_prefix = None;
        self.welcome = false;
        self.overlays.help = false;
        let has_images = !self.input.pending_images.is_empty();
        if !task.is_empty() || has_images {
            self.input.history.push(task.to_string());
            let mut display = task.to_string();
            if has_images {
                display.push_str("\n[attached images:\n");
                for (_, placeholder) in &self.input.pending_images {
                    display.push_str(&format!("  - {}\n", placeholder));
                }
                display.push(']');
            }
            self.session_items
                .push(radiumical_core::session::SessionItem::User {
                    content: display.clone(),
                });
            for line in display.lines() {
                self.output.push(format!("> {line}"));
            }
            self.output.push(String::new());
            self.viewport.stick_to_bottom = true;
            self.thinking.full_reasoning.clear();
            self.thinking.show_full_reasoning = false;
            self.thinking.cancelled = false;

            // Filter out deleted temp images and replace their placeholders.
            let mut final_task = if self.thinking.cod_enabled && !task.is_empty() {
                format!("{task}\n\n[Chain of Draft: think in <=5 word steps, be terse. Output reasoning as brief fragments, then final answer.]")
            } else {
                task.to_string()
            };
            let mut images = Vec::new();
            let pending = std::mem::take(&mut self.input.pending_images);
            for (path, placeholder) in pending {
                if path.exists() {
                    images.push(path);
                } else {
                    final_task = final_task.replace(
                        &placeholder,
                        "[this file has been deleted at the user's computer]",
                    );
                }
            }

            let cmd = if images.is_empty() {
                crate::tui::BackendCmd::RunTask(final_task)
            } else {
                crate::tui::BackendCmd::RunTaskWithImages {
                    task: final_task,
                    images,
                }
            };
            let _ = self.cmd_tx.blocking_send(cmd);
        }
        self.input.text.clear();
        self.input.cursor = 0;
        self.input.hints.clear();
        self.input.history_idx = None;
        self.input.history_filter_prefix = None;
    }
}
