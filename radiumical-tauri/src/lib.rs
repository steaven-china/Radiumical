use radiumical_core::config::Config;
use radiumical_core::pipeline::PipelineRunner;
use radiumical_core::provider::create_provider;
use radiumical_core::providers::{
    discover_models_for_config, fetch_provider_sources, find_provider, ProviderSource,
};
use radiumical_core::session::{
    SessionItem, SessionMeta, SessionMode, SessionPool, WorkspaceRegistry,
};
use radiumical_core::types::{AgentMode, ProviderKind, SessionConfig, UiEvent};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{watch, Mutex as TokioMutex};

fn resolve_key_from_registry(provider_name: &str) -> String {
    // Try the provider registry (cache -> embedded) first.
    if let Some(source) = find_provider(provider_name) {
        if let Some(key) = source.api_key() {
            return key;
        }
    }
    // Fall back to the global config file.
    Config::load()
        .ok()
        .and_then(|c| c.api_key)
        .filter(|k| !k.is_empty())
        .unwrap_or_default()
}

/// Resolve provider kind and fill in missing api_base / api_key / model from the registry.
fn resolve_provider_inputs(
    provider_name: &str,
    api_base: String,
    api_key: String,
    model: String,
    api_type: Option<String>,
) -> (ProviderKind, String, String, String) {
    let kind = if let Some(api_type) = api_type {
        ProviderKind::Custom(provider_name.to_lowercase(), api_type)
    } else {
        radiumical_core::providers::parse_provider_kind(provider_name)
    };

    let source = find_provider(provider_name);

    let api_base = if api_base.trim().is_empty() {
        source
            .as_ref()
            .map(|s| s.api_base.clone())
            .unwrap_or_default()
    } else {
        api_base
    };

    let api_key = if !api_key.trim().is_empty() {
        api_key.trim().to_string()
    } else {
        resolve_key_from_registry(provider_name)
    };

    let model = if !model.trim().is_empty() {
        model.trim().to_string()
    } else {
        source
            .as_ref()
            .and_then(|s| s.default_model.clone())
            .unwrap_or_default()
    };

    (kind, api_base, api_key, model)
}

// ── Display Item — single source of truth for UI ──

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum DisplayItem {
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "reasoning")]
    Reasoning { content: String, streaming: bool },
    #[serde(rename = "assistant")]
    Assistant { content: String, streaming: bool },
    #[serde(rename = "tool")]
    Tool {
        name: String,
        args: String,
        result: Option<String>,
        running: bool,
    },
    #[serde(rename = "error")]
    Error { content: String },
    #[serde(rename = "thinking")]
    Thinking,
}

// ── App State ──

struct AppState {
    runner: TokioMutex<PipelineRunner>,
    config: TokioMutex<SessionConfig>,
    cancel_tx: TokioMutex<Option<watch::Sender<bool>>>,
    workspace: PathBuf,
    session_items: Arc<TokioMutex<Vec<SessionItem>>>,
    display_items: Arc<TokioMutex<Vec<DisplayItem>>>,
}

#[derive(Serialize)]
struct AppInfo {
    model: String,
    provider: String,
    api_type: String,
    mode: String,
    workspace: String,
    api_base: String,
    api_key_source: String,
    max_context_tokens: usize,
    llm_timeout_secs: u64,
    tool_timeout_secs: u64,
}

// ── Tauri Commands ──

#[tauri::command]
async fn get_display(state: tauri::State<'_, AppState>) -> Result<Vec<DisplayItem>, String> {
    let items = state.display_items.lock().await;
    Ok(items.clone())
}

#[tauri::command]
fn get_version() -> String {
    radiumical_core::version::version_string()
}

