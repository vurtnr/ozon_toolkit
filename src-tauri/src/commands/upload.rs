use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::Emitter;

use crate::events::EVENT_UPLOAD_PROGRESS;

const UPLOAD_CHUNK_SIZE: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct UploadProgressPayload {
    pub uploaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f64,
    pub status: String,
    pub file_name: String,
    pub source_path: String,
    pub target_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UploadSummary {
    pub source_path: String,
    pub uploaded_path: String,
    pub file_name: String,
    pub total_bytes: u64,
}

fn validate_absolute_excel_path(excel_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(excel_path);
    if !path.is_absolute() {
        return Err("excel_path must be absolute".to_string());
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("xlsx") {
        return Err("excel_path must end with .xlsx".to_string());
    }
    if !path.exists() {
        return Err("excel_path does not exist".to_string());
    }
    Ok(path.to_path_buf())
}

fn build_upload_target_path(source_path: &Path) -> Result<PathBuf, String> {
    let uploads_dir = std::env::temp_dir().join("desktop_app_uploads");
    fs::create_dir_all(&uploads_dir).map_err(|e| format!("create upload dir failed: {e}"))?;

    let source_name = source_path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("upload.xlsx");
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("resolve upload timestamp failed: {e}"))?
        .as_millis();
    Ok(uploads_dir.join(format!("{millis}-{source_name}")))
}

pub fn copy_file_in_chunks<F>(
    source_path: &Path,
    target_path: &Path,
    chunk_size: usize,
    mut on_progress: F,
) -> Result<u64, String>
where
    F: FnMut(u64, u64) -> Result<(), String>,
{
    if chunk_size == 0 {
        return Err("chunk_size must be greater than 0".to_string());
    }

    let total_bytes = source_path
        .metadata()
        .map_err(|e| format!("read source metadata failed: {e}"))?
        .len();

    let source_file = File::open(source_path).map_err(|e| format!("open source failed: {e}"))?;
    let target_file =
        File::create(target_path).map_err(|e| format!("create target failed: {e}"))?;

    let mut reader = BufReader::new(source_file);
    let mut writer = BufWriter::new(target_file);
    let mut buffer = vec![0_u8; chunk_size];
    let mut uploaded_bytes = 0_u64;

    loop {
        let read_size = reader
            .read(&mut buffer)
            .map_err(|e| format!("read source chunk failed: {e}"))?;
        if read_size == 0 {
            break;
        }

        writer
            .write_all(&buffer[..read_size])
            .map_err(|e| format!("write target chunk failed: {e}"))?;

        uploaded_bytes += read_size as u64;
        on_progress(uploaded_bytes, total_bytes)?;
    }

    writer
        .flush()
        .map_err(|e| format!("flush target failed: {e}"))?;
    Ok(total_bytes)
}

fn emit_upload_progress(
    window: &tauri::Window,
    payload: &UploadProgressPayload,
) -> Result<(), String> {
    window
        .emit(EVENT_UPLOAD_PROGRESS, payload)
        .map_err(|e| format!("emit {EVENT_UPLOAD_PROGRESS} failed: {e}"))
}

#[tauri::command]
pub fn upload_excel_file(
    window: tauri::Window,
    excel_path: String,
) -> Result<UploadSummary, String> {
    let source_path = validate_absolute_excel_path(&excel_path)?;
    let target_path = build_upload_target_path(&source_path)?;
    let file_name = source_path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("upload.xlsx")
        .to_string();

    emit_upload_progress(
        &window,
        &UploadProgressPayload {
            uploaded_bytes: 0,
            total_bytes: source_path
                .metadata()
                .map_err(|e| format!("read source metadata failed: {e}"))?
                .len(),
            percent: 0.0,
            status: "uploading".to_string(),
            file_name: file_name.clone(),
            source_path: source_path.to_string_lossy().to_string(),
            target_path: None,
        },
    )?;

    let total_bytes = copy_file_in_chunks(
        &source_path,
        &target_path,
        UPLOAD_CHUNK_SIZE,
        |uploaded, total| {
            let percent = if total == 0 {
                100.0
            } else {
                (uploaded as f64 / total as f64 * 100.0).min(100.0)
            };
            emit_upload_progress(
                &window,
                &UploadProgressPayload {
                    uploaded_bytes: uploaded,
                    total_bytes: total,
                    percent,
                    status: "uploading".to_string(),
                    file_name: file_name.clone(),
                    source_path: source_path.to_string_lossy().to_string(),
                    target_path: None,
                },
            )
        },
    )?;

    let uploaded_path = target_path.to_string_lossy().to_string();
    emit_upload_progress(
        &window,
        &UploadProgressPayload {
            uploaded_bytes: total_bytes,
            total_bytes,
            percent: 100.0,
            status: "completed".to_string(),
            file_name: file_name.clone(),
            source_path: source_path.to_string_lossy().to_string(),
            target_path: Some(uploaded_path.clone()),
        },
    )?;

    Ok(UploadSummary {
        source_path: source_path.to_string_lossy().to_string(),
        uploaded_path,
        file_name,
        total_bytes,
    })
}
