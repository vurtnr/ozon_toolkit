use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use calamine::{open_workbook, Reader, Xlsx};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use regex::Regex;
use reqwest::blocking::Client;
use rust_xlsxwriter::{Color, Format, Workbook};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::config::{
    build_sidecar_env, load_settings_from_disk, resolve_effective_dashscope_api_key,
    settings_file_path, AppSettings,
};
use crate::core::excel::extract_wps_images;
use crate::core::orchestrator::{
    orchestrate_match, CandidateFetcher, NoMatchReason, OrchestrationDiagnostics, SearchPass,
    VlmCallStage,
};
use crate::core::search_image::{
    generate_search_images, parse_search_image_plan, GeneratedSearchImages, SearchImagePlan,
};
use crate::core::types::{Candidate, MatchSummary};
use crate::core::vlm::{
    DashScopeVlmClient, ReferenceImages, SearchImagePlanner, VlmBatchRequest, VlmCallTrace,
    VlmClient, VlmMatchResult,
};
use crate::events::{
    emit_event, EventSink, LogEvent, ProgressEvent, RowResultEvent, TaskDoneEvent, TauriWindowSink,
    EVENT_BLOCKING_ALERT, EVENT_LOG, EVENT_PROGRESS, EVENT_ROW_RESULT, EVENT_TASK_DONE,
};
use crate::lifecycle::cleanup::run_with_task_guard;
use crate::recovery::{
    blocking_alert_for_code, CODE_ANTI_BOT_CHALLENGE, CODE_CHROME_NOT_FOUND, CODE_LOGIN_REQUIRED,
    CODE_RESUME_REQUIRED, GLOBAL_RECOVERY_GATE,
};

const DEFAULT_SIDECAR_SEARCH_URL: &str = "http://127.0.0.1:8266/search";
const DEFAULT_SIDECAR_HEALTH_URL: &str = "http://127.0.0.1:8266/health";
const DEFAULT_SIDECAR_SESSION_URL: &str = "http://127.0.0.1:8266/session-state";
const DEFAULT_SIDECAR_SHUTDOWN_URL: &str = "http://127.0.0.1:8266/shutdown";
const MOCK_CANDIDATES_ENV: &str = "RUN_TASK_MOCK_CANDIDATES_JSON";
const MOCK_CANDIDATE_RESPONSES_ENV: &str = "RUN_TASK_MOCK_CANDIDATE_RESPONSES_JSON";
const MOCK_SEARCH_IMAGE_PLAN_ENV: &str = "RUN_TASK_MOCK_SEARCH_IMAGE_PLAN_JSON";
const MOCK_VLM_REPLIES_ENV: &str = "RUN_TASK_MOCK_VLM_REPLIES_JSON";
const SIDECAR_HEALTH_URL_ENV: &str = "SIDECAR_HEALTH_URL";
const DIAGNOSTICS_ROOT_ENV: &str = "RUN_TASK_DIAGNOSTICS_ROOT";
const ALWAYS_WRITE_DIAGNOSTICS_ENV: &str = "RUN_TASK_ALWAYS_WRITE_DIAGNOSTICS";
const DIAGNOSTICS_DELAY_MS_ENV: &str = "RUN_TASK_DIAGNOSTICS_DELAY_MS";
const SIDECAR_SHUTDOWN_URL_ENV: &str = "SIDECAR_SHUTDOWN_URL";
const SIDECAR_URL_ENV: &str = "SIDECAR_SEARCH_URL";
const SIDECAR_SESSION_URL_ENV: &str = "SIDECAR_SESSION_URL";
const SIDECAR_EXECUTABLE_PATH_ENV: &str = "SIDECAR_EXECUTABLE_PATH";
const SIDECAR_PROFILE_DIR_ENV: &str = "SIDECAR_PROFILE_DIR";
const SIDECAR_WAIT_TIMEOUT_SECS: u64 = 15;
const SIDECAR_SESSION_POLL_INTERVAL_MILLIS: u64 = 1_500;

static SIDECAR_CHILD: Mutex<Option<Child>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct RunTaskSummary {
    pub excel_path: String,
    pub processed_rows: u32,
    pub total_rows: u32,
    pub status: String,
    pub result_path: Option<String>,
}

#[derive(Debug, Clone)]
struct TaskWorkbook {
    headers: Vec<String>,
    rows: Vec<TaskRow>,
}

#[derive(Debug, Clone)]
struct TaskRow {
    excel_row_index: u32,
    ozon_name: String,
    sku: String,
    original_cells: Vec<String>,
    image_bytes: Option<Vec<u8>>,
}

#[derive(Debug)]
struct TaskOutputRow {
    row_index: u32,
    sku: String,
    original_cells: Vec<String>,
    status: String,
    compare_elapsed_text: Option<String>,
    price: Option<String>,
    item_url: Option<String>,
    image_url: Option<String>,
}

#[derive(Debug, Clone)]
struct TaskDiagnosticsSession {
    root_dir: PathBuf,
}

#[derive(Debug, Clone, Default, Serialize)]
struct RowStageTimings {
    search_plan_ms: Option<u64>,
    search_image_render_ms: Option<u64>,
    primary_search_ms: Option<u64>,
    fallback_search_ms: Option<u64>,
    screening_ms: Option<u64>,
    screening_candidate_count: usize,
    screening_chunk_count: usize,
    final_review_ms: Option<u64>,
    final_review_candidate_count: usize,
    final_review_batch_count: usize,
}

#[derive(Debug, Clone)]
struct PendingDiagnosticsJob {
    row: TaskRow,
    status: String,
    search_plan: SearchImagePlan,
    search_images: GeneratedSearchImages,
    source_image_path: PathBuf,
    diagnostics: OrchestrationDiagnostics,
    used_fallback_image: bool,
    no_match_reason: Option<NoMatchReason>,
    row_stage_timings: RowStageTimings,
}

#[derive(Debug, Serialize)]
struct DiagnosticManifest<'a> {
    row_index: u32,
    sku: &'a str,
    ozon_name: &'a str,
    status: &'a str,
    used_fallback_image: bool,
    no_match_reason: Option<&'a str>,
    search_plan: &'a SearchImagePlan,
    row_stage_timings: &'a RowStageTimings,
}

#[derive(Debug, Serialize)]
struct DiagnosticVlmCallMetadata<'a> {
    pass_label: &'a str,
    stage: &'a str,
    chunk_index: usize,
    match_ids: &'a [usize],
    candidates: &'a [Candidate],
}

#[derive(Debug, Serialize)]
struct SidecarSearchRequest {
    #[serde(rename = "imagePath")]
    image_path: String,
    #[serde(rename = "forceFullCrop")]
    force_full_crop: bool,
}

