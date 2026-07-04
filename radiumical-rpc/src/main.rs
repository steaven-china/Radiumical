//! Radiumical RPC server — JSON protocol over stdin/stdout.
//!
//! Reads one JSON request per line from stdin, writes JSON responses and
//! streaming events to stdout. Logs go to stderr.
//!
//! Protocol:
//! - Request line:  `{ "id": 1, "method": "chat", "params": { "message": "..." } }`
//! - Response line: `{ "id": 1, "result": { "status": "started" } }`
//! - Event line:    `{ "event": "llm_chunk", "data": { "content": "..." } }`

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use radiumical_core::agent::Agent;
use radiumical_core::harness::Harness;
use radiumical_core::provider::create_provider;
use radiumical_core::types::{AgentMode, ProviderKind, SessionConfig, UiEvent};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};

/// Radiumical RPC server — JSON over stdio.
#[derive(Parser, Debug)]
#[command(name = "radiumical-rpc", version, about, long_about = None)]
struct Cli {
    /// Workspace directory
    #[arg(short = 'w', long, default_value = ".")]
    workspace: PathBuf,

    /// Provider
    #[arg(short = 'p', long, default_value = "deepseek")]
    provider: String,

    /// Model
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

    /// LLM request timeout in seconds
    #[arg(long, default_value = "120")]
    llm_timeout: u64,

    /// Tool execution timeout in seconds
    #[arg(long, default_value = "300")]
    tool_timeout: u64,
}

// ═══ Protocol types ═══

#[derive(Debug, Deserialize)]
struct Request {
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct Response {
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
struct RpcError {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize)]
struct Event {
    event: String,
    data: serde_json::Value,
}

// ═══ Server state ═══

struct ServerState {
    harness: Mutex<Harness>,
    config: Mutex<SessionConfig>,
    workspace: Mutex<PathBuf>,
    cancel_tx: Mutex<Option<tokio::sync::watch::Sender<bool>>>,
    running: AtomicBool,
    mcp_clients: Vec<McpClientEntry>,
}

struct McpClientEntry {
    client: Arc<radiumical_core::mcp::McpClient>,
    tools: Vec<radiumical_core::mcp::McpToolInfo>,
    enabled: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    radiumical_core::agent_pool::ensure_defaults();
    radiumical_core::skill::ensure_defaults();
    for (k, v) in radiumical_core::secure_env::load_env() {
        if std::env::var(&k).is_err() {
            std::env::set_var(&k, &v);
        }
    }

    let (config, provider, workspace) = build_initial_state(&cli).await?;
    let harness = Harness::new(config.clone(), Arc::clone(&provider));
    radiumical_core::subagent::set_defaults(config.clone(), Arc::clone(&provider));
    radiumical_core::tools::cluster_tool::set_defaults(config.clone(), Arc::clone(&provider));

    let mcp_clients = load_mcp_clients(config.tool_timeout_secs).await;

    let state = Arc::new(ServerState {
        harness: Mutex::new(harness),
        config: Mutex::new(config),
        workspace: Mutex::new(workspace),
        cancel_tx: Mutex::new(None),
        running: AtomicBool::new(false),
        mcp_clients,
    });

    let (out_tx, mut out_rx) = mpsc::channel::<String>(256);
    let stdout = Arc::new(Mutex::new(tokio::io::stdout()));
    let stdout_writer = Arc::clone(&stdout);

    tokio::spawn(async move {
        while let Some(line) = out_rx.recv().await {
            let mut stdout = stdout_writer.lock().await;
            if stdout.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            if stdout.write_all(b"\n").await.is_err() {
                break;
            }
            if stdout.flush().await.is_err() {
                break;
            }
        }
    });

    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                send_error(&out_tx, serde_json::Value::Null, -32700, e.to_string()).await;
                continue;
            }
        };
        let state = Arc::clone(&state);
        let out_tx = out_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = dispatch(req, state, out_tx).await {
                tracing::warn!("dispatch error: {e}");
            }
        });
    }

    Ok(())
}

