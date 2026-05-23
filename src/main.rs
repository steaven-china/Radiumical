mod board;
mod commands;
mod config;
mod conversation;
mod dashboard;
mod hooks;
mod layout;
mod lsp;
mod markdown;
mod pipeline;
mod provider;
mod session;
mod systools;
mod tui;
mod tools;
mod types;

use anyhow::Result;
use clap::Parser;
use commands::{CommandOutcome, CommandPool};
use pipeline::PipelineRunner;
use provider::create_provider;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use tui::{BackendCmd, UiEvent};
use types::{ProviderKind, SessionConfig};

/// Radiumical — a lean, fast CLI coding agent.
#[derive(Parser, Debug)]
#[command(name = "radiumical", version, about, long_about = None)]
struct Cli {
    /// Task to execute (if omitted, enters interactive mode)
    #[arg(short = 't', long)]
    task: Option<String>,

    /// Workspace directory (defaults to current directory)
    #[arg(short = 'w', long, default_value = ".")]
    workspace: PathBuf,

    /// Provider: openai, deepseek, anthropic, ollama
    #[arg(short = 'p', long, default_value = "deepseek")]
    provider: String,

    /// Model name (auto-selected based on provider if not set)
    #[arg(short = 'm', long)]
    model: Option<String>,

    /// API key
    #[arg(short = 'k', long)]
    api_key: Option<String>,

    /// API base URL
    #[arg(long)]
    api_base: Option<String>,

    /// Max tool-calling iterations
    #[arg(long, default_value = "32")]
    max_iterations: usize,

    /// Max parallel tool executions
    #[arg(long, default_value = "8")]
    concurrency: usize,

    /// LLM request timeout in seconds
    #[arg(long, default_value = "120")]
    llm_timeout: u64,

    /// Tool execution timeout in seconds
    #[arg(long, default_value = "300")]
    tool_timeout: u64,

    /// Heartbeat interval in seconds (0 = disable)
    #[arg(long, default_value = "10")]
    heartbeat: u64,

    /// Verbose output
    #[arg(short = 'v', long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_level = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_target(false)
        .init();

    let provider_kind = match cli.provider.to_lowercase().as_str() {
        "openai" => ProviderKind::OpenAI,
        "deepseek" => ProviderKind::OpenAI,
        "anthropic" => ProviderKind::Anthropic,
        "ollama" => ProviderKind::Ollama,
        _ => ProviderKind::OpenAI,
    };

    let model = cli.model.or_else(|| {
        let cfg = config::Config::load().ok();
        cfg.and_then(|c| c.model)
    }).unwrap_or_else(|| {
        if cli.provider.to_lowercase() == "deepseek" {
            "deepseek-v4-pro".into()
        } else {
            provider_kind.default_model().into()
        }
    });

    let api_key = cli.api_key.unwrap_or_else(|| {
        std::env::var("RADIUMICAL_API_KEY")
            .or_else(|_| std::env::var("DEEPSEEK_API_KEY"))
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_default()
    });

    if api_key.is_empty() {
        eprintln!("⚠️  No API key. Set DEEPSEEK_API_KEY or use --api-key.");
        std::process::exit(1);
    }

    let api_base = cli.api_base.or_else(|| {
        if cli.provider.to_lowercase() == "deepseek" {
            Some("https://api.deepseek.com/v1".into())
        } else {
            None
        }
    });

    let workspace = std::fs::canonicalize(&cli.workspace).unwrap_or_else(|_| cli.workspace.clone());

    let mut config = SessionConfig {
        provider: provider_kind,
        model,
        api_key: api_key.clone(),
        api_base: api_base.clone(),
        max_iterations: cli.max_iterations,
        system_prompt: types::default_system_prompt(),
        llm_timeout_secs: cli.llm_timeout,
        tool_timeout_secs: cli.tool_timeout,
        heartbeat_interval_secs: cli.heartbeat,
        concurrency: cli.concurrency,
        use_markdown: false, // TUI handles rendering
        mode: types::AgentMode::Auto,
    };

    let provider = create_provider(
        &config.provider,
        config.api_base.as_deref(),
        &config.api_key,
        &config.model,
    );

    // ── Channels for frontend ↔ backend communication ──
    let (ui_tx, ui_rx) = mpsc::channel::<UiEvent>();
    let (cmd_tx, cmd_rx) = mpsc::channel::<BackendCmd>();

    // ── Non-interactive mode (--task) ──
    if let Some(task) = cli.task {
        let mut runner = PipelineRunner::new(config.clone(), provider);
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        runner
            .run(task, workspace, None, ui_tx, cancel_rx)
            .await?;
        return Ok(());
    }

    // ── Interactive mode: spawn TUI on a separate thread ──
    let tui_config = config.clone();
    let tui_handle = std::thread::spawn(move || {
        if let Err(e) = tui::run(cmd_tx, ui_rx, tui_config) {
            eprintln!("TUI error: {e}");
        }
    });

    // ── Create runner ONCE (persistent conversation across turns) ──
    let mut runner = PipelineRunner::new(config.clone(), Arc::clone(&provider));

    // ── Backend loop (this tokio thread) ──
    let cmd_pool = CommandPool::new();
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    loop {
        let cmd = match cmd_rx.recv() {
            Ok(c) => c,
            Err(_) => break, // TUI closed
        };

        match cmd {
            BackendCmd::Cancel => {
                let _ = cancel_tx.send(true);
                let _ = ui_tx.send(UiEvent::ThinkingDone);
            }
            BackendCmd::RunTask(text) => {
                let _ = cancel_tx.send(false); // reset cancel
                // Check for slash commands first
                match cmd_pool.dispatch(&text, &mut config) {
                    CommandOutcome::Exit => {
                        break;
                    }
                    CommandOutcome::Continue => {
                        continue;
                    }
                    CommandOutcome::Agent(task) => {
                        let heartbeat_interval = config.heartbeat_interval_secs;
                        let hb_cancel = if heartbeat_interval > 0 {
                            let (hb_tx, mut hb_rx) = tokio::sync::mpsc::unbounded_channel();
                            let ui_tx_hb = ui_tx.clone();
                            tokio::spawn(async move {
                                let mut interval = tokio::time::interval(
                                    std::time::Duration::from_secs(heartbeat_interval),
                                );
                                loop {
                                    tokio::select! {
                                        _ = interval.tick() => {
                                            let _ = ui_tx_hb.send(UiEvent::ThinkingTick);
                                        }
                                        _ = hb_rx.recv() => break,
                                    }
                                }
                            });
                            Some(hb_tx)
                        } else {
                            None
                        };

                        if let Err(e) = runner.run(task, workspace.clone(), hb_cancel, ui_tx.clone(), cancel_rx.clone()).await {
                            let _ = ui_tx.send(UiEvent::Error(e.to_string()));
                        }
                        let _ = ui_tx.send(UiEvent::ThinkingDone);
                    }
                }
            }
        }
    }

    tui_handle.join().ok();
    Ok(())
}