#[derive(Debug, Deserialize)]
struct SidecarSearchResponse {
    success: bool,
    data: Option<Vec<Candidate>>,
    code: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SidecarSessionResponse {
    success: bool,
    status: Option<String>,
    code: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidecarSessionState {
    Ready,
    LoginRequired,
    AntiBotChallenge,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MockCandidateResponseEntry {
    Candidates(Vec<Candidate>),
    Error { err: String },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MockVlmReplyEntry {
    MatchIds(Vec<usize>),
    Error { err: String },
}

#[derive(Debug, Clone, Copy, Default)]
struct SearchPassTimings {
    primary_search_ms: Option<u64>,
    fallback_search_ms: Option<u64>,
}

struct MockVlmClient {
    replies: Mutex<VecDeque<Result<Vec<usize>, String>>>,
}

impl MockVlmClient {
    fn from_env() -> Result<Option<Self>, String> {
        let Ok(raw) = std::env::var(MOCK_VLM_REPLIES_ENV) else {
            return Ok(None);
        };

        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let parsed = serde_json::from_str::<Vec<MockVlmReplyEntry>>(trimmed)
            .map_err(|e| format!("parse {MOCK_VLM_REPLIES_ENV} failed: {e}"))?;
        let replies = parsed
            .into_iter()
            .map(|entry| match entry {
                MockVlmReplyEntry::MatchIds(match_ids) => Ok(match_ids),
                MockVlmReplyEntry::Error { err } => Err(err),
            })
            .collect();

        Ok(Some(Self {
            replies: Mutex::new(replies),
        }))
    }
}

impl VlmClient for MockVlmClient {
    fn match_candidate_grid(
        &self,
        _references: ReferenceImages<'_>,
        candidates: &[Candidate],
        _ozon_name_opt: Option<&str>,
    ) -> Result<VlmMatchResult, String> {
        let match_ids = self
            .replies
            .lock()
            .map_err(|_| "mock vlm lock poisoned".to_string())?
            .pop_front()
            .unwrap_or_else(|| Ok(Vec::new()))?;

        Ok(VlmMatchResult {
            trace: VlmCallTrace {
                system_prompt: "mock-system".to_string(),
                user_prompt: "mock-user".to_string(),
                raw_response_text: serde_json::json!({
                    "reasoning": "mock",
                    "match_ids": match_ids,
                })
                .to_string(),
                grid_jpeg_bytes: Vec::new(),
                candidates: candidates.to_vec(),
            },
            match_ids,
        })
    }
}

impl SearchImagePlanner for MockVlmClient {
    fn plan_search_images(
        &self,
        _ozon_image_base64: &str,
        _ozon_name: &str,
    ) -> Result<SearchImagePlan, String> {
        Err("mock search image planner not configured".to_string())
    }
}

enum RuntimeVlmClient {
    DashScope(DashScopeVlmClient),
    Mock(MockVlmClient),
}

impl VlmClient for RuntimeVlmClient {
    fn match_candidate_grid(
        &self,
        references: ReferenceImages<'_>,
        candidates: &[Candidate],
        ozon_name_opt: Option<&str>,
    ) -> Result<VlmMatchResult, String> {
        match self {
            Self::DashScope(client) => {
                client.match_candidate_grid(references, candidates, ozon_name_opt)
            }
            Self::Mock(client) => {
                client.match_candidate_grid(references, candidates, ozon_name_opt)
            }
        }
    }

    fn match_candidate_grids<'a>(
        &self,
        requests: &[VlmBatchRequest<'a>],
        ozon_name_opt: Option<&str>,
    ) -> Vec<Result<VlmMatchResult, String>> {
        match self {
            Self::DashScope(client) => client.match_candidate_grids(requests, ozon_name_opt),
            Self::Mock(client) => client.match_candidate_grids(requests, ozon_name_opt),
        }
    }
}

impl SearchImagePlanner for RuntimeVlmClient {
    fn plan_search_images(
        &self,
        ozon_image_base64: &str,
        ozon_name: &str,
    ) -> Result<SearchImagePlan, String> {
        match self {
            Self::DashScope(client) => client.plan_search_images(ozon_image_base64, ozon_name),
            Self::Mock(client) => client.plan_search_images(ozon_image_base64, ozon_name),
        }
    }
}

struct SidecarCandidateFetcher<'a> {
    sink: RefCell<&'a mut dyn EventSink>,
    client: &'a Client,
    call_count: Cell<usize>,
    mock_sequence: RefCell<Option<VecDeque<Result<Vec<Candidate>, String>>>>,
    timings: RefCell<SearchPassTimings>,
    row_index: u32,
    sku: String,
}

impl<'a> SidecarCandidateFetcher<'a> {
    fn new(
        sink: &'a mut dyn EventSink,
        client: &'a Client,
        mock_sequence: Option<VecDeque<Result<Vec<Candidate>, String>>>,
        row_index: u32,
        sku: impl Into<String>,
    ) -> Self {
        Self {
            sink: RefCell::new(sink),
            client,
            call_count: Cell::new(0),
            mock_sequence: RefCell::new(mock_sequence),
            timings: RefCell::new(SearchPassTimings::default()),
            row_index,
            sku: sku.into(),
        }
    }

    fn into_mock_sequence(self) -> Option<VecDeque<Result<Vec<Candidate>, String>>> {
        self.mock_sequence.into_inner()
    }

    fn timings(&self) -> SearchPassTimings {
        *self.timings.borrow()
    }
}

impl CandidateFetcher for SidecarCandidateFetcher<'_> {
    fn fetch_candidates(&self, image_path: &Path) -> Result<Vec<Candidate>, String> {
        let mut sink = self.sink.borrow_mut();
        let mut mock_sequence = self.mock_sequence.borrow_mut();
        let call_count = self.call_count.get();
        self.call_count.set(call_count + 1);
        let (stage_key, stage_message) = if call_count == 0 {
            ("searching_1688_primary", "主搜索图搜索中")
        } else {
            ("searching_1688_fallback", "备用搜索图搜索中")
        };
        emit_row_event(
            &mut **sink,
            self.row_index,
            &self.sku,
            stage_key,
            stage_message.to_string(),
            None,
            None,
            None,
            None,
            false,
        )?;
        emit_event(
            &mut **sink,
            EVENT_LOG,
            &LogEvent {
                level: "info".to_string(),
                message: stage_message.to_string(),
            },
        )?;
        let search_started_at = Instant::now();
        let result = fetch_candidates_with_session_recovery(
            &mut **sink,
            self.client,
            image_path,
            false,
            &mut *mock_sequence,
        );
        let search_elapsed_ms = elapsed_millis(search_started_at.elapsed());
        let elapsed_text = format_elapsed_text(search_elapsed_ms);
        {
            let mut timings = self.timings.borrow_mut();
            if call_count == 0 {
                timings.primary_search_ms = Some(search_elapsed_ms);
            } else {
                timings.fallback_search_ms = Some(search_elapsed_ms);
            }
        }
        if let Ok(candidates) = &result {
            let (next_stage, next_status) = if candidates.is_empty() {
                if call_count == 0 {
                    (
                        "searching_1688_fallback",
                        "主搜索图未召回有效候选，准备备用搜索图".to_string(),
                    )
                } else {
                    ("screening_candidates", "双搜索图未召回有效候选".to_string())
                }
            } else {
                (
                    "screening_candidates",
                    format!("已召回 {} 个候选，AI复核中", candidates.len()),
                )
            };
            emit_row_event(
                &mut **sink,
                self.row_index,
                &self.sku,
                next_stage,
                next_status,
                None,
                None,
                None,
                Some(elapsed_text.clone()),
                false,
            )?;
            emit_event(
                &mut **sink,
                EVENT_LOG,
                &LogEvent {
                    level: "info".to_string(),
                    message: format!(
                        "{}完成: {} 个候选, {}",
                        stage_message,
                        candidates.len(),
                        elapsed_text
                    ),
                },
            )?;
        } else if let Err(error) = &result {
            emit_event(
                &mut **sink,
                EVENT_LOG,
                &LogEvent {
                    level: "warn".to_string(),
                    message: format!("{}失败: {} ({})", stage_message, error, elapsed_text),
                },
            )?;
        }
        result
    }
}

fn validate_absolute_excel_path(excel_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(excel_path);

    if !path.is_absolute() {
        return Err("excel_path must be absolute".to_string());
    }
    if path.extension().and_then(|v| v.to_str()) != Some("xlsx") {
        return Err("excel_path must end with .xlsx".to_string());
    }
    if !path.exists() {
        return Err("excel_path does not exist".to_string());
    }

    Ok(path.to_path_buf())
}

fn load_task_rows(excel_path: &Path) -> Result<TaskWorkbook, String> {
    let mut workbook: Xlsx<_> =
        open_workbook(excel_path).map_err(|e| format!("open workbook failed: {e}"))?;

    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| "workbook has no worksheet".to_string())?;
    let formula_range = workbook
        .worksheet_formula(&sheet_name)
        .and_then(|result| result.ok());

    let range = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| "workbook has no worksheet".to_string())?
        .map_err(|e| format!("read worksheet failed: {e}"))?;

    let image_map = extract_wps_images(excel_path.to_string_lossy().as_ref())?;
    let image_id_re = Regex::new(r#"ID_[A-Za-z0-9]{32}"#)
        .map_err(|e| format!("compile image id regex failed: {e}"))?;

    let headers = range
        .rows()
        .next()
        .map(|row| row.iter().map(|cell| cell.to_string()).collect::<Vec<_>>())
        .unwrap_or_default();

    let mut rows = Vec::new();
    for (idx, row) in range.rows().enumerate().skip(1) {
        let original_cells = row.iter().map(|cell| cell.to_string()).collect::<Vec<_>>();
        let ozon_name = row
            .first()
            .map(|v| v.to_string())
            .map(|v| v.trim().to_string())
            .unwrap_or_default();
        let sku = row
            .get(1)
            .map(|v| v.to_string())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "UNKNOWN_SKU".to_string());

        let mut image_bytes = None;
        for col_idx in 0..row.len() {
            let mut image_id = image_id_re
                .captures(&row[col_idx].to_string())
                .and_then(|caps| caps.get(0).map(|m| m.as_str().to_string()));

            if image_id.is_none() {
                image_id = formula_range
                    .as_ref()
                    .and_then(|fr| fr.get_value((idx as u32, col_idx as u32)))
                    .and_then(|formula| {
                        image_id_re
                            .captures(formula)
                            .and_then(|caps| caps.get(0).map(|m| m.as_str().to_string()))
                    });
            }

            if let Some(id) = image_id {
                if let Some(bytes) = image_map.get(&id) {
                    image_bytes = Some(bytes.clone());
                    break;
                }
            }
        }

        rows.push(TaskRow {
            excel_row_index: (idx + 1) as u32,
            ozon_name,
            sku,
            original_cells,
            image_bytes,
        });
    }

    Ok(TaskWorkbook { headers, rows })
}

