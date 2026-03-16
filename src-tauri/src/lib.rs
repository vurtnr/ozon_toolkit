pub mod commands;
pub mod config;
pub mod core;
pub mod events;
pub mod lifecycle;
pub mod recovery;
// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .on_window_event(|_window, event| {
            if matches!(
                event,
                tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed
            ) {
                commands::run_task::shutdown_managed_sidecar();
            }
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::run_task::run_task,
            commands::run_task::resume_after_challenge,
            commands::upload::upload_excel_file,
            commands::settings::load_settings,
            commands::settings::save_settings,
            commands::settings::get_runtime_platform
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                commands::run_task::shutdown_managed_sidecar();
            }
        });
}