#[tauri::command]
async fn run_task(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task: String,
) -> Result<(), String> {
    // Push user message to both session and display
    {
        let mut items = state.session_items.lock().await;
        items.push(SessionItem::User {
            content: task.clone(),
        });
    }
    {
        let mut display = state.display_items.lock().await;
        display.push(DisplayItem::User {
            content: task.clone(),
        });
    }
    let _ = app.emit("display-sync", state.display_items.lock().await.clone());

    let (cancel_tx, cancel_rx) = watch::channel(false);
    {
        let mut tx = state.cancel_tx.lock().await;
        *tx = Some(cancel_tx);
    }

    let (ui_tx, mut ui_rx) = tokio::sync::mpsc::channel::<UiEvent>(256);

    let items_handle = Arc::clone(&state.session_items);
    let display_handle = Arc::clone(&state.display_items);
    let handle = app.clone();

    let event_task = tokio::spawn(async move {
        let mut assistant_content = String::new();
        let mut assistant_reasoning = String::new();
        let mut pending_tool_name = String::new();
        let mut pending_tool_args = String::new();

        while let Some(event) = ui_rx.recv().await {
            match event {
                UiEvent::LlmChunk(chunk) => {
                    assistant_content.push_str(&chunk);
                    let mut d = display_handle.lock().await;
                    // Update last streaming assistant, or create new one
                    match d.last_mut() {
                        Some(DisplayItem::Assistant { content, streaming }) if *streaming => {
                            content.push_str(&chunk);
                        }
                        _ => {
                            d.push(DisplayItem::Assistant {
                                content: chunk,
                                streaming: true,
                            });
                        }
                    }
                    let _ = handle.emit("display-sync", d.clone());
                }
                UiEvent::LlmReasoning(reasoning) => {
                    assistant_reasoning.push_str(&reasoning);
                    let mut d = display_handle.lock().await;
                    // Update last streaming reasoning, or create new one
                    match d.last_mut() {
                        Some(DisplayItem::Reasoning { content, streaming }) if *streaming => {
                            content.push_str(&reasoning);
                        }
                        _ => {
                            d.push(DisplayItem::Reasoning {
                                content: reasoning,
                                streaming: true,
                            });
                        }
                    }
                    let _ = handle.emit("display-sync", d.clone());
                }
                UiEvent::ThinkingTick => {
                    let mut d = display_handle.lock().await;
                    if !matches!(d.last(), Some(DisplayItem::Thinking)) {
                        d.push(DisplayItem::Thinking);
                    }
                    let _ = handle.emit("display-sync", d.clone());
                }
                UiEvent::ThinkingDone => {
                    let mut d = display_handle.lock().await;
                    if matches!(d.last(), Some(DisplayItem::Thinking)) {
                        d.pop();
                    }
                    let _ = handle.emit("display-sync", d.clone());
                }
                UiEvent::LlmDone => {
                    // Finalize any streaming items
                    let mut d = display_handle.lock().await;
                    for item in d.iter_mut().rev() {
                        match item {
                            DisplayItem::Assistant { streaming, .. } if *streaming => {
                                *streaming = false;
                            }
                            DisplayItem::Reasoning { streaming, .. } if *streaming => {
                                *streaming = false;
                            }
                            _ => {}
                        }
                    }
                    // Remove thinking indicator
                    if matches!(d.last(), Some(DisplayItem::Thinking)) {
                        d.pop();
                    }
                    drop(d);

                    // Flush to session_items
                    {
                        let mut items = items_handle.lock().await;
                        if !assistant_reasoning.is_empty() {
                            items.push(SessionItem::Reasoning {
                                content: assistant_reasoning.clone(),
                            });
                        }
                        if !assistant_content.is_empty() {
                            items.push(SessionItem::Assistant {
                                content: assistant_content.clone(),
                            });
                        }
                    }
                    assistant_content.clear();
                    assistant_reasoning.clear();
                    let _ = handle.emit("display-sync", display_handle.lock().await.clone());
                }
                UiEvent::ToolStart { name, args, .. } => {
                    pending_tool_name = name.clone();
                    pending_tool_args = args.clone();
                    let mut d = display_handle.lock().await;
                    d.push(DisplayItem::Tool {
                        name,
                        args,
                        result: None,
                        running: true,
                    });
                    let _ = handle.emit("display-sync", d.clone());
                }
                UiEvent::ToolDone => {
                    let _ = handle.emit("tool-done", ());
                }
                UiEvent::ToolResult { content } => {
                    // Update display tool item
                    let mut d = display_handle.lock().await;
                    for item in d.iter_mut().rev() {
                        if let DisplayItem::Tool {
                            running, result, ..
                        } = item
                        {
                            if *running {
                                *running = false;
                                *result = Some(content.clone());
                                break;
                            }
                        }
                    }
                    drop(d);

                    // Flush to session_items
                    {
                        let mut items = items_handle.lock().await;
                        let id = format!("call_{}", items.len());
                        items.push(SessionItem::Tool {
                            id,
                            name: pending_tool_name.clone(),
                            args: pending_tool_args.clone(),
                            result: Some(content.clone()),
                        });
                    }
                    let _ = handle.emit("display-sync", display_handle.lock().await.clone());
                }
                UiEvent::Error(e) => {
                    let mut d = display_handle.lock().await;
                    d.push(DisplayItem::Error { content: e.clone() });
                    drop(d);
                    {
                        let mut items = items_handle.lock().await;
                        items.push(SessionItem::Raw {
                            lines: vec![e.clone()],
                        });
                    }
                    let _ = handle.emit("display-sync", display_handle.lock().await.clone());
                }
                UiEvent::Toast {
                    message,
                    level,
                    duration_secs,
                } => {
                    let _ = handle.emit(
                        "toast",
                        serde_json::json!({
                            "message": message, "level": level, "durationSecs": duration_secs
                        }),
                    );
                }
                UiEvent::Choice { id, mode, options } => {
                    let _ = handle.emit(
                        "choice",
                        serde_json::json!({
                            "id": id, "mode": mode, "options": options
                        }),
                    );
                }
                UiEvent::ProvidersLoaded(sources) => {
                    let _ = handle.emit(
                        "providers-loaded",
                        serde_json::json!({ "sources": sources }),
                    );
                }
                UiEvent::ModelsLoaded(models) => {
                    let _ = handle.emit("models-loaded", serde_json::json!({ "models": models }));
                }
                UiEvent::TitleGenerated(title) => {
                    let _ = handle.emit("session-title", title);
                }
                UiEvent::SubAgentDone { id, success } => {
                    let _ = handle.emit(
                        "subagent-done",
                        serde_json::json!({ "id": id, "success": success }),
                    );
                }
                UiEvent::McpStatus {
                    name,
                    alive,
                    tool_count,
                } => {
                    let _ = handle.emit(
                        "mcp-status",
                        serde_json::json!({
                            "name": name, "alive": alive, "toolCount": tool_count
                        }),
                    );
                }
                UiEvent::PlanUpdated { title, tasks } => {
                    let task_list: Vec<serde_json::Value> = tasks.iter().map(|t| {
                        serde_json::json!({ "id": t.id, "title": t.title, "status": format!("{:?}", t.status) })
                    }).collect();
                    let _ = handle.emit(
                        "plan-updated",
                        serde_json::json!({ "title": title, "tasks": task_list }),
                    );
                }
                UiEvent::CheckpointCreated(cp) => {
                    let _ = handle.emit("checkpoint-created", serde_json::json!(cp));
                }
            }
        }
    });

    let mut runner = state.runner.lock().await;
    let workspace = state.workspace.clone();
    let task_desc = task.clone();

    // Validate that we have a usable provider configuration before running.
    {
        let cfg = state.config.lock().await;
        if cfg.api_base.as_deref().unwrap_or("").is_empty() {
            drop(cfg);
            drop(runner);
            return Err("API base is not configured. Check provider settings.".into());
        }
    }

    let result = runner
        .run(task, workspace, &[], None, ui_tx, cancel_rx)
        .await;
    drop(runner);

    event_task.abort();

    {
        let mut tx = state.cancel_tx.lock().await;
        *tx = None;
    }

    // Auto-save
    {
        let items = state.session_items.lock().await;
        if !items.is_empty() {
            let workspace = state.workspace.to_string_lossy().to_string();
            let pool = SessionPool::for_workspace(&workspace);
            let config = state.config.lock().await;
            let mode = SessionMode::from(config.mode.clone());
            let _ = pool.save(
                "autosave",
                &items,
                &config.model,
                config.provider.name(),
                mode,
                "",
                Some(&task_desc),
            );
        }
    }

    result.map_err(|e| e.to_string())
}

