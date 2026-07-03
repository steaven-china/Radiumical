use std::time::Instant;

pub const TIPS: &[Tip] = &[
    // ── Keyboard shortcuts ──
    Tip { text: "Ctrl+C cancels the current task — press again to quit", category: "keys" },
    Tip { text: "Ctrl+L clears the screen output", category: "keys" },
    Tip { text: "Ctrl+A / Ctrl+E jump to line start / end", category: "keys" },
    Tip { text: "Ctrl+W deletes the word before the cursor", category: "keys" },
    Tip { text: "Ctrl+O toggles full reasoning display", category: "keys" },
    Tip { text: "Shift+Enter inserts a newline in the input", category: "keys" },
    Tip { text: "Tab autocompletes slash commands", category: "keys" },
    Tip { text: "Up/Down arrows browse input history — prefix-filtered", category: "keys" },
    Tip { text: "PgUp / PgDn scroll the output pane", category: "keys" },
    Tip { text: "End with empty input jumps to the bottom", category: "keys" },
    // ── Commands ──
    Tip { text: "Type // to open the dashboard navigation panel", category: "cmd" },
    Tip { text: "/retry (or /r) re-sends your last request", category: "cmd" },
    Tip { text: "/status shows current model, mode, and session info", category: "cmd" },
    Tip { text: "/copy copies the last assistant response to clipboard", category: "cmd" },
    Tip { text: "/sessions opens the visual session manager", category: "cmd" },
    Tip { text: "/provider lets you switch model without restarting", category: "cmd" },
    Tip { text: "/review asks the agent to review changes in this session", category: "cmd" },
    Tip { text: "/think shows current reasoning effort — /think high to change", category: "cmd" },
    Tip { text: "/cod on enables Chain of Draft — terse step-by-step reasoning", category: "cmd" },
    Tip { text: "/env set KEY=VALUE stores API keys securely", category: "cmd" },
    Tip { text: "/diagnostics runs lint and LSP checks via the agent", category: "cmd" },
    Tip { text: "/new starts a fresh session — auto-saves the current one", category: "cmd" },
    // ── Workflow tips ──
    Tip { text: "Use /plan mode to let the agent explore code without writing", category: "flow" },
    Tip { text: "Use /exec mode when you trust the agent to write freely", category: "flow" },
    Tip { text: "Double-click a tool call box to expand its result", category: "flow" },
    Tip { text: "Use /sessions to save and restore conversation context", category: "flow" },
    Tip { text: "Ask the agent to /review after making changes", category: "flow" },
    Tip { text: "Use /remember to store notes across sessions", category: "flow" },
    Tip { text: "MCP tools appear automatically from ~/.radi/mcp.json", category: "flow" },
    Tip { text: "Skills in ~/.radi/skills/ are loaded on demand by the agent", category: "flow" },
    Tip { text: "Context compresses automatically at 80% of max tokens", category: "flow" },
    Tip { text: "Session data is stored under ~/.radi/sessions/", category: "flow" },
];

pub struct Tip {
    pub text: &'static str,
    #[allow(dead_code)]
    pub category: &'static str,
}

pub struct TipState {
    pub current: usize,
    pub shown_count: usize,
    pub last_rotate: Instant,
    pub interval_secs: u64,
    pub enabled: bool,
}

impl TipState {
    pub fn new() -> Self {
        Self {
            current: pseudo_random_index(),
            shown_count: 0,
            last_rotate: Instant::now(),
            interval_secs: 45,
            enabled: true,
        }
    }

    pub fn text(&self) -> &str {
        TIPS[self.current % TIPS.len()].text
    }

    #[allow(dead_code)]
    pub fn category(&self) -> &str {
        TIPS[self.current % TIPS.len()].category
    }

    pub fn should_rotate(&self) -> bool {
        self.enabled && self.last_rotate.elapsed().as_secs() >= self.interval_secs
    }

    pub fn rotate(&mut self) {
        self.current = (self.current + 1) % TIPS.len();
        self.shown_count += 1;
        self.last_rotate = Instant::now();
    }

    pub fn next(&mut self) {
        self.rotate();
    }

    pub fn toggle(&mut self) -> bool {
        self.enabled = !self.enabled;
        self.enabled
    }
}

fn pseudo_random_index() -> usize {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;
    nanos % TIPS.len()
}
