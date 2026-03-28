use std::path::PathBuf;

use tauri::Manager;

use crate::config::{
    load_settings_from_disk, save_settings_to_disk, settings_file_path,
    validate_dashscope_api_key_if_present, validate_profit_ratio_if_present, AppSettings,
};

fn settings_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("resolve app config dir failed: {e}"))?;
    Ok(settings_file_path(&config_dir))
}

#[tauri::command]
pub fn load_settings(app: tauri::AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(&app)?;
    load_settings_from_disk(&path)
}

#[tauri::command]
pub fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
    if let Some(api_key) = validate_dashscope_api_key_if_present(&settings.dashscope_api_key)? {
        // Keep key in current app process only; disk persistence is disabled in config store.
        std::env::set_var("DASHSCOPE_API_KEY", api_key);
    }
    validate_profit_ratio_if_present(&settings.profit_ratio)?;

    let path = settings_path(&app)?;
    save_settings_to_disk(&path, &settings)
}

#[tauri::command]
pub fn get_runtime_platform() -> String {
    std::env::consts::OS.to_string()
}
