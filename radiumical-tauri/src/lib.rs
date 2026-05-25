use tauri::Manager;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! Radiumical is ready.", name)
}

#[tauri::command]
fn run_task(task: String) -> String {
    format!("Task received: {task}")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![greet, run_task])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