#[tauri::command]
async fn cancel_task(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let tx = state.cancel_tx.lock().await;
    if let Some(ref sender) = *tx {
        let _ = sender.send(true);
        Ok(())
    } else {
        Err("No task running".into())
    }
}

#[tauri::command]
async fn is_running(state: tauri::State<'_, AppState>) -> Result<bool, String> {
    let tx = state.cancel_tx.lock().await;
    Ok(tx.is_some())
}

#[tauri::command]
async fn new_session(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    {
        let items = state.session_items.lock().await;
        if !items.is_empty() {
            let workspace = state.workspace.to_string_lossy().to_string();
            let pool = SessionPool::for_workspace(&workspace);
            let config = state.config.lock().await;
            let mode = SessionMode::from(config.mode.clone());
            let _ = pool.save(
                "autosave",
                &items,
                &config.model,
                config.provider.name(),
                mode,
                "",
                None,
            );
        }
    }
    let mut runner = state.runner.lock().await;
    runner.reset_conversation();
    state.session_items.lock().await.clear();
    state.display_items.lock().await.clear();
    let _ = app.emit("display-sync", Vec::<DisplayItem>::new());
    Ok(())
}

#[tauri::command]
async fn list_sessions(state: tauri::State<'_, AppState>) -> Result<Vec<SessionMeta>, String> {
    let workspace = state.workspace.to_string_lossy().to_string();
    let pool = SessionPool::for_workspace(&workspace);
    pool.list().map_err(|e| e.to_string())
}

