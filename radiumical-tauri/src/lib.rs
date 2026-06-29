use radiumical_core::pipeline::PipelineRunner;
use radiumical_core::provider::create_provider;
use radiumical_core::types::{ProviderKind, SessionConfig};
use tauri::Emitter;
use tokio::sync::Mutex as TokioMutex;

struct AppState {
    runner: TokioMutex<PipelineRunner>,
}

#[tauri::command]
async fn run_task(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    task: String,
) -> Result<String, String> {
    let workspace = std::env::current_dir()
        .unwrap_or_default()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let (ui_tx, mut ui_rx) = tokio::sync::mpsc::unbounded_channel::<radiumical_core::types::UiEvent>();
    let (_, cancel_rx) = tokio::sync::watch::channel(false);

    // Spawn pipeline in background, emit events to frontend
    let handle = app.clone();
    tokio::spawn(async move {
        while let Some(event) = ui_rx.recv().await {
            match event {
                radiumical_core::types::UiEvent::LlmChunk(chunk) => {
                    let _ = handle.emit("llm-chunk", chunk);
                }
                radiumical_core::types::UiEvent::LlmReasoning(reasoning) => {
                    let _ = handle.emit("llm-reasoning", reasoning);
                }
                radiumical_core::types::UiEvent::ThinkingTick => {
                    let _ = handle.emit("thinking-tick", "");
                }
                radiumical_core::types::UiEvent::LlmDone => {
                    let _ = handle.emit("llm-done", "");
                }
                radiumical_core::types::UiEvent::ThinkingDone => {
                    let _ = handle.emit("thinking-done", "");
                }
                radiumical_core::types::UiEvent::ToolStart {
                    name,
                    index,
                    total,
                    args,
                } => {
                    let _ = handle.emit(
                        "tool-start",
                        serde_json::json!({
                            "name": name,
                            "index": index,
                            "total": total,
                            "args": args
                        }),
                    );
                }
                radiumical_core::types::UiEvent::ToolDone => {
                    let _ = handle.emit("tool-done", "");
                }
                radiumical_core::types::UiEvent::ToolResult { content } => {
                    let _ = handle.emit("tool-result", content);
                }
                radiumical_core::types::UiEvent::Choice { id, mode, options } => {
                    let _ = handle.emit(
                        "choice",
                        serde_json::json!({ "id": id, "mode": mode, "options": options }),
                    );
                }
                radiumical_core::types::UiEvent::ProvidersLoaded(sources) => {
                    let _ = handle.emit(
                        "providers-loaded",
                        serde_json::json!({ "sources": sources }),
                    );
                }
                radiumical_core::types::UiEvent::ModelsLoaded(models) => {
                    let _ = handle.emit("models-loaded", serde_json::json!({ "models": models }));
                }
                radiumical_core::types::UiEvent::Error(e) => {
                    let _ = handle.emit("llm-error", e);
                }
                radiumical_core::types::UiEvent::Toast {
                    message, level, ..
                } => {
                    let _ = handle.emit(
                        "toast",
                        serde_json::json!({ "message": message, "level": level }),
                    );
                }
            }
        }
    });

    let mut runner = state.runner.lock().await;
    let result = runner.run(task, workspace, &[], None, ui_tx, cancel_rx).await;
    drop(runner);
    result.map_err(|e| e.to_string())?;
    Ok("Done.".into())
}

#[tauri::command]
fn get_config() -> Result<String, String> {
    Ok(format!("Model: deepseek-v4-pro\nThinking: max"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let api_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
    let provider = create_provider(
        &ProviderKind::OpenAI,
        Some("https://api.deepseek.com/v1"),
        &api_key,
        "deepseek-v4-pro",
    );
    let config = SessionConfig {
        provider: ProviderKind::OpenAI,
        model: "deepseek-v4-pro".into(),
        api_key,
        api_base: Some("https://api.deepseek.com/v1".into()),
        ..Default::default()
    };
    let runner = PipelineRunner::new(config, provider);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            runner: TokioMutex::new(runner),
        })
        .invoke_handler(tauri::generate_handler![run_task, get_config])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
