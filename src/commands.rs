//! Slash command registry for the REPL.
use crate::types::{AgentMode, SessionConfig};

/// Outcome of a slash command: either handled internally, or forward to agent.
pub enum CommandOutcome {
    Continue,
    Exit,
    Agent(String),
}

pub struct CommandPool {
    entries: Vec<CommandEntry>,
}

struct CommandEntry {
    names: &'static [&'static str],
    handler: fn(&mut SessionConfig, &str) -> CommandOutcome,
}

impl CommandPool {
    pub fn new() -> Self {
        let mut pool = Self {
            entries: Vec::new(),
        };

        pool.add(&["/help", "/?"], |_, _| {
            eprintln!("  /plan     Read-only mode (explore, no edits)");
            eprintln!("  /exec     Write mode (make changes)");
            eprintln!("  /review   Self-review recent changes");
            eprintln!("  /clear    Clear screen");
            eprintln!("  /auto     Back to full auto mode");
            eprintln!("  /tools    List available tools");
            eprintln!("  /exit,/q  Quit");
            CommandOutcome::Continue
        });

        pool.add(&["/plan"], |cfg, _| {
            cfg.mode = AgentMode::Plan;
            eprintln!("  📖 Plan mode — read-only tools only");
            CommandOutcome::Continue
        });

        pool.add(&["/exec"], |cfg, _| {
            cfg.mode = AgentMode::Exec;
            eprintln!("  ✏️  Exec mode — all tools enabled");
            CommandOutcome::Continue
        });

        pool.add(&["/auto"], |cfg, _| {
            cfg.mode = AgentMode::Auto;
            eprintln!("  🔄 Auto mode — full capabilities");
            CommandOutcome::Continue
        });

        pool.add(&["/clear", "/cls"], |_, _| {
            print!("\x1b[2J\x1b[H");
            CommandOutcome::Continue
        });

        pool.add(&["/exit", "/quit", "/q"], |_, _| {
            eprintln!("Bye!");
            CommandOutcome::Exit
        });

        pool.add(&["/review"], |_, _| {
            let task = "Review the last changes made in this session. Check for: (1) bugs or logic errors, (2) style consistency, (3) missing tests, (4) dead code. Report findings concisely.";
            CommandOutcome::Agent(task.to_string())
        });

        pool.add(&["/tools", "/t"], |cfg, _| {
            use crate::tools::all_tools;
            let tools = all_tools();
            eprintln!("  Available tools ({}):", tools.len());
            for t in &tools {
                let def = t.definition();
                let marker = match cfg.mode {
                    AgentMode::Plan => {
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
                eprintln!(
                    "    {} {:<14} {}",
                    marker,
                    def.function.name,
                    truncate50(&def.function.description)
                );
            }
            CommandOutcome::Continue
        });

        pool
    }

    fn add(
        &mut self,
        names: &'static [&'static str],
        handler: fn(&mut SessionConfig, &str) -> CommandOutcome,
    ) {
        self.entries.push(CommandEntry {
            names,
            handler,
        });
    }

    /// Try to match input against known slash commands.
    pub fn dispatch(&self, input: &str, config: &mut SessionConfig) -> CommandOutcome {
        let trimmed = input.trim();
        for entry in &self.entries {
            if entry.names.iter().any(|n| *n == trimmed) {
                return (entry.handler)(config, trimmed);
            }
        }
        // Also match bare "exit", "quit", "q" (without slash)
        match trimmed {
            "exit" | "quit" | "q" => {
                eprintln!("Bye!");
                return CommandOutcome::Exit;
            }
            _ => {}
        }
        CommandOutcome::Agent(trimmed.to_string())
    }
}

fn truncate50(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 50 {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(47).collect::<String>())
    }
}
