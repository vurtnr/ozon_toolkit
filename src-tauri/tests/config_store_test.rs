use std::path::PathBuf;

use desktop_app_lib::config::{
    build_sidecar_env, load_settings_from_disk, normalize_chrome_executable_path,
    resolve_effective_dashscope_api_key, save_settings_to_disk,
    validate_dashscope_api_key_if_present, AppSettings,
};

fn temp_settings_path() -> PathBuf {
    let unique = format!(
        "desktop-app-config-test-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

#[test]
fn default_settings_are_empty() {
    let settings = AppSettings::default();
    assert_eq!(settings.dashscope_api_key, "");
    assert_eq!(settings.chrome_executable_path, "");
}

#[test]
fn settings_can_persist_and_reload() {
    let path = temp_settings_path();
    let settings = AppSettings {
        dashscope_api_key: "secret-key".to_string(),
        chrome_executable_path: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
            .to_string(),
    };

    save_settings_to_disk(&path, &settings).expect("save should succeed");
    let loaded = load_settings_from_disk(&path).expect("load should succeed");

    assert_eq!(loaded.dashscope_api_key, "");
    assert_eq!(
        loaded.chrome_executable_path,
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn loading_settings_masks_legacy_persisted_api_key() {
    let path = temp_settings_path();
    std::fs::write(
        &path,
        r#"{"dashscope_api_key":"legacy-secret","chrome_executable_path":"/tmp/chrome"}"#,
    )
    .expect("seed legacy settings should succeed");

    let loaded = load_settings_from_disk(&path).expect("load should succeed");
    assert_eq!(loaded.dashscope_api_key, "");
    assert_eq!(loaded.chrome_executable_path, "/tmp/chrome");

    let _ = std::fs::remove_file(path);
}

#[test]
fn sidecar_env_includes_chrome_path_when_provided() {
    let settings = AppSettings {
        dashscope_api_key: "k".to_string(),
        chrome_executable_path: "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"
            .to_string(),
    };

    let env = build_sidecar_env(&settings);
    assert!(env.contains(&(
        "CHROME_EXECUTABLE_PATH".to_string(),
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe".to_string(),
    )));
}

#[test]
fn loading_settings_normalizes_legacy_macos_app_bundle_paths() {
    let path = temp_settings_path();
    std::fs::write(
        &path,
        r#"{"dashscope_api_key":"","chrome_executable_path":"/Applications/Google Chrome.app"}"#,
    )
    .expect("seed legacy settings should succeed");

    let loaded = load_settings_from_disk(&path).expect("load should succeed");
    assert_eq!(
        loaded.chrome_executable_path,
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn build_sidecar_env_normalizes_macos_app_bundle_paths() {
    let settings = AppSettings {
        dashscope_api_key: String::new(),
        chrome_executable_path: "/Applications/Google Chrome.app".to_string(),
    };

    let env = build_sidecar_env(&settings);
    assert!(env.contains(&(
        "CHROME_EXECUTABLE_PATH".to_string(),
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".to_string(),
    )));
}

#[test]
fn normalize_chrome_executable_path_keeps_non_bundle_paths_intact() {
    assert_eq!(
        normalize_chrome_executable_path(
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"
        ),
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"
    );
}

#[test]
fn dashscope_api_key_validation_rejects_short_or_whitespace_values() {
    assert!(validate_dashscope_api_key_if_present("").is_ok());
    assert!(validate_dashscope_api_key_if_present("   ").is_ok());
    assert!(validate_dashscope_api_key_if_present("short-key").is_err());
    assert!(validate_dashscope_api_key_if_present("abc def ghi jkl").is_err());
    assert!(
        validate_dashscope_api_key_if_present("sk-valid-key-1234567890")
            .expect("valid key should parse")
            .is_some()
    );
}

#[test]
fn effective_key_prefers_env_over_settings() {
    let original_env = std::env::var("DASHSCOPE_API_KEY").ok();
    std::env::set_var("DASHSCOPE_API_KEY", "sk-env-priority-1234567890");

    let settings = AppSettings {
        dashscope_api_key: "sk-settings-fallback-1234567890".to_string(),
        chrome_executable_path: String::new(),
    };
    let resolved =
        resolve_effective_dashscope_api_key(Some(&settings)).expect("should resolve from env");
    assert_eq!(resolved, "sk-env-priority-1234567890");

    if let Some(value) = original_env {
        std::env::set_var("DASHSCOPE_API_KEY", value);
    } else {
        std::env::remove_var("DASHSCOPE_API_KEY");
    }
}