async fn build_initial_state(cli: &Cli) -> Result<(SessionConfig, Arc<dyn radiumical_core::provider::Provider>, PathBuf)> {
    let provider_kind = parse_provider(&cli.provider);
    let model = cli.model.clone().unwrap_or_else(|| {
        if cli.provider.to_lowercase() == "deepseek" {
            "deepseek-v4-pro".into()
        } else {
            provider_kind.default_model().into()
        }
    });
    let api_key = cli.api_key.clone().unwrap_or_else(|| {
        std::env::var("RADIUMICAL_API_KEY")
            .or_else(|_| std::env::var("DEEPSEEK_API_KEY"))
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .unwrap_or_default()
    });
    if api_key.is_empty() {
        eprintln!("No API key. Set DEEPSEEK_API_KEY or use --api-key.");
        std::process::exit(1);
    }

    let api_base = cli.api_base.clone().or_else(|| {
        if cli.provider.to_lowercase() == "deepseek" {
            Some("https://api.deepseek.com/v1".into())
        } else {
            None
        }
    });

    let workspace = std::fs::canonicalize(&cli.workspace).unwrap_or_else(|_| cli.workspace.clone());

    let config = SessionConfig {
        provider: provider_kind,
        model,
        api_key,
        api_base,
        max_iterations: cli.max_iterations,
        system_prompt: radiumical_core::types::default_system_prompt(),
        llm_timeout_secs: cli.llm_timeout,
        tool_timeout_secs: cli.tool_timeout,
        heartbeat_interval_secs: 0,
        concurrency: 8,
        use_markdown: false,
        mode: AgentMode::Auto,
        max_context_tokens: 1_000_000,
        context_compress_ratio: 0.8,
        auto_continue: true,
        session_id: format!(
            "rpc-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ),
    };

    let provider = create_provider(&config.provider, config.api_base.as_deref(), &config.api_key, &config.model);
    Ok((config, provider, workspace))
}

fn parse_provider(s: &str) -> ProviderKind {
    match s.to_lowercase().as_str() {
        "anthropic" => ProviderKind::Anthropic,
        "ollama" => ProviderKind::Ollama,
        _ => ProviderKind::OpenAI,
    }
}

async fn load_mcp_clients(tool_timeout_secs: u64) -> Vec<McpClientEntry> {
    let mcp_config = radiumical_core::mcp::load_config();
    let mcp_timeout = std::time::Duration::from_secs(tool_timeout_secs);
    let mut clients = Vec::new();
    for (name, server_cfg) in &mcp_config.servers {
        match radiumical_core::mcp::McpClient::spawn(name, server_cfg, mcp_timeout).await {
            Ok(client) => match client.list_tools().await {
                Ok(tools) => {
                    eprintln!("MCP '{name}': {} tools loaded", tools.len());
                    clients.push(McpClientEntry {
                        client: Arc::new(client),
                        tools,
                        enabled: true,
                    });
                }
                Err(e) => eprintln!("MCP '{name}': tools/list failed: {e}"),
            },
            Err(e) => eprintln!("MCP '{name}': spawn failed: {e}"),
        }
    }
    clients
}

async fn dispatch(req: Request, state: Arc<ServerState>, out_tx: mpsc::Sender<String>) -> Result<()> {
    match req.method.as_str() {
        "initialize" => handle_initialize(req, state, out_tx).await,
        "chat" => handle_chat(req, state, out_tx).await,
        "cancel" => handle_cancel(req, state, out_tx).await,
        "reset" => handle_reset(req, state, out_tx).await,
        "set_model" => handle_set_model(req, state, out_tx).await,
        "set_mode" => handle_set_mode(req, state, out_tx).await,
        "choice_response" => handle_choice_response(req, state, out_tx).await,
        _ => {
            send_error(&out_tx, req.id, -32601, format!("method not found: {}", req.method)).await;
            Ok(())
        }
    }
}

async fn handle_initialize(
    req: Request,
    state: Arc<ServerState>,
    out_tx: mpsc::Sender<String>,
) -> Result<()> {
    if state.running.load(Ordering::SeqCst) {
        send_error(&out_tx, req.id, -32000, "cannot initialize while chat is running".into()).await;
        return Ok(());
    }

    let mut config = state.config.lock().await;
    let params = &req.params;

    if let Some(provider) = params.get("provider").and_then(|v| v.as_str()) {
        config.provider = parse_provider(provider);
    }
    if let Some(model) = params.get("model").and_then(|v| v.as_str()) {
        config.model = model.into();
    }
    if let Some(api_key) = params.get("api_key").and_then(|v| v.as_str()) {
        config.api_key = api_key.into();
    } else if let Some(api_key) = params.get("apiKey").and_then(|v| v.as_str()) {
        config.api_key = api_key.into();
    }
    if let Some(api_base) = params.get("api_base").and_then(|v| v.as_str()) {
        config.api_base = Some(api_base.into());
    } else if let Some(api_base) = params.get("apiBase").and_then(|v| v.as_str()) {
        config.api_base = Some(api_base.into());
    }
    if let Some(mode) = params.get("mode").and_then(|v| v.as_str()) {
        config.mode = parse_mode(mode);
    }
    if let Some(workspace) = params.get("workspace").and_then(|v| v.as_str()) {
        *state.workspace.lock().await = std::fs::canonicalize(workspace).unwrap_or_else(|_| PathBuf::from(workspace));
    }

    let provider = create_provider(&config.provider, config.api_base.as_deref(), &config.api_key, &config.model);
    radiumical_core::subagent::set_defaults(config.clone(), Arc::clone(&provider));
    radiumical_core::tools::cluster_tool::set_defaults(config.clone(), Arc::clone(&provider));

    let mut harness = state.harness.lock().await;
    harness.set_provider(provider);
    harness.set_model(config.model.clone());
    harness.set_mode(config.mode.clone());

    send_result(&out_tx, req.id, serde_json::json!({"status": "ok"})).await;
    Ok(())
}

async fn handle_chat(req: Request, state: Arc<ServerState>, out_tx: mpsc::Sender<String>) -> Result<()> {
    if state
        .running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        send_error(&out_tx, req.id, -32000, "chat already running".into()).await;
        return Ok(());
    }

    let message = req
        .params
        .get("message")
        .and_then(|v| v.as_str())
        .context("chat requires params.message")?
        .to_string();

    let (ui_tx, mut ui_rx) = mpsc::channel::<UiEvent>(256);
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    *state.cancel_tx.lock().await = Some(cancel_tx);

    let workspace = state.workspace.lock().await.clone();
    let state_run = Arc::clone(&state);
    let mut run_handle = tokio::spawn(async move {
        let mut harness = state_run.harness.lock().await;
        let agent = Agent::default_coder();
        let mcp_tools: Vec<Box<dyn radiumical_core::tools::Tool>> = state_run
            .mcp_clients
            .iter()
            .filter(|e| e.enabled)
            .flat_map(|e| {
                e.tools.iter().map(move |info| {
                    Box::new(radiumical_core::tools::McpToolAdapter {
                        info: info.clone(),
                        client: Arc::clone(&e.client),
                    }) as Box<dyn radiumical_core::tools::Tool>
                })
            })
            .collect();
        harness
            .run(message, workspace, &agent, &mcp_tools, None, ui_tx, cancel_rx)
            .await
    });

    send_result(&out_tx, req.id, serde_json::json!({"status": "started"})).await;

    let mut result = Ok(());
    loop {
        tokio::select! {
            event = ui_rx.recv() => match event {
                Some(ev) => {
                    if let Some(event) = ui_event_to_rpc(ev) {
                        send_event(&out_tx, event).await;
                    }
                }
                None => break,
            },
            res = &mut run_handle => {
                result = res.unwrap_or_else(|e| Err(anyhow::anyhow!("harness panicked: {e}")));
                break;
            }
        }
    }

    // Drain any remaining UI events after the harness finishes.
    while let Ok(ev) = ui_rx.try_recv() {
        if let Some(event) = ui_event_to_rpc(ev) {
            send_event(&out_tx, event).await;
        }
    }

    if let Err(ref e) = result {
        send_event(
            &out_tx,
            Event {
                event: "error".into(),
                data: serde_json::json!({"message": e.to_string()}),
            },
        )
        .await;
    }

    send_event(
        &out_tx,
        Event {
            event: "done".into(),
            data: serde_json::json!({}),
        },
    )
    .await;

    *state.cancel_tx.lock().await = None;
    state.running.store(false, Ordering::SeqCst);
    result
}

