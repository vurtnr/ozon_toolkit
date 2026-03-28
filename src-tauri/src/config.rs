use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MIN_DASHSCOPE_KEY_LEN: usize = 16;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppSettings {
    pub dashscope_api_key: String,
    pub chrome_executable_path: String,
    pub profit_ratio: String,
}

pub fn settings_file_path(base_dir: &Path) -> PathBuf {
    base_dir.join("settings.json")
}

pub fn normalize_chrome_executable_path(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let normalized = trimmed.trim_end_matches('/');
    if normalized.contains(".app/Contents/MacOS/") {
        return normalized.to_string();
    }

    let Some(bundle_root) = normalized.strip_suffix(".app") else {
        return normalized.to_string();
    };

    let executable_name = bundle_root
        .rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or(bundle_root);

    format!("{bundle_root}.app/Contents/MacOS/{executable_name}")
}

pub fn load_settings_from_disk(path: &Path) -> Result<AppSettings, String> {
    if !path.exists() {
        return Ok(AppSettings::default());
    }

    let content = fs::read_to_string(path).map_err(|e| format!("read settings failed: {e}"))?;
    let mut parsed = serde_json::from_str::<AppSettings>(&content)
        .map_err(|e| format!("parse settings failed: {e}"))?;
    // Never expose persisted API keys back to UI/runtime config reads.
    parsed.dashscope_api_key.clear();
    parsed.chrome_executable_path =
        normalize_chrome_executable_path(&parsed.chrome_executable_path);
    Ok(parsed)
}

pub fn save_settings_to_disk(path: &Path, settings: &AppSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create settings dir failed: {e}"))?;
    }

    // API key is session-only; do not persist to disk.
    let persisted = AppSettings {
        dashscope_api_key: String::new(),
        chrome_executable_path: normalize_chrome_executable_path(&settings.chrome_executable_path),
        profit_ratio: settings.profit_ratio.trim().to_string(),
    };
    let content = serde_json::to_string_pretty(&persisted)
        .map_err(|e| format!("serialize settings failed: {e}"))?;
    fs::write(path, content).map_err(|e| format!("write settings failed: {e}"))
}

fn normalize_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn validate_dashscope_api_key_if_present(value: &str) -> Result<Option<String>, String> {
    let Some(trimmed) = normalize_non_empty(value) else {
        return Ok(None);
    };

    if trimmed.chars().any(|ch| ch.is_whitespace()) {
        return Err("DASHSCOPE_API_KEY 不能包含空白字符".to_string());
    }
    if trimmed.len() < MIN_DASHSCOPE_KEY_LEN {
        return Err(format!(
            "DASHSCOPE_API_KEY 长度过短（至少 {MIN_DASHSCOPE_KEY_LEN} 个字符）"
        ));
    }

    Ok(Some(trimmed))
}

pub fn validate_profit_ratio_if_present(value: &str) -> Result<Option<String>, String> {
    let Some(trimmed) = normalize_non_empty(value) else {
        return Ok(None);
    };

    if trimmed.chars().any(|ch| ch.is_whitespace()) {
        return Err("利润比例不能包含空白字符".to_string());
    }
    if !trimmed.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
        return Err("利润比例只能包含数字，且最多保留两位小数".to_string());
    }

    let mut parts = trimmed.split('.');
    let integer_part = parts.next().unwrap_or_default();
    let fractional_part = parts.next();
    if integer_part.is_empty() || parts.next().is_some() {
        return Err("利润比例格式无效".to_string());
    }
    if let Some(fraction) = fractional_part {
        if fraction.is_empty() || fraction.len() > 2 {
            return Err("利润比例最多保留两位小数".to_string());
        }
    }

    let parsed = trimmed
        .parse::<f64>()
        .map_err(|_| "利润比例格式无效".to_string())?;
    if !parsed.is_finite() || parsed <= 0.0 || parsed >= 100.0 {
        return Err("利润比例必须大于 0 且小于 100".to_string());
    }

    Ok(Some(trimmed))
}

pub fn resolve_effective_dashscope_api_key(
    settings: Option<&AppSettings>,
) -> Result<String, String> {
    if let Ok(value) = std::env::var("DASHSCOPE_API_KEY") {
        if let Some(valid) = validate_dashscope_api_key_if_present(&value)? {
            return Ok(valid);
        }
    }

    if let Some(settings) = settings {
        if let Some(valid) = validate_dashscope_api_key_if_present(&settings.dashscope_api_key)? {
            return Ok(valid);
        }
    }

    Err(
        "未检测到有效 DASHSCOPE_API_KEY。请在系统环境变量中设置，或在设置页输入后保存。"
            .to_string(),
    )
}

pub fn resolve_effective_profit_ratio(settings: Option<&AppSettings>) -> Result<f64, String> {
    if let Some(settings) = settings {
        if let Some(valid) = validate_profit_ratio_if_present(&settings.profit_ratio)? {
            return valid
                .parse::<f64>()
                .map_err(|_| "利润比例格式无效".to_string());
        }
    }

    Err("未设置有效利润比例，请在首页输入大于 0 且小于 100 的利润比例。".to_string())
}

pub fn build_sidecar_env(settings: &AppSettings) -> Vec<(String, String)> {
    let mut envs = Vec::new();

    let chrome = normalize_chrome_executable_path(&settings.chrome_executable_path);
    if !chrome.is_empty() {
        envs.push(("CHROME_EXECUTABLE_PATH".to_string(), chrome));
    }

    let api_key = settings.dashscope_api_key.trim();
    if !api_key.is_empty() {
        envs.push(("DASHSCOPE_API_KEY".to_string(), api_key.to_string()));
    }

    envs
}