fn simulated_sidecar_error(excel_path: &Path) -> Option<String> {
    if let Ok(code) = std::env::var("SIMULATE_SIDECAR_ERROR") {
        let trimmed = code.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let file_name = excel_path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_lowercase();

    if file_name.contains("chrome-not-found") {
        return Some(CODE_CHROME_NOT_FOUND.to_string());
    }
    if file_name.contains("login-required") {
        return Some(CODE_LOGIN_REQUIRED.to_string());
    }
    if file_name.contains("anti-bot") {
        return Some(CODE_ANTI_BOT_CHALLENGE.to_string());
    }

    None
}

fn emit_blocking_alert_if_needed(sink: &mut dyn EventSink, code: &str) -> Result<(), String> {
    if let Some(alert) = blocking_alert_for_code(code) {
        emit_event(sink, EVENT_BLOCKING_ALERT, &alert)?;
    }
    Ok(())
}

fn sanitize_filename(input: &str) -> String {
    let value: String = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if value.is_empty() {
        "UNKNOWN_SKU".to_string()
    } else {
        value
    }
}

impl TaskDiagnosticsSession {
    fn new(output_anchor_path: &Path) -> Result<Self, String> {
        let root = diagnostics_root_for_excel(output_anchor_path);
        let excel_stem = output_anchor_path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("task");
        let session_name = format!(
            "{}-{}",
            sanitize_filename(excel_stem),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| format!("build diagnostics timestamp failed: {e}"))?
                .as_nanos()
        );
        let root_dir = root.join(session_name);
        std::fs::create_dir_all(&root_dir)
            .map_err(|e| format!("create diagnostics session dir failed: {e}"))?;
        Ok(Self { root_dir })
    }

    fn save_row_bundle(
        &self,
        row: &TaskRow,
        status: &str,
        search_plan: &SearchImagePlan,
        search_images: &GeneratedSearchImages,
        source_image_path: &Path,
        diagnostics: &OrchestrationDiagnostics,
        used_fallback_image: bool,
        no_match_reason: Option<&NoMatchReason>,
        row_stage_timings: &RowStageTimings,
    ) -> Result<PathBuf, String> {
        maybe_delay_diagnostics_write_for_tests();
        let row_dir = self.root_dir.join(format!(
            "{:04}-{}",
            row.excel_row_index,
            sanitize_filename(&row.sku)
        ));
        std::fs::create_dir_all(&row_dir)
            .map_err(|e| format!("create row diagnostics dir failed: {e}"))?;

        copy_file_to(
            source_image_path,
            &row_dir.join(source_image_output_name(source_image_path)),
        )?;
        copy_file_to(
            &search_images.primary_path,
            &row_dir.join("search_primary.png"),
        )?;
        copy_file_to(
            &search_images.fallback_path,
            &row_dir.join("search_fallback.png"),
        )?;

        write_json_pretty(
            &row_dir.join("manifest.json"),
            &DiagnosticManifest {
                row_index: row.excel_row_index,
                sku: &row.sku,
                ozon_name: &row.ozon_name,
                status,
                used_fallback_image,
                no_match_reason: no_match_reason.map(no_match_reason_label),
                search_plan,
                row_stage_timings,
            },
        )?;
        write_json_pretty(
            &row_dir.join("primary_candidates.json"),
            &diagnostics.primary_candidates,
        )?;
        if !diagnostics.fallback_candidates.is_empty() {
            write_json_pretty(
                &row_dir.join("fallback_candidates.json"),
                &diagnostics.fallback_candidates,
            )?;
        }

        let vlm_dir = row_dir.join("vlm_calls");
        std::fs::create_dir_all(&vlm_dir)
            .map_err(|e| format!("create vlm calls dir failed: {e}"))?;
        for (call_index, call) in diagnostics.vlm_calls.iter().enumerate() {
            let call_dir = vlm_dir.join(format!(
                "{:02}-{}-{}-{}",
                call_index + 1,
                call.pass_label,
                vlm_stage_label(&call.stage),
                call.chunk_index
            ));
            std::fs::create_dir_all(&call_dir)
                .map_err(|e| format!("create vlm call dir failed: {e}"))?;
            write_json_pretty(
                &call_dir.join("metadata.json"),
                &DiagnosticVlmCallMetadata {
                    pass_label: &call.pass_label,
                    stage: vlm_stage_label(&call.stage),
                    chunk_index: call.chunk_index,
                    match_ids: &call.match_ids,
                    candidates: &call.trace.candidates,
                },
            )?;
            std::fs::write(
                call_dir.join("system_prompt.txt"),
                &call.trace.system_prompt,
            )
            .map_err(|e| format!("write system prompt failed: {e}"))?;
            std::fs::write(call_dir.join("user_prompt.txt"), &call.trace.user_prompt)
                .map_err(|e| format!("write user prompt failed: {e}"))?;
            std::fs::write(
                call_dir.join("response_raw.txt"),
                &call.trace.raw_response_text,
            )
            .map_err(|e| format!("write raw response failed: {e}"))?;
            if !call.trace.grid_jpeg_bytes.is_empty() {
                std::fs::write(call_dir.join("grid.jpg"), &call.trace.grid_jpeg_bytes)
                    .map_err(|e| format!("write vlm grid failed: {e}"))?;
            }
        }

        Ok(row_dir)
    }
}

fn diagnostics_root_for_excel(excel_path: &Path) -> PathBuf {
    std::env::var(DIAGNOSTICS_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            excel_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("desktop_app_diagnostics")
        })
}

fn resolve_output_anchor_path(
    uploaded_excel_path: &Path,
    original_source_excel_path: Option<&str>,
) -> PathBuf {
    original_source_excel_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| uploaded_excel_path.to_path_buf())
}

fn should_write_diagnostics(
    summary: &MatchSummary,
    no_match_reason: Option<&NoMatchReason>,
) -> bool {
    if std::env::var(ALWAYS_WRITE_DIAGNOSTICS_ENV)
        .ok()
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
    {
        return true;
    }

    matches!(
        summary,
        MatchSummary::NoMatch | MatchSummary::MatchedButPriceUnavailable { .. }
    ) || no_match_reason.is_some()
}

fn no_match_reason_label(reason: &NoMatchReason) -> &'static str {
    match reason {
        NoMatchReason::NoCandidates => "no_candidates",
        NoMatchReason::InitialScreenNoMatch => "initial_screen_no_match",
        NoMatchReason::FinalReviewRejected => "final_review_rejected",
    }
}

fn vlm_stage_label(stage: &VlmCallStage) -> &'static str {
    match stage {
        VlmCallStage::Screening => "screening",
        VlmCallStage::FinalReview => "final_review",
    }
}

fn source_image_output_name(source_image_path: &Path) -> &'static str {
    match source_image_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "source_image.jpg",
        _ => "source_image.png",
    }
}

fn copy_file_to(source_path: &Path, destination_path: &Path) -> Result<(), String> {
    std::fs::copy(source_path, destination_path)
        .map(|_| ())
        .map_err(|e| format!("copy file into diagnostics failed: {e}"))
}

fn write_json_pretty<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| format!("serialize diagnostics json failed: {e}"))?;
    std::fs::write(path, bytes).map_err(|e| format!("write diagnostics json failed: {e}"))
}

fn maybe_delay_diagnostics_write_for_tests() {
    let delay_ms = std::env::var(DIAGNOSTICS_DELAY_MS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0);
    if delay_ms > 0 {
        std::thread::sleep(Duration::from_millis(delay_ms));
    }
}

fn elapsed_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn format_elapsed_text(elapsed_ms: u64) -> String {
    format!("{:.2}s", elapsed_ms as f64 / 1000.0)
}

fn format_stage_timing_summary(sku: &str, timings: &RowStageTimings) -> String {
    let mut parts = Vec::new();
    if let Some(value) = timings.search_plan_ms {
        parts.push(format!("搜索图规划={}", format_elapsed_text(value)));
    }
    if let Some(value) = timings.search_image_render_ms {
        parts.push(format!("搜索图生成={}", format_elapsed_text(value)));
    }
    if let Some(value) = timings.primary_search_ms {
        parts.push(format!("主搜={}", format_elapsed_text(value)));
    }
    if let Some(value) = timings.fallback_search_ms {
        parts.push(format!("备用搜索={}", format_elapsed_text(value)));
    }
    if let Some(value) = timings.screening_ms {
        parts.push(format!(
            "AI初筛={}({}候选/{}轮)",
            format_elapsed_text(value),
            timings.screening_candidate_count,
            timings.screening_chunk_count
        ));
    }
    if let Some(value) = timings.final_review_ms {
        parts.push(format!(
            "终审={}({}候选/{}轮)",
            format_elapsed_text(value),
            timings.final_review_candidate_count,
            timings.final_review_batch_count
        ));
    }

    if parts.is_empty() {
        format!("{sku} 阶段耗时: 无可用数据")
    } else {
        format!("{sku} 阶段耗时: {}", parts.join(", "))
    }
}