async fn handle_cancel(req: Request, state: Arc<ServerState>, out_tx: mpsc::Sender<String>) -> Result<()> {
    if let Some(tx) = state.cancel_tx.lock().await.take() {
        let _ = tx.send(true);
        send_result(&out_tx, req.id, serde_json::json!({"status": "cancelled"})).await;
    } else {
        send_result(&out_tx, req.id, serde_json::json!({"status": "no running chat"})).await;
    }
    Ok(())
}

async fn handle_reset(req: Request, state: Arc<ServerState>, out_tx: mpsc::Sender<String>) -> Result<()> {
    if state.running.load(Ordering::SeqCst) {
        send_error(&out_tx, req.id, -32000, "cannot reset while chat is running".into()).await;
        return Ok(());
    }
    let mut harness = state.harness.lock().await;
    harness.reset_conversation();
    send_result(&out_tx, req.id, serde_json::json!({"status": "ok"})).await;
    Ok(())
}

async fn handle_set_model(
    req: Request,
    state: Arc<ServerState>,
    out_tx: mpsc::Sender<String>,
) -> Result<()> {
    let model = req
        .params
        .get("model")
        .and_then(|v| v.as_str())
        .context("set_model requires params.model")?
        .to_string();
    {
        let mut config = state.config.lock().await;
        config.model = model.clone();
    }
    let mut harness = state.harness.lock().await;
    harness.set_model(model);
    send_result(&out_tx, req.id, serde_json::json!({"status": "ok"})).await;
    Ok(())
}

