use radiumical_core::pipeline::PipelineRunner;
use radiumical_core::provider;
use radiumical_core::types::{ProviderKind, SessionConfig};
use tauri::Manager;

struct AppState {
    runner: std::sync::Mutex<PipelineRunner>,
}

#[tauri::command]
async fn run_task(
    state: tauri::State<'_, AppState>,
    task: String,
) -> Result<String, String> {
    let workspace = std::env::current_dir().unwrap_or_default();
    let (ui_tx, _) = std::sync::mpsc::channel();
    let (_, cancel_rx) = tokio::sync::watch::channel(false);
    
    let mut runner = state.runner.lock().map_err(|e| e.to_string())?;
    // Create a fresh runner for this task
    Ok(format!("Task received: {task} (pipeline not yet wired for Tauri)"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config = SessionConfig::default();
    let provider = provider::create_provider(
        &ProviderKind::OpenAI,
        Some("https://api.deepseek.com/v1"),
        &std::env::var("DEEPSEEK_API_KEY").unwrap_or_default(),
        "deepseek-v4-pro",
    );
    let runner = PipelineRunner::new(config, provider);
    
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState { runner: std::sync::Mutex::new(runner) })
        .invoke_handler(tauri::generate_handler![run_task])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