#[tauri::command]
async fn save_session(
    state: tauri::State<'_, AppState>,
    name: String,
    description: Option<String>,
) -> Result<(), String> {
    let config = state.config.lock().await;
    let items = state.session_items.lock().await;
    let workspace = state.workspace.to_string_lossy().to_string();
    let pool = SessionPool::for_workspace(&workspace);
    let mode = SessionMode::from(config.mode.clone());
    pool.save(
        &name,
        &items,
        &config.model,
        &format!("{:?}", config.provider).to_lowercase(),
        mode,
        "",
        description.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn load_session(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    name: String,
) -> Result<(), String> {
    let workspace = state.workspace.to_string_lossy().to_string();
    let pool = SessionPool::for_workspace(&workspace);
    let (meta, items) = pool
        .load(&name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Session '{}' not found", name))?;

    let mut runner = state.runner.lock().await;
    runner.load_session_items(&items);
    drop(runner);

    *state.session_items.lock().await = items.clone();

    // Rebuild display from session items
    let display = session_items_to_display(&items);
    *state.display_items.lock().await = display.clone();
    let _ = app.emit("display-sync", display);
    let _ = app.emit("session-loaded", serde_json::json!({ "name": meta.name }));
    Ok(())
}

#[tauri::command]
async fn delete_session(state: tauri::State<'_, AppState>, name: String) -> Result<bool, String> {
    let workspace = state.workspace.to_string_lossy().to_string();
    let pool = SessionPool::for_workspace(&workspace);
    pool.delete(&name).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_app_info(state: tauri::State<'_, AppState>) -> Result<AppInfo, String> {
    let config = state.config.lock().await;
    let api_key_source = if !config.api_key.is_empty() {
        "config".to_string()
    } else {
        let env_name = match config.provider {
            ProviderKind::Anthropic => "ANTHROPIC_API_KEY",
            ProviderKind::Ollama => "OLLAMA_API_KEY",
            _ => "OPENAI_API_KEY",
        };
        if std::env::var(env_name)
            .ok()
            .filter(|k| !k.is_empty())
            .is_some()
        {
            format!("env:{env_name}")
        } else {
            let fallback = "DEEPSEEK_API_KEY";
            if std::env::var(fallback)
                .ok()
                .filter(|k| !k.is_empty())
                .is_some()
            {
                format!("env:{fallback}")
            } else {
                "none".to_string()
            }
        }
    };
    Ok(AppInfo {
        model: config.model.clone(),
        provider: config.provider.name().to_string(),
        api_type: config.provider.api_type().to_string(),
        mode: format!("{:?}", config.mode).to_lowercase(),
        workspace: state.workspace.to_string_lossy().to_string(),
        api_base: config.api_base.clone().unwrap_or_default(),
        api_key_source,
        max_context_tokens: config.max_context_tokens,
        llm_timeout_secs: config.llm_timeout_secs,
        tool_timeout_secs: config.tool_timeout_secs,
    })
}

#[tauri::command]
async fn save_api_key(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    api_key: String,
) -> Result<(), String> {
    let api_key = api_key.trim().to_string();
    let mut cfg = Config::load().map_err(|e| e.to_string())?;
    cfg.api_key = Some(api_key.clone());
    cfg.save().map_err(|e| e.to_string())?;

    let mut config = state.config.lock().await;
    config.api_key = api_key.clone();
    let provider = create_provider(
        &config.provider,
        config.api_base.as_deref(),
        &api_key,
        &config.model,
    );
    let new_runner = PipelineRunner::new(config.clone(), provider);
    drop(config);

    *state.runner.lock().await = new_runner;
    let _ = app.emit(
        "provider-changed",
        serde_json::json!({ "api_key_source": "config" }),
    );
    Ok(())
}

#[tauri::command]
async fn set_model(state: tauri::State<'_, AppState>, model: String) -> Result<(), String> {
    state.config.lock().await.model = model.clone();
    state.runner.lock().await.set_model(model);
    Ok(())
}

#[tauri::command]
async fn set_mode(state: tauri::State<'_, AppState>, mode: String) -> Result<(), String> {
    let agent_mode = match mode.to_lowercase().as_str() {
        "plan" => AgentMode::Plan,
        "exec" => AgentMode::Exec,
        _ => AgentMode::Auto,
    };
    state.config.lock().await.mode = agent_mode.clone();
    state.runner.lock().await.set_mode(agent_mode);
    Ok(())
}

#[tauri::command]
async fn fetch_providers() -> Result<Vec<ProviderSource>, String> {
    Ok(fetch_provider_sources().await)
}

#[tauri::command]
async fn set_provider(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    provider_name: String,
    api_base: String,
    api_key: String,
    model: String,
    api_type: Option<String>,
) -> Result<AppInfo, String> {
    let (kind, api_base, resolved_key, model) =
        resolve_provider_inputs(&provider_name, api_base, api_key, model, api_type);

    if api_base.trim().is_empty() {
        return Err(format!(
            "API base is empty for provider '{}'",
            provider_name
        ));
    }

    let provider = create_provider(&kind, Some(&api_base), &resolved_key, &model);
    let mut config = state.config.lock().await;
    config.provider = kind;
    config.api_key = resolved_key;
    config.api_base = Some(api_base);
    config.model = model.clone();
    let cfg_clone = config.clone();
    drop(config);

    let mut runner = state.runner.lock().await;
    runner.set_model(model.clone());
    *runner = PipelineRunner::new(cfg_clone, provider);
    drop(runner);

    let info = get_app_info(state).await?;
    let _ = app.emit("provider-changed", serde_json::json!({ "model": model }));
    Ok(info)
}

#[tauri::command]
async fn fetch_models_for_provider(
    provider_name: String,
    api_base: String,
    api_key: String,
    api_type: Option<String>,
) -> Result<Vec<String>, String> {
    let (kind, api_base, resolved_key, _model) =
        resolve_provider_inputs(&provider_name, api_base, api_key, String::new(), api_type);

    if api_base.trim().is_empty() {
        return Err(format!(
            "API base is empty for provider '{}'",
            provider_name
        ));
    }

    let config = SessionConfig {
        provider: kind,
        api_key: resolved_key,
        api_base: if api_base.is_empty() {
            None
        } else {
            Some(api_base)
        },
        ..Default::default()
    };
    Ok(discover_models_for_config(&config).await)
}

#[tauri::command]
async fn get_config() -> Result<serde_json::Value, String> {
    let mut value = serde_json::to_value(&Config::load().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    // Never expose the raw API key to the frontend.
    if let Some(obj) = value.as_object_mut() {
        obj.insert("api_key".into(), serde_json::Value::String("***".into()));
    }
    Ok(value)
}

#[tauri::command]
async fn save_config(config_json: serde_json::Value) -> Result<(), String> {
    let mut config: Config =
        serde_json::from_value(config_json).map_err(|e| format!("Invalid config: {}", e))?;
    // Preserve the existing API key; use `save_api_key` to change it.
    config.api_key = Config::load().ok().and_then(|c| c.api_key);
    config.save().map_err(|e| e.to_string())
}

#[tauri::command]
async fn reload_config(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<AppInfo, String> {
    let workspace_str = state.workspace.to_string_lossy().to_string();
    let ws_hash = radiumical_core::session::workspace_hash(&workspace_str);
    let session_config = Config::load_for_workspace(&ws_hash);

    let provider = create_provider(
        &session_config.provider,
        session_config.api_base.as_deref(),
        &session_config.api_key,
        &session_config.model,
    );

    {
        let mut cfg = state.config.lock().await;
        *cfg = session_config.clone();
    }
    {
        let mut runner = state.runner.lock().await;
        *runner = PipelineRunner::new(session_config.clone(), provider);
    }

    let info = get_app_info(state).await?;
    let _ = app.emit(
        "provider-changed",
        serde_json::json!({ "model": info.model }),
    );
    Ok(info)
}

#[tauri::command]
async fn get_workspaces() -> Result<WorkspaceRegistry, String> {
    let mut registry = WorkspaceRegistry::load();
    registry.discover();
    Ok(registry)
}

#[tauri::command]
async fn choice_response(_id: String, value: String) -> Result<(), String> {
    if let Some(tx) = radiumical_core::tools::interact::take_choice_tx() {
        let _ = tx.send(value);
        Ok(())
    } else {
        Err("No pending choice".into())
    }
}

// ── Helpers ──

fn session_items_to_display(items: &[SessionItem]) -> Vec<DisplayItem> {
    let mut out = Vec::new();
    for item in items {
        match item {
            SessionItem::User { content } => {
                out.push(DisplayItem::User {
                    content: content.clone(),
                });
            }
            SessionItem::Assistant { content } => {
                out.push(DisplayItem::Assistant {
                    content: content.clone(),
                    streaming: false,
                });
            }
            SessionItem::Reasoning { content } => {
                out.push(DisplayItem::Reasoning {
                    content: content.clone(),
                    streaming: false,
                });
            }
            SessionItem::Tool {
                name, args, result, ..
            } => {
                out.push(DisplayItem::Tool {
                    name: name.clone(),
                    args: args.clone(),
                    result: result.clone(),
                    running: false,
                });
            }
            SessionItem::Raw { lines } => {
                if !lines.is_empty() {
                    out.push(DisplayItem::Error {
                        content: lines.join("\n"),
                    });
                }
            }
            SessionItem::Meta { .. } => {}
        }
    }
    out
}

fn detect_workspace() -> PathBuf {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| {
            if cwd.join("Cargo.toml").exists() {
                return Some(cwd);
            }
            cwd.parent()
                .filter(|p| p.join("Cargo.toml").exists())
                .map(|p| p.to_path_buf())
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

// ── App Entry ──

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let workspace = detect_workspace();
    let workspace_str = workspace.to_string_lossy().to_string();
    let ws_hash = radiumical_core::session::workspace_hash(&workspace_str);
    let session_config = Config::load_for_workspace(&ws_hash);

    let provider = create_provider(
        &session_config.provider,
        session_config.api_base.as_deref(),
        &session_config.api_key,
        &session_config.model,
    );

    let initial_items = {
        let pool = SessionPool::for_workspace(&workspace_str);
        pool.load("autosave")
            .ok()
            .flatten()
            .map(|(_, items)| items)
            .unwrap_or_default()
    };

    let mut runner = PipelineRunner::new(session_config.clone(), provider);
    if !initial_items.is_empty() {
        runner.load_session_items(&initial_items);
    }

    let initial_display = session_items_to_display(&initial_items);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            runner: TokioMutex::new(runner),
            config: TokioMutex::new(session_config),
            cancel_tx: TokioMutex::new(None),
            workspace,
            session_items: Arc::new(TokioMutex::new(initial_items)),
            display_items: Arc::new(TokioMutex::new(initial_display)),
        })
        .invoke_handler(tauri::generate_handler![
            run_task,
            cancel_task,
            is_running,
            new_session,
            list_sessions,
            save_session,
            load_session,
            delete_session,
            get_display,
            get_version,
            get_app_info,
            save_api_key,
            set_model,
            set_mode,
            fetch_providers,
            set_provider,
            fetch_models_for_provider,
            get_config,
            save_config,
            reload_config,
            get_workspaces,
            choice_response,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