fn has_stage_timing_data(timings: &RowStageTimings) -> bool {
    timings.search_plan_ms.is_some()
        || timings.search_image_render_ms.is_some()
        || timings.primary_search_ms.is_some()
        || timings.fallback_search_ms.is_some()
        || timings.screening_ms.is_some()
        || timings.final_review_ms.is_some()
}

fn sidecar_search_url() -> String {
    std::env::var(SIDECAR_URL_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_SIDECAR_SEARCH_URL.to_string())
}

fn sidecar_health_url() -> String {
    std::env::var(SIDECAR_HEALTH_URL_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_SIDECAR_HEALTH_URL.to_string())
}

fn sidecar_session_url() -> String {
    std::env::var(SIDECAR_SESSION_URL_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_SIDECAR_SESSION_URL.to_string())
}

fn sidecar_shutdown_url() -> String {
    std::env::var(SIDECAR_SHUTDOWN_URL_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_SIDECAR_SHUTDOWN_URL.to_string())
}

fn has_mock_candidates() -> bool {
    [MOCK_CANDIDATES_ENV, MOCK_CANDIDATE_RESPONSES_ENV]
        .into_iter()
        .any(|key| {
            std::env::var(key)
                .ok()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
}

fn load_mock_candidate_sequence() -> Result<Option<VecDeque<Result<Vec<Candidate>, String>>>, String>
{
    let Ok(raw) = std::env::var(MOCK_CANDIDATE_RESPONSES_ENV) else {
        return Ok(None);
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let parsed = serde_json::from_str::<Vec<MockCandidateResponseEntry>>(trimmed)
        .map_err(|e| format!("parse {MOCK_CANDIDATE_RESPONSES_ENV} failed: {e}"))?;
    let queue = parsed
        .into_iter()
        .map(|entry| match entry {
            MockCandidateResponseEntry::Candidates(candidates) => Ok(candidates),
            MockCandidateResponseEntry::Error { err } => Err(err),
        })
        .collect();

    Ok(Some(queue))
}

fn build_runtime_vlm_client() -> Result<RuntimeVlmClient, String> {
    if let Some(mock) = MockVlmClient::from_env()? {
        return Ok(RuntimeVlmClient::Mock(mock));
    }

    DashScopeVlmClient::from_env().map(RuntimeVlmClient::DashScope)
}

fn load_mock_search_image_plan() -> Result<Option<SearchImagePlan>, String> {
    let Ok(raw) = std::env::var(MOCK_SEARCH_IMAGE_PLAN_ENV) else {
        return Ok(None);
    };

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    parse_search_image_plan(trimmed).map(Some)
}

fn resolve_search_image_plan(
    vlm_client: &RuntimeVlmClient,
    ozon_image_base64: &str,
    ozon_name: &str,
) -> Result<SearchImagePlan, String> {
    if let Some(plan) = load_mock_search_image_plan()? {
        return Ok(plan);
    }

    vlm_client.plan_search_images(ozon_image_base64, ozon_name)
}

fn build_mock_source_png() -> Result<Vec<u8>, String> {
    let image = RgbaImage::from_fn(320, 320, |x, y| {
        if x > 70 && x < 250 && y > 50 && y < 270 {
            Rgba([220, 40, 40, 255])
        } else {
            Rgba([245, 245, 245, 255])
        }
    });
    let dynamic = DynamicImage::ImageRgba8(image);
    let mut cursor = std::io::Cursor::new(Vec::new());
    dynamic
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("write mock source png failed: {e}"))?;
    Ok(cursor.into_inner())
}

fn write_source_image_or_mock_png(
    row: &TaskRow,
    temp_dir: &Path,
    use_mock_candidates: bool,
) -> Result<(PathBuf, String), String> {
    let image_path = if row.image_bytes.is_some() {
        temp_dir.join(format!(
            "{}-{}.jpg",
            row.excel_row_index,
            sanitize_filename(&row.sku)
        ))
    } else {
        temp_dir.join(format!(
            "{}-{}-source.png",
            row.excel_row_index,
            sanitize_filename(&row.sku)
        ))
    };

    let (bytes, mime_type) = if let Some(image_bytes) = &row.image_bytes {
        (image_bytes.clone(), "image/jpeg")
    } else if use_mock_candidates {
        (build_mock_source_png()?, "image/png")
    } else {
        return Err("source image missing".to_string());
    };

    std::fs::write(&image_path, &bytes).map_err(|e| format!("write source image failed: {e}"))?;
    Ok((
        image_path,
        format!("data:{mime_type};base64,{}", BASE64_STANDARD.encode(bytes)),
    ))
}

fn encode_image_file_as_data_url(image_path: &Path) -> Result<String, String> {
    let bytes =
        std::fs::read(image_path).map_err(|e| format!("read image file for base64 failed: {e}"))?;
    let mime_type = match image_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "application/octet-stream",
    };

    Ok(format!(
        "data:{mime_type};base64,{}",
        BASE64_STANDARD.encode(bytes)
    ))
}

fn ensure_sidecar_reachable(client: &Client) -> Result<(), String> {
    ping_sidecar(client).map_err(|e| {
        format!("sidecar service unavailable: {e}. 请先启动 sidecar（二进制 engine）后重试。")
    })
}

fn ping_sidecar(client: &Client) -> Result<(), String> {
    client
        .get(sidecar_health_url())
        .send()
        .map_err(|e| format!("{e}"))?;
    Ok(())
}

fn parse_sidecar_session_state(status: &str) -> Result<SidecarSessionState, String> {
    match status {
        "ready" => Ok(SidecarSessionState::Ready),
        "login_required" => Ok(SidecarSessionState::LoginRequired),
        "anti_bot_challenge" => Ok(SidecarSessionState::AntiBotChallenge),
        other => Err(format!("unexpected sidecar session status: {other}")),
    }
}

fn fetch_sidecar_session_state(client: &Client) -> Result<SidecarSessionState, String> {
    let response = client
        .get(sidecar_session_url())
        .send()
        .map_err(|e| format!("request sidecar session failed: {e}"))?;

    let status = response.status();
    let text = response
        .text()
        .map_err(|e| format!("read sidecar session response failed: {e}"))?;

    let parsed = serde_json::from_str::<SidecarSessionResponse>(&text)
        .map_err(|e| format!("parse sidecar session response failed: {e}; body={text}"))?;

    if !status.is_success() {
        return Err(parsed
            .code
            .or(parsed.error)
            .unwrap_or_else(|| format!("sidecar session http error {status}")));
    }

    if !parsed.success {
        return Err(parsed
            .code
            .or(parsed.error)
            .unwrap_or_else(|| "UNKNOWN_SIDECAR_SESSION_ERROR".to_string()));
    }

    parse_sidecar_session_state(
        parsed
            .status
            .as_deref()
            .ok_or_else(|| "sidecar session response missing status".to_string())?,
    )
}

fn wait_for_sidecar_ready_session_with_interval(
    sink: &mut dyn EventSink,
    client: &Client,
    poll_interval: Duration,
) -> Result<(), String> {
    let mut login_alert_emitted = false;

    loop {
        match fetch_sidecar_session_state(client) {
            Ok(SidecarSessionState::Ready) => return Ok(()),
            Ok(SidecarSessionState::LoginRequired) => {
                if !login_alert_emitted {
                    emit_blocking_alert_if_needed(sink, CODE_LOGIN_REQUIRED)?;
                    login_alert_emitted = true;
                }
                std::thread::sleep(poll_interval);
            }
            Ok(SidecarSessionState::AntiBotChallenge) => {
                GLOBAL_RECOVERY_GATE.pause();
                emit_blocking_alert_if_needed(sink, CODE_ANTI_BOT_CHALLENGE)?;
                return Err(CODE_ANTI_BOT_CHALLENGE.to_string());
            }
            Err(code) if code == CODE_CHROME_NOT_FOUND => {
                emit_blocking_alert_if_needed(sink, CODE_CHROME_NOT_FOUND)?;
                return Err(CODE_CHROME_NOT_FOUND.to_string());
            }
            Err(error) => return Err(error),
        }
    }
}

fn wait_for_sidecar_ready_session(sink: &mut dyn EventSink, client: &Client) -> Result<(), String> {
    wait_for_sidecar_ready_session_with_interval(
        sink,
        client,
        Duration::from_millis(SIDECAR_SESSION_POLL_INTERVAL_MILLIS),
    )
}

fn validate_task_runtime_prerequisites(
    task_rows: &[TaskRow],
    client: &Client,
    use_mock_candidates: bool,
) -> Result<(), String> {
    if use_mock_candidates {
        return Ok(());
    }

    let has_any_extractable_image = task_rows.iter().any(|row| row.image_bytes.is_some());
    if !has_any_extractable_image && !use_mock_candidates {
        return Err(
            "未提取到可搜索图片：请确认 Excel 中嵌入了商品图，并且图片引用不是失效状态。"
                .to_string(),
        );
    }

    ensure_sidecar_reachable(client)
}

fn fetch_candidates_from_sidecar(
    client: &Client,
    image_path: &Path,
    force_full_crop: bool,
    mock_sequence: &mut Option<VecDeque<Result<Vec<Candidate>, String>>>,
) -> Result<Vec<Candidate>, String> {
    if let Some(sequence) = mock_sequence.as_mut() {
        return sequence.pop_front().unwrap_or_else(|| Ok(Vec::new()));
    }

    if let Ok(mock_json) = std::env::var(MOCK_CANDIDATES_ENV) {
        let trimmed = mock_json.trim();
        if !trimmed.is_empty() {
            return serde_json::from_str::<Vec<Candidate>>(trimmed)
                .map_err(|e| format!("parse {MOCK_CANDIDATES_ENV} failed: {e}"));
        }
    }

    let request_body = SidecarSearchRequest {
        image_path: image_path.to_string_lossy().to_string(),
        force_full_crop,
    };

    let response = client
        .post(sidecar_search_url())
        .json(&request_body)
        .send()
        .map_err(|e| format!("request sidecar failed: {e}"))?;

    let status = response.status();
    let text = response
        .text()
        .map_err(|e| format!("read sidecar response failed: {e}"))?;

    let parsed = serde_json::from_str::<SidecarSearchResponse>(&text)
        .map_err(|e| format!("parse sidecar response failed: {e}; body={text}"))?;

    if !status.is_success() {
        return Err(parsed
            .code
            .or(parsed.error)
            .unwrap_or_else(|| format!("sidecar http error {status}")));
    }

    if parsed.success {
        return Ok(parsed.data.unwrap_or_default());
    }

    Err(parsed
        .code
        .or(parsed.error)
        .unwrap_or_else(|| "UNKNOWN_SIDECAR_ERROR".to_string()))
}

fn fetch_candidates_with_session_recovery(
    sink: &mut dyn EventSink,
    client: &Client,
    image_path: &Path,
    force_full_crop: bool,
    mock_sequence: &mut Option<VecDeque<Result<Vec<Candidate>, String>>>,
) -> Result<Vec<Candidate>, String> {
    loop {
        match fetch_candidates_from_sidecar(client, image_path, force_full_crop, mock_sequence) {
            Ok(candidates) => return Ok(candidates),
            Err(code) if code == CODE_LOGIN_REQUIRED => {
                wait_for_sidecar_ready_session(sink, client)?;
            }
            Err(code) => return Err(code),
        }
    }
}

fn write_result_workbook(
    result_path: &Path,
    headers: &[String],
    rows: &[TaskOutputRow],
) -> Result<(), String> {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    let header_format = Format::new().set_bold().set_background_color(Color::Silver);
    let base_col_len = headers.len() as u16;

    for (col_idx, header) in headers.iter().enumerate() {
        worksheet
            .write_string_with_format(0, col_idx as u16, header, &header_format)
            .map_err(|e| format!("write result header failed: {e}"))?;
    }
    worksheet
        .write_string_with_format(0, base_col_len, "1688成本价", &header_format)
        .map_err(|e| format!("write result header failed: {e}"))?;
    worksheet
        .write_string_with_format(0, base_col_len + 1, "1688链接", &header_format)
        .map_err(|e| format!("write result header failed: {e}"))?;
    worksheet
        .write_string_with_format(0, base_col_len + 2, "AI分析结论", &header_format)
        .map_err(|e| format!("write result header failed: {e}"))?;
    worksheet
        .write_string_with_format(0, base_col_len + 3, "图像比对耗时", &header_format)
        .map_err(|e| format!("write result header failed: {e}"))?;

    for (idx, row) in rows.iter().enumerate() {
        let write_row = (idx + 1) as u32;

        for (col_idx, value) in row.original_cells.iter().enumerate() {
            worksheet
                .write_string(write_row, col_idx as u16, value)
                .map_err(|e| format!("write result row failed: {e}"))?;
        }

        if let Some(price) = &row.price {
            worksheet
                .write_string(write_row, base_col_len, price)
                .map_err(|e| format!("write result row failed: {e}"))?;
        }
        if let Some(item_url) = &row.item_url {
            worksheet
                .write_string(write_row, base_col_len + 1, item_url)
                .map_err(|e| format!("write result row failed: {e}"))?;
        }
        worksheet
            .write_string(write_row, base_col_len + 2, &row.status)
            .map_err(|e| format!("write result row failed: {e}"))?;
        if let Some(compare_elapsed_text) = &row.compare_elapsed_text {
            worksheet
                .write_string(write_row, base_col_len + 3, compare_elapsed_text)
                .map_err(|e| format!("write result row failed: {e}"))?;
        }
    }

    workbook
        .save(result_path)
        .map_err(|e| format!("save result workbook failed: {e}"))
}

fn empty_output_row(row: &TaskRow, status: &str) -> TaskOutputRow {
    TaskOutputRow {
        row_index: row.excel_row_index,
        sku: row.sku.clone(),
        original_cells: row.original_cells.clone(),
        status: status.to_string(),
        compare_elapsed_text: None,
        price: None,
        item_url: None,
        image_url: None,
    }
}

fn output_row_from_match(
    row: &TaskRow,
    summary: MatchSummary,
    status: String,
    compare_elapsed_text: String,
) -> TaskOutputRow {
    match summary {
        MatchSummary::Cheapest(cheapest) => TaskOutputRow {
            row_index: row.excel_row_index,
            sku: row.sku.clone(),
            original_cells: row.original_cells.clone(),
            status,
            compare_elapsed_text: Some(compare_elapsed_text),
            price: Some(cheapest.price),
            item_url: Some(cheapest.item_url),
            image_url: Some(cheapest.image_url),
        },
        MatchSummary::MatchedButPriceUnavailable { .. } | MatchSummary::NoMatch => TaskOutputRow {
            row_index: row.excel_row_index,
            sku: row.sku.clone(),
            original_cells: row.original_cells.clone(),
            status,
            compare_elapsed_text: Some(compare_elapsed_text),
            price: None,
            item_url: None,
            image_url: None,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_row_event(
    sink: &mut dyn EventSink,
    row_index: u32,
    sku: &str,
    stage: &str,
    status: String,
    image_url: Option<String>,
    item_url: Option<String>,
    price: Option<String>,
    elapsed_text: Option<String>,
    is_final: bool,
) -> Result<(), String> {
    emit_event(
        sink,
        EVENT_ROW_RESULT,
        &RowResultEvent {
            row_index,
            sku: sku.to_string(),
            stage: stage.to_string(),
            status,
            image_url,
            item_url,
            price,
            elapsed_text,
            is_final,
        },
    )
}

fn emit_row_stage_event(
    sink: &mut dyn EventSink,
    row: &TaskRow,
    stage: &str,
    status: &str,
) -> Result<(), String> {
    emit_row_event(
        sink,
        row.excel_row_index,
        &row.sku,
        stage,
        status.to_string(),
        None,
        None,
        None,
        None,
        false,
    )
}

fn emit_final_row_result_event(
    sink: &mut dyn EventSink,
    row: &TaskOutputRow,
) -> Result<(), String> {
    emit_row_event(
        sink,
        row.row_index,
        &row.sku,
        "completed",
        row.status.clone(),
        row.image_url.clone(),
        row.item_url.clone(),
        row.price.clone(),
        row.compare_elapsed_text.clone(),
        true,
    )
}

fn no_match_status(reason: Option<&NoMatchReason>) -> &'static str {
    match reason {
        Some(NoMatchReason::NoCandidates) => "无可比对候选(双搜索图未召回有效1688结果)",
        Some(NoMatchReason::InitialScreenNoMatch) => "候选已召回，但AI初筛未判定为高相似候选",
        Some(NoMatchReason::FinalReviewRejected) => "候选已召回，但终选复核未通过",
        None => "无真实同款",
    }
}

pub fn run_task_with_original_source_and_sink(
    excel_path: &str,
    original_source_excel_path: Option<&str>,
    sink: &mut dyn EventSink,
) -> Result<RunTaskSummary, String> {
    let excel = validate_absolute_excel_path(excel_path)?;
    let output_anchor_path = resolve_output_anchor_path(&excel, original_source_excel_path);

    if GLOBAL_RECOVERY_GATE.is_paused() {
        emit_blocking_alert_if_needed(sink, CODE_RESUME_REQUIRED)?;
        return Err(CODE_RESUME_REQUIRED.to_string());
    }

    if let Some(code) = simulated_sidecar_error(&excel) {
        if code == CODE_ANTI_BOT_CHALLENGE {
            GLOBAL_RECOVERY_GATE.pause();
        }
        emit_blocking_alert_if_needed(sink, &code)?;
        return Err(code);
    }

    let temp_dir = std::env::temp_dir().join("desktop_app_temp_images");
    let result_path = output_anchor_path
        .parent()
        .map(|p| p.join("result.xlsx"))
        .unwrap_or_else(|| PathBuf::from("result.xlsx"));
    let task_workbook = load_task_rows(&excel)?;
    let total_rows = task_workbook.rows.len() as u32;
    let client = Client::new();
    let use_mock_candidates = has_mock_candidates();
    validate_task_runtime_prerequisites(&task_workbook.rows, &client, use_mock_candidates)?;
    if !use_mock_candidates {
        wait_for_sidecar_ready_session(sink, &client)?;
    }
    let vlm_client = build_runtime_vlm_client()?;
    let mut mock_candidate_sequence = load_mock_candidate_sequence()?;

    run_with_task_guard(temp_dir.clone(), || {
        let diagnostics_session = match TaskDiagnosticsSession::new(&output_anchor_path) {
            Ok(session) => Some(session),
            Err(error) => {
                emit_event(
                    sink,
                    EVENT_LOG,
                    &LogEvent {
                        level: "warn".to_string(),
                        message: format!("初始化诊断目录失败: {error}"),
                    },
                )?;
                None
            }
        };

        emit_event(
            sink,
            EVENT_LOG,
            &LogEvent {
                level: "info".to_string(),
                message: format!("task started for {}", excel.display()),
            },
        )?;

        emit_event(
            sink,
            EVENT_PROGRESS,
            &ProgressEvent {
                processed: 0,
                total: total_rows,
            },
        )?;

        let mut processed_rows = 0u32;
        let mut output_rows = Vec::new();
        let mut diagnostics_handles = Vec::new();

        for row in &task_workbook.rows {
            processed_rows += 1;
            let mut output_row = empty_output_row(row, "Excel中无图");
            let mut pending_diagnostics: Option<PendingDiagnosticsJob> = None;
            let mut row_stage_timings = RowStageTimings::default();
            emit_row_stage_event(sink, row, "queued", "排队中")?;

            if row.image_bytes.is_some() || use_mock_candidates {
                let compare_started_at = Instant::now();
                emit_row_stage_event(sink, row, "planning_search_image", "正在生成搜索图")?;
                emit_event(
                    sink,
                    EVENT_LOG,
                    &LogEvent {
                        level: "info".to_string(),
                        message: "正在生成搜索图".to_string(),
                    },
                )?;
                match write_source_image_or_mock_png(row, &temp_dir, use_mock_candidates) {
                    Ok((source_image_path, ozon_base64)) => {
                        let search_plan_started_at = Instant::now();
                        match resolve_search_image_plan(&vlm_client, &ozon_base64, &row.ozon_name) {
                            Ok(search_plan) => {
                                row_stage_timings.search_plan_ms =
                                    Some(elapsed_millis(search_plan_started_at.elapsed()));
                                let render_search_images_started_at = Instant::now();
                                match generate_search_images(
                                    &source_image_path,
                                    &search_plan,
                                    &temp_dir,
                                    &row.sku,
                                ) {
                                    Ok(search_images) => {
                                        row_stage_timings.search_image_render_ms =
                                            Some(elapsed_millis(
                                                render_search_images_started_at.elapsed(),
                                            ));
                                        match (
                                            encode_image_file_as_data_url(
                                                &search_images.primary_path,
                                            ),
                                            encode_image_file_as_data_url(
                                                &search_images.fallback_path,
                                            ),
                                        ) {
                                            (
                                                Ok(primary_search_base64),
                                                Ok(fallback_search_base64),
                                            ) => {
                                                let fetcher = SidecarCandidateFetcher::new(
                                                    sink,
                                                    &client,
                                                    mock_candidate_sequence.take(),
                                                    row.excel_row_index,
                                                    row.sku.clone(),
                                                );
                                                let result = orchestrate_match(
                                                    &fetcher,
                                                    &vlm_client,
                                                    SearchPass {
                                                        image_path: &search_images.primary_path,
                                                        reference_image_base64:
                                                            &primary_search_base64,
                                                    },
                                                    SearchPass {
                                                        image_path: &search_images.fallback_path,
                                                        reference_image_base64:
                                                            &fallback_search_base64,
                                                    },
                                                    &ozon_base64,
                                                    Some(&row.ozon_name),
                                                );
                                                let search_timings = fetcher.timings();
                                                row_stage_timings.primary_search_ms =
                                                    search_timings.primary_search_ms;
                                                row_stage_timings.fallback_search_ms =
                                                    search_timings.fallback_search_ms;
                                                mock_candidate_sequence =
                                                    fetcher.into_mock_sequence();

                                                match result {
                                                    Ok(orchestration) => {
                                                        let elapsed =
                                                            format_elapsed_text(elapsed_millis(
                                                                compare_started_at.elapsed(),
                                                            ));
                                                        let summary = orchestration.summary.clone();
                                                        let no_match_reason =
                                                            orchestration.no_match_reason.clone();
                                                        let used_fallback_image =
                                                            orchestration.used_fallback_image;
                                                        row_stage_timings.screening_ms = Some(
                                                            orchestration
                                                                .diagnostics
                                                                .screening_elapsed_ms,
                                                        );
                                                        row_stage_timings
                                                            .screening_candidate_count =
                                                            orchestration
                                                                .diagnostics
                                                                .screening_candidate_count;
                                                        row_stage_timings.screening_chunk_count =
                                                            orchestration
                                                                .diagnostics
                                                                .screening_chunk_count;
                                                        if orchestration
                                                            .diagnostics
                                                            .final_review_batch_count
                                                            > 0
                                                        {
                                                            row_stage_timings.final_review_ms =
                                                                Some(
                                                                    orchestration
                                                                        .diagnostics
                                                                        .final_review_elapsed_ms,
                                                                );
                                                        }
                                                        row_stage_timings
                                                            .final_review_candidate_count =
                                                            orchestration
                                                                .diagnostics
                                                                .final_review_candidate_count;
                                                        row_stage_timings
                                                            .final_review_batch_count =
                                                            orchestration
                                                                .diagnostics
                                                                .final_review_batch_count;
                                                        output_row = match summary.clone() {
                                                            MatchSummary::Cheapest(cheapest) => {
                                                                let status = if used_fallback_image {
                                                                    "AI比对成功(备用搜索图召回)"
                                                                } else {
                                                                    "AI比对成功(主搜索图召回)"
                                                                };
                                                                output_row_from_match(
                                                                    row,
                                                                    MatchSummary::Cheapest(
                                                                        cheapest,
                                                                    ),
                                                                    status.to_string(),
                                                                    elapsed,
                                                                )
                                                            }
                                                            MatchSummary::NoMatch => {
                                                                output_row_from_match(
                                                                    row,
                                                                    MatchSummary::NoMatch,
                                                                    no_match_status(
                                                                        no_match_reason.as_ref(),
                                                                    )
                                                                    .to_string(),
                                                                    elapsed,
                                                                )
                                                            }
                                                            MatchSummary::MatchedButPriceUnavailable {
                                                                total_matches,
                                                            } => output_row_from_match(
                                                                row,
                                                                MatchSummary::MatchedButPriceUnavailable {
                                                                    total_matches,
                                                                },
                                                                "命中同款但价格不可解析"
                                                                    .to_string(),
                                                                elapsed,
                                                            ),
                                                        };

                                                        if should_write_diagnostics(
                                                            &summary,
                                                            no_match_reason.as_ref(),
                                                        ) {
                                                            pending_diagnostics =
                                                                Some(PendingDiagnosticsJob {
                                                                    row: row.clone(),
                                                                    status: output_row
                                                                        .status
                                                                        .clone(),
                                                                    search_plan: search_plan
                                                                        .clone(),
                                                                    search_images: search_images
                                                                        .clone(),
                                                                    source_image_path:
                                                                        source_image_path.clone(),
                                                                    diagnostics: orchestration
                                                                        .diagnostics
                                                                        .clone(),
                                                                    used_fallback_image,
                                                                    no_match_reason:
                                                                        no_match_reason.clone(),
                                                                    row_stage_timings:
                                                                        row_stage_timings.clone(),
                                                                });
                                                        }
                                                    }
                                                    Err(code) => {
                                                        if code == CODE_ANTI_BOT_CHALLENGE {
                                                            GLOBAL_RECOVERY_GATE.pause();
                                                            emit_blocking_alert_if_needed(
                                                                sink, &code,
                                                            )?;
                                                            return Err(code);
                                                        }
                                                        if code == CODE_CHROME_NOT_FOUND {
                                                            emit_blocking_alert_if_needed(
                                                                sink,
                                                                CODE_CHROME_NOT_FOUND,
                                                            )?;
                                                            return Err(
                                                                CODE_CHROME_NOT_FOUND.to_string()
                                                            );
                                                        }

                                                        let status = if code.contains("大模型API")
                                                        {
                                                            format!("大模型API异常: {code}")
                                                        } else if code.contains("sidecar")
                                                            || code.contains("LOGIN_REQUIRED")
                                                            || code.contains("ANTI_BOT")
                                                        {
                                                            "Node爬虫获取失败".to_string()
                                                        } else {
                                                            format!("sidecar_error({code})")
                                                        };
                                                        output_row = output_row_from_match(
                                                            row,
                                                            MatchSummary::NoMatch,
                                                            status,
                                                            format_elapsed_text(elapsed_millis(
                                                                compare_started_at.elapsed(),
                                                            )),
                                                        );
                                                    }
                                                }
                                            }
                                            (Err(error), _) | (_, Err(error)) => {
                                                output_row = output_row_from_match(
                                                    row,
                                                    MatchSummary::NoMatch,
                                                    format!("搜索图编码失败: {error}"),
                                                    format_elapsed_text(elapsed_millis(
                                                        compare_started_at.elapsed(),
                                                    )),
                                                );
                                            }
                                        }
                                    }
                                    Err(error) => {
                                        row_stage_timings.search_image_render_ms =
                                            Some(elapsed_millis(
                                                render_search_images_started_at.elapsed(),
                                            ));
                                        output_row = output_row_from_match(
                                            row,
                                            MatchSummary::NoMatch,
                                            format!("搜索图生成失败: {error}"),
                                            format_elapsed_text(elapsed_millis(
                                                compare_started_at.elapsed(),
                                            )),
                                        );
                                    }
                                }
                            }
                            Err(error) => {
                                row_stage_timings.search_plan_ms =
                                    Some(elapsed_millis(search_plan_started_at.elapsed()));
                                output_row = output_row_from_match(
                                    row,
                                    MatchSummary::NoMatch,
                                    format!("搜索图生成失败: {error}"),
                                    format_elapsed_text(elapsed_millis(
                                        compare_started_at.elapsed(),
                                    )),
                                );
                            }
                        }
                    }
                    Err(error) => {
                        output_row = output_row_from_match(
                            row,
                            MatchSummary::NoMatch,
                            format!("搜索图生成失败: {error}"),
                            format_elapsed_text(elapsed_millis(compare_started_at.elapsed())),
                        );
                    }
                }
            }

            emit_final_row_result_event(sink, &output_row)?;
            if has_stage_timing_data(&row_stage_timings) {
                emit_event(
                    sink,
                    EVENT_LOG,
                    &LogEvent {
                        level: "info".to_string(),
                        message: format_stage_timing_summary(&row.sku, &row_stage_timings),
                    },
                )?;
            }

            output_rows.push(output_row);

            emit_event(
                sink,
                EVENT_PROGRESS,
                &ProgressEvent {
                    processed: processed_rows,
                    total: total_rows,
                },
            )?;

            if let (Some(session), Some(job)) = (diagnostics_session.clone(), pending_diagnostics) {
                diagnostics_handles.push(std::thread::spawn(move || {
                    let diagnostics_started_at = Instant::now();
                    let result = session.save_row_bundle(
                        &job.row,
                        &job.status,
                        &job.search_plan,
                        &job.search_images,
                        &job.source_image_path,
                        &job.diagnostics,
                        job.used_fallback_image,
                        job.no_match_reason.as_ref(),
                        &job.row_stage_timings,
                    );
                    (result, elapsed_millis(diagnostics_started_at.elapsed()))
                }));
            }
        }

        write_result_workbook(&result_path, &task_workbook.headers, &output_rows)?;

        for handle in diagnostics_handles {
            match handle.join() {
                Ok((Ok(path), diagnostics_elapsed_ms)) => {
                    emit_event(
                        sink,
                        EVENT_LOG,
                        &LogEvent {
                            level: "info".to_string(),
                            message: format!(
                                "诊断产物已写出: {} ({})",
                                path.display(),
                                format_elapsed_text(diagnostics_elapsed_ms)
                            ),
                        },
                    )?;
                }
                Ok((Err(error), diagnostics_elapsed_ms)) => {
                    emit_event(
                        sink,
                        EVENT_LOG,
                        &LogEvent {
                            level: "warn".to_string(),
                            message: format!(
                                "写出诊断产物失败: {} ({})",
                                error,
                                format_elapsed_text(diagnostics_elapsed_ms)
                            ),
                        },
                    )?;
                }
                Err(_) => {
                    emit_event(
                        sink,
                        EVENT_LOG,
                        &LogEvent {
                            level: "warn".to_string(),
                            message: "写出诊断产物失败: 后台任务异常退出".to_string(),
                        },
                    )?;
                }
            }
        }

        let summary = RunTaskSummary {
            excel_path: output_anchor_path.to_string_lossy().to_string(),
            processed_rows,
            total_rows,
            status: "completed".to_string(),
            result_path: Some(result_path.to_string_lossy().to_string()),
        };

        emit_event(
            sink,
            EVENT_TASK_DONE,
            &TaskDoneEvent {
                excel_path: summary.excel_path.clone(),
                status: summary.status.clone(),
                processed_rows: summary.processed_rows,
                total_rows: summary.total_rows,
                result_path: summary.result_path.clone(),
            },
        )?;

        Ok(summary)
    })
}

pub fn run_task_with_sink(
    excel_path: &str,
    sink: &mut dyn EventSink,
) -> Result<RunTaskSummary, String> {
    run_task_with_original_source_and_sink(excel_path, None, sink)
}

fn resolve_runtime_dashscope_api_key(window: &tauri::Window) -> Result<String, String> {
    let maybe_settings = load_runtime_settings(window);

    resolve_effective_dashscope_api_key(maybe_settings.as_ref())
}

fn load_runtime_settings(window: &tauri::Window) -> Option<AppSettings> {
    window
        .app_handle()
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| settings_file_path(&dir))
        .and_then(|path| load_settings_from_disk(&path).ok())
}

pub fn sidecar_runtime_dir_for_base(base_dir: &Path) -> PathBuf {
    base_dir.join("sidecar-runtime")
}

pub fn choose_sidecar_profile_base_dir(
    local_data_dir: Option<&Path>,
    cache_dir: Option<&Path>,
    fallback_dir: &Path,
) -> PathBuf {
    local_data_dir
        .map(Path::to_path_buf)
        .or_else(|| cache_dir.map(Path::to_path_buf))
        .unwrap_or_else(|| fallback_dir.to_path_buf())
}

pub fn sidecar_profile_dir_for_base(base_dir: &Path) -> PathBuf {
    base_dir.join("sidecar-profile").join("1688_profile")
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|e| format!("create sidecar profile dir failed: {e}"))?;

    for entry in std::fs::read_dir(source)
        .map_err(|e| format!("read legacy sidecar profile dir failed: {e}"))?
    {
        let entry = entry.map_err(|e| format!("read legacy sidecar profile entry failed: {e}"))?;
        let entry_path = entry.path();
        let target_path = target.join(entry.file_name());
        if entry_path.is_dir() {
            copy_dir_recursive(&entry_path, &target_path)?;
        } else {
            std::fs::copy(&entry_path, &target_path)
                .map_err(|e| format!("copy legacy sidecar profile file failed: {e}"))?;
        }
    }

    Ok(())
}

pub fn shutdown_managed_sidecar() {
    if let Ok(client) = Client::builder().timeout(Duration::from_secs(2)).build() {
        let _ = client.post(sidecar_shutdown_url()).send();
    }

    let mut child_process = {
        let Ok(mut guard) = SIDECAR_CHILD.lock() else {
            return;
        };
        guard.take()
    };

    let Some(mut child_process) = child_process.take() else {
        return;
    };

    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        match child_process.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => break,
        }
    }

    let _ = child_process.kill();
    let _ = child_process.wait();
}

