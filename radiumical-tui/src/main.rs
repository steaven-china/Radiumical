mod board;
mod choice_panel;
mod dashboard;
mod layout;
mod markdown;
mod panel;
mod panels;
mod session_tui;
mod settings;
mod tips;
mod tui;

use anyhow::Result;
use clap::Parser;
use radiumical_core::commands::{CommandOutcome, CommandPool};
use radiumical_core::pipeline::PipelineRunner;
use radiumical_core::provider::create_provider;
use radiumical_core::providers::{discover_models, ProviderRegistry, DEFAULT_REGISTRY_URL};
use radiumical_core::types::{ProviderKind, SessionConfig};
use std::path::PathBuf;
use std::sync::Arc;
use tui::{BackendCmd, UiEvent};

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

    // Ensure default agent and skill definitions exist
    radiumical_core::agent_pool::ensure_defaults();
    radiumical_core::skill::ensure_defaults();

    // Load secure env (device-bound API keys) and inject into process env.
    for (k, v) in radiumical_core::secure_env::load_env() {
        if std::env::var(&k).is_err() {
            std::env::set_var(&k, &v);
        }
    }

    let provider_kind = match cli.provider.to_lowercase().as_str() {
        "openai" => ProviderKind::OpenAI,
        "deepseek" => ProviderKind::OpenAI,
        "anthropic" => ProviderKind::Anthropic,
        "ollama" => ProviderKind::Ollama,
        _ => ProviderKind::OpenAI,
    };

    let model = cli
        .model
        .or_else(|| {
            let cfg = radiumical_core::config::Config::load().ok();
            cfg.and_then(|c| c.model)
        })
        .unwrap_or_else(|| {
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

    let file_cfg =
        radiumical_core::config::Config::load().unwrap_or(radiumical_core::config::Config {
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
        });

    let mut config = SessionConfig {
        provider: provider_kind,
        model,
        api_key: api_key.clone(),
        api_base: api_base.clone(),
        max_iterations: cli.max_iterations,
        system_prompt: radiumical_core::types::default_system_prompt(),
        llm_timeout_secs: cli.llm_timeout,
        tool_timeout_secs: cli.tool_timeout,
        heartbeat_interval_secs: cli.heartbeat,
        concurrency: cli.concurrency,
        use_markdown: false, // TUI handles rendering
        mode: radiumical_core::types::AgentMode::Auto,
        max_context_tokens: file_cfg.max_context_tokens.unwrap_or(1_000_000),
        context_compress_ratio: file_cfg.context_compress_ratio.unwrap_or(0.8),
        auto_continue: true,
    };

    let mut provider = create_provider(
        &config.provider,
        config.api_base.as_deref(),
        &config.api_key,
        &config.model,
    );

    // Set sub-agent defaults so SubAgentTool can spawn independently
    radiumical_core::subagent::set_defaults(config.clone(), Arc::clone(&provider));
    // Set cluster tool defaults
    radiumical_core::tools::cluster_tool::set_defaults(config.clone(), Arc::clone(&provider));

    // ── Channels for frontend ↔ backend communication ──
    let (ui_tx, ui_rx) = tokio::sync::mpsc::channel::<UiEvent>(256);
    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<BackendCmd>(8);

    // ── Non-interactive mode (--task) ──
    if let Some(task) = cli.task {
        let mut runner = PipelineRunner::new(config.clone(), provider);
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        runner
            .run(task, workspace, &[], None, ui_tx, cancel_rx)
            .await?;
        return Ok(());
    }

    // ── Interactive mode: spawn TUI on a separate thread ──
    let tui_config = config.clone();
    let tui_workspace = workspace.to_string_lossy().to_string();
    let tui_handle = std::thread::spawn(move || {
        if let Err(e) = tui::run(cmd_tx, ui_rx, tui_config, tui_workspace) {
            eprintln!("TUI error: {e}");
        }
    });

    // ── Backend loop (this tokio thread) ──
    let mut runner = Arc::new(tokio::sync::Mutex::new(PipelineRunner::new(
        config.clone(),
        Arc::clone(&provider),
    )));
    let cache_dir = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".radiumical_cache");
    let registry = match ProviderRegistry::new(cache_dir) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to initialize provider registry: {e}");
            std::process::exit(1);
        }
    };
    let cmd_pool = CommandPool::new();
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    // ── MCP servers ──
    let mcp_config = radiumical_core::mcp::load_config();
    let mcp_timeout = std::time::Duration::from_secs(config.tool_timeout_secs);
    // (client, cached_tools, enabled) — tools discovered at startup, reused per task.
    let mut mcp_clients: Vec<(
        Arc<radiumical_core::mcp::McpClient>,
        Vec<radiumical_core::mcp::McpToolInfo>,
        bool,
    )> = Vec::new();
    for (name, server_cfg) in &mcp_config.servers {
        match radiumical_core::mcp::McpClient::spawn(name, server_cfg, mcp_timeout).await {
            Ok(client) => match client.list_tools().await {
                Ok(tools) => {
                    eprintln!("MCP '{name}': {} tools loaded", tools.len());
                    let tool_count = tools.len();
                    mcp_clients.push((Arc::new(client), tools, true));
                    let _ = ui_tx
                        .send(UiEvent::McpStatus {
                            name: name.clone(),
                            alive: true,
                            tool_count,
                        })
                        .await;
                }
                Err(e) => {
                    eprintln!("MCP '{name}': tools/list failed: {e}");
                }
            },
            Err(e) => {
                eprintln!("MCP '{name}': spawn failed: {e}");
                let _ = ui_tx
                    .send(UiEvent::McpStatus {
                        name: name.clone(),
                        alive: false,
                        tool_count: 0,
                    })
                    .await;
            }
        }
    }
    // MCP tools will be added per-task in the harness run loop.

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(BackendCmd::Cancel) => {
                        let _ = cancel_tx.send(true);
                        let _ = ui_tx.send(UiEvent::ThinkingDone).await;
                    }
                    Some(BackendCmd::ChoiceResponse { id: _, value }) => {
                        if let Some(tx) = radiumical_core::tools::interact::take_choice_tx() {
                            let _ = tx.send(value);
                        }
                    }
                    Some(BackendCmd::RunTask(text)) => {
                        if text.starts_with("\x01subagent:") {
                            let rest = &text[11..];
                            if let Some((id, task)) = rest.split_once(':') {
                                let provider = Arc::clone(&provider);
                                let cfg = config.clone();
                                radiumical_core::subagent::spawn(
                                    id.to_string(),
                                    task.to_string(),
                                    None,
                                    cfg,
                                    provider,
                                    Some(ui_tx.clone()),
                                )
                                .await;
                                let _ = ui_tx.send(UiEvent::LlmChunk(format!(
                                    "Sub-agent '{id}' spawned.\n"
                                ))).await;
                                continue;
                            }
                        }
                        let _ = cancel_tx.send(false);
                        match cmd_pool.dispatch(&text, &mut config) {
                            CommandOutcome::Exit => break,
                            CommandOutcome::Continue => continue,
                            CommandOutcome::Agent(task) => {
                                let heartbeat_interval = config.heartbeat_interval_secs;
                                let hb_cancel = if heartbeat_interval > 0 {
                                    let (hb_tx, mut hb_rx) =
                                        tokio::sync::mpsc::channel(1);
                                    let ui_tx_hb = ui_tx.clone();
                                    tokio::spawn(async move {
                                        let mut interval = tokio::time::interval(
                                            std::time::Duration::from_secs(heartbeat_interval),
                                        );
                                        loop {
                                            tokio::select! {
                                                _ = interval.tick() => {
                                                    let _ = ui_tx_hb.send(UiEvent::ThinkingTick).await;
                                                }
                                                _ = hb_rx.recv() => break,
                                            }
                                        }
                                    });
                                    Some(hb_tx)
                                } else {
                                    None
                                };

                                let runner = Arc::clone(&runner);
                                let ui_tx = ui_tx.clone();
                                let cancel_rx = cancel_rx.clone();
                                let workspace = workspace.clone();
                                // Build MCP tool adapters from cached tool info (skip disabled).
                                let mcp_tools: Vec<Box<dyn radiumical_core::tools::Tool>> = mcp_clients
                                    .iter()
                                    .filter(|(_, _, enabled)| *enabled)
                                    .flat_map(|(client, tools, _)| {
                                        tools.iter().map(move |info| {
                                            Box::new(radiumical_core::tools::McpToolAdapter {
                                                info: info.clone(),
                                                client: Arc::clone(client),
                                            }) as Box<dyn radiumical_core::tools::Tool>
                                        })
                                    })
                                    .collect();
                                tokio::spawn(async move {
                                    let mut runner = runner.lock().await;
                                    if let Err(e) = runner
                                        .run(task, workspace, &mcp_tools, hb_cancel, ui_tx.clone(), cancel_rx)
                                        .await
                                    {
                                        let _ = ui_tx.send(UiEvent::Error(e.to_string())).await;
                                    }
                        let _ = ui_tx.send(UiEvent::ThinkingDone).await;
                                });
                            }
                        }
                    }
                    Some(BackendCmd::SetModel(model)) => {
                        config.model = model.clone();
                        provider = create_provider(
                            &config.provider,
                            config.api_base.as_deref(),
                            &config.api_key,
                            &config.model,
                        );
                        runner = Arc::new(tokio::sync::Mutex::new(PipelineRunner::new(
                            config.clone(),
                            Arc::clone(&provider),
                        )));
                        radiumical_core::subagent::set_defaults(
                            config.clone(),
                            Arc::clone(&provider),
                        );
                    }
                    Some(BackendCmd::SetMode(mode)) => {
                        config.mode = mode.clone();
                        runner.lock().await.set_mode(mode);
                    }
                    Some(BackendCmd::SetThinkingEffort(effort)) => {
                        provider.set_reasoning_effort(Some(effort));
                    }
                    Some(BackendCmd::ResetConversation) => {
                        runner.lock().await.reset_conversation();
                    }
                    Some(BackendCmd::LoadSession(items)) => {
                        runner.lock().await.load_session_items(&items);
                    }
                    Some(BackendCmd::ToggleMcpServer { name }) => {
                        for (_, tools, enabled) in &mut mcp_clients {
                            if tools.iter().any(|t| t.server_name == name) {
                                *enabled = !*enabled;
                                break;
                            }
                        }
                    }
                    Some(BackendCmd::RefreshModels) => {
                        let ui_tx = ui_tx.clone();
                        let cfg = config.clone();
                        tokio::spawn(async move {
                            let models = radiumical_core::providers::discover_models_for_config(&cfg).await;
                            let _ = ui_tx.send(UiEvent::ModelsLoaded(models)).await;
                        });
                    }
                    Some(BackendCmd::FetchProviders) => {
                        let registry = registry.clone();
                        let ui_tx = ui_tx.clone();
                        tokio::spawn(async move {
                            // Try online first, fall back to embedded list.
                            let (sources, from_online) = match registry
                                .fetch_or_cache(DEFAULT_REGISTRY_URL)
                                .await
                            {
                                Ok(s) => (s, true),
                                Err(_) => (registry.embedded_fallback(), false),
                            };
                            let _ = ui_tx.send(UiEvent::ProvidersLoaded(sources)).await;
                            if !from_online {
                                let _ = ui_tx.send(UiEvent::Toast {
                                    message: "Using bundled provider list (offline)".into(),
                                    level: "warn".into(),
                                    duration_secs: 5,
                                }).await;
                            }
                        });
                    }
                    Some(BackendCmd::FetchModels(source)) => {
                        let registry = registry.clone();
                        let ui_tx = ui_tx.clone();
                        tokio::spawn(async move {
                            let models = discover_models(
                                registry.client(),
                                &source,
                                source.api_key(),
                            )
                            .await;
                            let _ = ui_tx.send(UiEvent::ModelsLoaded(models)).await;
                        });
                    }
                    None => break,
                }
            }
        }
    }

    tui_handle.join().ok();
    Ok(())
}
