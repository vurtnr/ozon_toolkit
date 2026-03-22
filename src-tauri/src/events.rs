use serde::Serialize;
use tauri::Emitter;

pub const EVENT_PROGRESS: &str = "progress";
pub const EVENT_LOG: &str = "log";
pub const EVENT_ROW_RESULT: &str = "row_result";
pub const EVENT_TASK_DONE: &str = "task_done";
pub const EVENT_BLOCKING_ALERT: &str = "blocking_alert";
pub const EVENT_TASK_PHASE: &str = "task_phase";
pub const EVENT_UPLOAD_PROGRESS: &str = "upload_progress";

pub trait EventSink {
    fn emit_json(&mut self, event: &str, payload: serde_json::Value) -> Result<(), String>;
}

pub fn emit_event<T: Serialize>(
    sink: &mut dyn EventSink,
    event: &str,
    payload: &T,
) -> Result<(), String> {
    let value = serde_json::to_value(payload)
        .map_err(|e| format!("serialize event payload failed: {e}"))?;
    sink.emit_json(event, value)
}

pub struct TauriWindowSink<'a> {
    window: &'a tauri::Window,
}

impl<'a> TauriWindowSink<'a> {
    pub fn new(window: &'a tauri::Window) -> Self {
        Self { window }
    }
}

impl EventSink for TauriWindowSink<'_> {
    fn emit_json(&mut self, event: &str, payload: serde_json::Value) -> Result<(), String> {
        self.window
            .emit(event, payload)
            .map_err(|e| format!("emit {event} failed: {e}"))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    pub processed: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEvent {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RowResultEvent {
    pub row_index: u32,
    pub sku: String,
    pub stage: String,
    pub status: String,
    pub image_url: Option<String>,
    pub original_image_url: Option<String>,
    pub matched_image_url: Option<String>,
    pub item_url: Option<String>,
    pub price: Option<String>,
    pub elapsed_text: Option<String>,
    pub is_final: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskDoneEvent {
    pub excel_path: String,
    pub status: String,
    pub processed_rows: u32,
    pub total_rows: u32,
    pub result_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskPhaseEvent {
    pub phase: String,
    pub label: String,
    pub detail: String,
    pub blocking: bool,
}