fn resolve_sidecar_runtime_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let base_dir = app
        .path()
        .app_cache_dir()
        .or_else(|_| app.path().app_local_data_dir())
        .unwrap_or_else(|_| std::env::temp_dir().join("desktop_app"));
    let runtime_dir = sidecar_runtime_dir_for_base(&base_dir);

    std::fs::create_dir_all(&runtime_dir)
        .map_err(|e| format!("create sidecar runtime dir failed: {e}"))?;

    Ok(runtime_dir)
}

fn resolve_sidecar_profile_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let local_data_dir = app.path().app_local_data_dir().ok();
    let cache_dir = app.path().app_cache_dir().ok();
    let fallback_dir = std::env::temp_dir().join("desktop_app");
    let preferred_base_dir =
        choose_sidecar_profile_base_dir(local_data_dir.as_deref(), cache_dir.as_deref(), &fallback_dir);
    let profile_dir = sidecar_profile_dir_for_base(&preferred_base_dir);

    if let Some(parent) = profile_dir.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create sidecar profile parent dir failed: {e}"))?;
    }

    if !profile_dir.exists() {
        if let Some(cache_dir) = cache_dir.as_deref() {
            let legacy_profile_dir = sidecar_runtime_dir_for_base(cache_dir).join("1688_profile");
            if legacy_profile_dir.exists() && legacy_profile_dir != profile_dir {
                if std::fs::rename(&legacy_profile_dir, &profile_dir).is_err() {
                    copy_dir_recursive(&legacy_profile_dir, &profile_dir)?;
                }
            }
        }
    }

    std::fs::create_dir_all(&profile_dir)
        .map_err(|e| format!("create sidecar profile dir failed: {e}"))?;

    Ok(profile_dir)
}