async fn handle_set_mode(
    req: Request,
    state: Arc<ServerState>,
    out_tx: mpsc::Sender<String>,
) -> Result<()> {
    let mode_str = req
        .params
        .get("mode")
        .and_then(|v| v.as_str())
        .context("set_mode requires params.mode")?
        .to_string();
    let mode = parse_mode(&mode_str);
    {
        let mut config = state.config.lock().await;
        config.mode = mode.clone();
    }
    let mut harness = state.harness.lock().await;
    harness.set_mode(mode);
    send_result(&out_tx, req.id, serde_json::json!({"status": "ok"})).await;
    Ok(())
}

async fn handle_choice_response(
    req: Request,
    _state: Arc<ServerState>,
    out_tx: mpsc::Sender<String>,
) -> Result<()> {
    let id = req
        .params
        .get("id")
        .and_then(|v| v.as_str())
        .context("choice_response requires params.id")?
        .to_string();
    let value = req
        .params
        .get("value")
        .and_then(|v| v.as_str())
        .context("choice_response requires params.value")?
        .to_string();

    if let Some(tx) = radiumical_core::tools::interact::take_choice_tx() {
        let _ = tx.send(value);
        send_result(&out_tx, req.id, serde_json::json!({"status": "ok", "id": id})).await;
    } else {
        send_error(&out_tx, req.id, -32000, "no pending choice".into()).await;
    }
    Ok(())
}

fn parse_mode(s: &str) -> AgentMode {
    match s.to_lowercase().as_str() {
        "plan" => AgentMode::Plan,
        "exec" => AgentMode::Exec,
        _ => AgentMode::Auto,
    }
}

fn ui_event_to_rpc(ev: UiEvent) -> Option<Event> {
    match ev {
        UiEvent::LlmChunk(content) => Some(Event {
            event: "llm_chunk".into(),
            data: serde_json::json!({"content": content}),
        }),
        UiEvent::LlmReasoning(content) => Some(Event {
            event: "llm_reasoning".into(),
            data: serde_json::json!({"content": content}),
        }),
        UiEvent::ThinkingTick => Some(Event {
            event: "thinking_tick".into(),
            data: serde_json::json!({}),
        }),
        UiEvent::LlmDone => Some(Event {
            event: "llm_done".into(),
            data: serde_json::json!({}),
        }),
        UiEvent::ToolStart { name, index, total, args } => Some(Event {
            event: "tool_start".into(),
            data: serde_json::json!({"name": name, "index": index, "total": total, "args": args}),
        }),
        UiEvent::ToolDone => Some(Event {
            event: "tool_done".into(),
            data: serde_json::json!({}),
        }),
        UiEvent::ToolResult { content } => Some(Event {
            event: "tool_result".into(),
            data: serde_json::json!({"content": content}),
        }),
        UiEvent::Choice { id, mode, options } => Some(Event {
            event: "choice".into(),
            data: serde_json::json!({"id": id, "mode": mode, "options": options}),
        }),
        UiEvent::Error(message) => Some(Event {
            event: "error".into(),
            data: serde_json::json!({"message": message}),
        }),
        UiEvent::ThinkingDone => Some(Event {
            event: "done".into(),
            data: serde_json::json!({}),
        }),
        UiEvent::TitleGenerated(title) => Some(Event {
            event: "title_generated".into(),
            data: serde_json::json!({"title": title}),
        }),
        UiEvent::SubAgentDone { id, success } => Some(Event {
            event: "subagent_done".into(),
            data: serde_json::json!({"id": id, "success": success}),
        }),
        UiEvent::McpStatus { name, alive, tool_count } => Some(Event {
            event: "mcp_status".into(),
            data: serde_json::json!({"name": name, "alive": alive, "tool_count": tool_count}),
        }),
        UiEvent::PlanUpdated { title, tasks } => {
            let ts: Vec<serde_json::Value> = tasks
                .iter()
                .map(|t| serde_json::json!({"id": t.id, "title": t.title, "status": t.status.label()}))
                .collect();
            Some(Event {
                event: "plan_updated".into(),
                data: serde_json::json!({"title": title, "tasks": ts}),
            })
        }
        _ => None,
    }
}

async fn send_result(out_tx: &mpsc::Sender<String>, id: serde_json::Value, result: serde_json::Value) {
    let resp = Response {
        id,
        result: Some(result),
        error: None,
    };
    let _ = out_tx.send(serde_json::to_string(&resp).unwrap_or_default()).await;
}

async fn send_error(out_tx: &mpsc::Sender<String>, id: serde_json::Value, code: i32, message: String) {
    let resp = Response {
        id,
        result: None,
        error: Some(RpcError { code, message }),
    };
    let _ = out_tx.send(serde_json::to_string(&resp).unwrap_or_default()).await;
}

async fn send_event(out_tx: &mpsc::Sender<String>, event: Event) {
    let _ = out_tx.send(serde_json::to_string(&event).unwrap_or_default()).await;
}