fn find_sidecar_binary_in_dir(dir: &Path) -> Option<PathBuf> {
    if !dir.exists() || !dir.is_dir() {
        return None;
    }

    let target = option_env!("TARGET").unwrap_or_default();
    let mut preferred_names = vec![
        "engine".to_string(),
        format!("engine-{target}"),
        format!("engine-{target}.exe"),
    ];
    if cfg!(target_os = "windows") {
        preferred_names.insert(0, "engine.exe".to_string());
    }

    for name in preferred_names {
        let candidate = dir.join(name);
        if candidate.exists() && candidate.is_file() {
            return Some(candidate);
        }
    }

    let mut discovered = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file_name = path
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if file_name.starts_with("engine") {
                discovered.push(path);
            }
        }
    }

    discovered.sort();
    discovered.into_iter().next()
}

fn resolve_sidecar_executable(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var(SIDECAR_EXECUTABLE_PATH_ENV) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if candidate.exists() && candidate.is_file() {
                return Ok(candidate);
            }
            return Err(format!(
                "{SIDECAR_EXECUTABLE_PATH_ENV} 指向的文件不存在: {}",
                candidate.display()
            ));
        }
    }

    let mut dirs = Vec::<PathBuf>::new();
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries"));

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            dirs.push(parent.to_path_buf());
            dirs.push(parent.join("binaries"));
            dirs.push(parent.join("../binaries"));
        }
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        dirs.push(resource_dir.clone());
        dirs.push(resource_dir.join("binaries"));
    }

    for dir in dirs {
        if let Some(binary) = find_sidecar_binary_in_dir(&dir) {
            return Ok(binary);
        }
    }

    Err(
        "未找到 sidecar 二进制 engine。请确认 `src-tauri/binaries/` 下存在对应平台的 engine 文件。"
            .to_string(),
    )
}

fn wait_for_sidecar_ready(client: &Client, timeout: Duration) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if ping_sidecar(client).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Err("自动启动 sidecar 超时，请检查 engine 二进制是否可执行。".to_string())
}

fn ensure_sidecar_running(
    app: &tauri::AppHandle,
    settings: Option<&AppSettings>,
    api_key: &str,
    client: &Client,
) -> Result<(), String> {
    if ping_sidecar(client).is_ok() {
        return Ok(());
    }

    let mut guard = SIDECAR_CHILD
        .lock()
        .map_err(|_| "sidecar process lock poisoned".to_string())?;

    if let Some(child) = guard.as_mut() {
        match child.try_wait() {
            Ok(Some(_status)) => {
                *guard = None;
            }
            Ok(None) => {
                drop(guard);
                return wait_for_sidecar_ready(
                    client,
                    Duration::from_secs(SIDECAR_WAIT_TIMEOUT_SECS),
                );
            }
            Err(_e) => {
                *guard = None;
            }
        }
    }

    let binary = resolve_sidecar_executable(app)?;
    let runtime_dir = resolve_sidecar_runtime_dir(app)?;
    let profile_dir = resolve_sidecar_profile_dir(app)?;
    let mut cmd = Command::new(&binary);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(&runtime_dir);

    if let Some(settings) = settings {
        for (k, v) in build_sidecar_env(settings) {
            if !v.trim().is_empty() {
                cmd.env(k, v);
            }
        }
    }
    cmd.env("DASHSCOPE_API_KEY", api_key);
    cmd.env(SIDECAR_PROFILE_DIR_ENV, &profile_dir);

    let child = cmd
        .spawn()
        .map_err(|e| format!("自动启动 sidecar 失败（{}）: {e}", binary.display()))?;
    *guard = Some(child);
    drop(guard);

    wait_for_sidecar_ready(client, Duration::from_secs(SIDECAR_WAIT_TIMEOUT_SECS))
}

async fn run_blocking_task<F, T>(task: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|e| format!("blocking task join failed: {e}"))?
}

#[tauri::command]
pub async fn run_task(
    window: tauri::Window,
    excel_path: String,
    source_excel_path: Option<String>,
) -> Result<RunTaskSummary, String> {
    let settings = load_runtime_settings(&window);
    let api_key = resolve_runtime_dashscope_api_key(&window)?;
    let window_for_worker = window.clone();
    let app_handle = window.app_handle().clone();

    run_blocking_task(move || {
        std::env::set_var("DASHSCOPE_API_KEY", &api_key);

        let client = Client::new();
        ensure_sidecar_running(&app_handle, settings.as_ref(), &api_key, &client)?;

        let mut sink = TauriWindowSink::new(&window_for_worker);
        run_task_with_original_source_and_sink(&excel_path, source_excel_path.as_deref(), &mut sink)
    })
    .await
}

#[tauri::command]
pub fn resume_after_challenge(window: tauri::Window) -> Result<(), String> {
    GLOBAL_RECOVERY_GATE.resume();
    let mut sink = TauriWindowSink::new(&window);
    emit_event(
        &mut sink,
        EVENT_LOG,
        &LogEvent {
            level: "info".to_string(),
            message: "manual gate cleared, task queue resumed".to_string(),
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use crate::events::EVENT_BLOCKING_ALERT;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[derive(Default)]
    struct CollectingSink {
        events: Vec<String>,
    }

    impl EventSink for CollectingSink {
        fn emit_json(&mut self, event: &str, _payload: serde_json::Value) -> Result<(), String> {
            self.events.push(event.to_string());
            Ok(())
        }
    }

    fn spawn_mock_session_server(bodies: Vec<&'static str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock session server");
        let address = format!("http://{}/session-state", listener.local_addr().unwrap());

        std::thread::spawn(move || {
            for body in bodies {
                let (mut stream, _) = listener.accept().expect("accept mock request");
                let mut buffer = [0_u8; 1024];
                let _ = stream.read(&mut buffer);

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write mock response");
            }
        });

        address
    }

    fn spawn_mock_health_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock health server");
        let address = format!("http://{}/health", listener.local_addr().unwrap());

        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept mock health request");
            let mut buffer = [0_u8; 1024];
            let size = stream.read(&mut buffer).expect("read mock health request");
            let request = String::from_utf8_lossy(&buffer[..size]);

            if request.starts_with("GET /health ") {
                let body = r#"{"success":true}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write mock health response");
            }
        });

        address
    }

    #[test]
    fn wait_for_sidecar_ready_session_emits_login_alert_once_then_continues() {
        let _guard = ENV_LOCK.lock().expect("env lock should acquire");
        let session_url = spawn_mock_session_server(vec![
            r#"{"success":true,"status":"login_required"}"#,
            r#"{"success":true,"status":"login_required"}"#,
            r#"{"success":true,"status":"ready"}"#,
        ]);

        std::env::set_var(SIDECAR_SESSION_URL_ENV, &session_url);

        let client = Client::new();
        let mut sink = CollectingSink::default();
        let result = wait_for_sidecar_ready_session_with_interval(
            &mut sink,
            &client,
            Duration::from_millis(5),
        );

        std::env::remove_var(SIDECAR_SESSION_URL_ENV);

        result.expect("session wait should recover after login");
        assert_eq!(
            sink.events
                .iter()
                .filter(|event| event.as_str() == EVENT_BLOCKING_ALERT)
                .count(),
            1,
            "login-required auto-wait should emit a single blocking alert"
        );
    }

    #[test]
    fn ping_sidecar_uses_health_endpoint() {
        let _guard = ENV_LOCK.lock().expect("env lock should acquire");
        let health_url = spawn_mock_health_server();
        std::env::set_var(SIDECAR_HEALTH_URL_ENV, &health_url);

        let client = Client::new();
        let result = ping_sidecar(&client);

        std::env::remove_var(SIDECAR_HEALTH_URL_ENV);

        result.expect("ping should succeed against /health endpoint");
    }

    #[test]
    fn run_blocking_task_offloads_work_to_background_thread() {
        let caller_thread = thread::current().id();
        let worker_thread = Arc::new(Mutex::new(None));
        let worker_thread_for_task = worker_thread.clone();

        let result = tauri::async_runtime::block_on(run_blocking_task(move || {
            *worker_thread_for_task.lock().expect("worker thread lock") =
                Some(thread::current().id());
            Ok::<_, String>(42)
        }))
        .expect("blocking task should succeed");

        assert_eq!(result, 42);
        let observed = worker_thread
            .lock()
            .expect("worker thread lock")
            .expect("worker thread should be recorded");
        assert_ne!(caller_thread, observed);
    }
}
