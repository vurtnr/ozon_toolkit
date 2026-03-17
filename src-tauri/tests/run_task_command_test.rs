use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use calamine::{open_workbook, Reader, Xlsx};
use desktop_app_lib::commands::run_task::{
    build_match_hint, choose_sidecar_profile_base_dir, run_task_with_original_source_and_sink,
    run_task_with_sink, shutdown_managed_sidecar, sidecar_profile_dir_for_base,
    sidecar_runtime_dir_for_base,
    RunTaskSummary,
};
use desktop_app_lib::events::{
    EventSink, EVENT_LOG, EVENT_PROGRESS, EVENT_ROW_RESULT, EVENT_TASK_DONE,
};
use desktop_app_lib::recovery::GLOBAL_RECOVERY_GATE;
use rust_xlsxwriter::Workbook;
use serde_json::Value;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default)]
struct CollectingSink {
    events: Vec<String>,
    payloads: Vec<(String, Value)>,
}

impl EventSink for CollectingSink {
    fn emit_json(&mut self, event: &str, payload: serde_json::Value) -> Result<(), String> {
        self.events.push(event.to_string());
        self.payloads.push((event.to_string(), payload));
        Ok(())
    }
}

fn log_messages(sink: &CollectingSink) -> Vec<String> {
    sink.payloads
        .iter()
        .filter(|(name, _)| name == EVENT_LOG)
        .filter_map(|(_, payload)| {
            payload
                .get("message")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
        })
        .collect()
}

fn row_event_payloads<'a>(sink: &'a CollectingSink) -> Vec<&'a Value> {
    sink.payloads
        .iter()
        .filter(|(name, _)| name == EVENT_ROW_RESULT)
        .map(|(_, payload)| payload)
        .collect()
}

fn final_row_event_payloads<'a>(sink: &'a CollectingSink) -> Vec<&'a Value> {
    row_event_payloads(sink)
        .into_iter()
        .filter(|payload| payload["is_final"].as_bool() == Some(true))
        .collect()
}

fn make_temp_excel_path() -> PathBuf {
    let unique = format!(
        "run-task-test-{}-{}.xlsx",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

fn create_sample_workbook(path: &PathBuf) {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();

    worksheet.write_string(0, 0, "title").expect("write header");
    worksheet.write_string(0, 1, "sku").expect("write header");
    worksheet
        .write_string(1, 0, "sample-1")
        .expect("write row 1 title");
    worksheet
        .write_string(1, 1, "SKU-001")
        .expect("write row 1 sku");
    worksheet
        .write_string(2, 0, "sample-2")
        .expect("write row 2 title");
    worksheet
        .write_string(2, 1, "SKU-002")
        .expect("write row 2 sku");

    workbook.save(path).expect("save workbook");
}

fn create_single_row_workbook(path: &PathBuf) {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();

    worksheet.write_string(0, 0, "title").expect("write header");
    worksheet.write_string(0, 1, "sku").expect("write header");
    worksheet
        .write_string(1, 0, "sample-1")
        .expect("write row 1 title");
    worksheet
        .write_string(1, 1, "SKU-001")
        .expect("write row 1 sku");

    workbook.save(path).expect("save workbook");
}

#[test]
fn build_match_hint_prefers_planner_target_product_without_losing_original_title() {
    assert_eq!(
        build_match_hint(
            "Аксессуары и комплектующие для судов",
            "船用绳梯（带金属挂钩和红色踏步带）",
        ),
        "船用绳梯（带金属挂钩和红色踏步带）；原始标题：Аксессуары и комплектующие для судов"
    );
    assert_eq!(build_match_hint("sample title", "sample title"), "sample title");
    assert_eq!(build_match_hint("sample title", ""), "sample title");
}

fn remove_if_exists(path: &PathBuf) {
    let _ = std::fs::remove_file(path);
}

fn remove_dir_if_exists(path: &PathBuf) {
    let _ = std::fs::remove_dir_all(path);
}

fn set_mock_pipeline_env(candidate_responses_json: &str, vlm_replies_json: &str) {
    std::env::set_var(
        "RUN_TASK_MOCK_CANDIDATE_RESPONSES_JSON",
        candidate_responses_json,
    );
    std::env::set_var("RUN_TASK_MOCK_VLM_REPLIES_JSON", vlm_replies_json);
}

fn set_mock_search_image_plan_env(plan_json: &str) {
    std::env::set_var("RUN_TASK_MOCK_SEARCH_IMAGE_PLAN_JSON", plan_json);
}

fn clear_mock_pipeline_env() {
    std::env::remove_var("RUN_TASK_MOCK_CANDIDATE_RESPONSES_JSON");
    std::env::remove_var("RUN_TASK_MOCK_VLM_REPLIES_JSON");
    std::env::remove_var("RUN_TASK_MOCK_CANDIDATES_JSON");
    std::env::remove_var("RUN_TASK_MOCK_SEARCH_IMAGE_PLAN_JSON");
}

fn default_mock_search_image_plan_json() -> &'static str {
    r#"{
      "target_product":"bag",
      "scene_type":"single_product",
      "primary_bbox":{"x":0.18,"y":0.12,"width":0.56,"height":0.68},
      "fallback_bbox":{"x":0.10,"y":0.06,"width":0.74,"height":0.82},
      "background_strategy":"remove_and_whitefill",
      "subject_confidence":0.92,
      "needs_fallback_context":true
    }"#
}

#[test]
fn run_task_accepts_absolute_excel_path_and_emits_all_events() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    let excel_path = make_temp_excel_path();
    create_sample_workbook(&excel_path);
    set_mock_pipeline_env(
        r#"[
          [{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"https://img.1688.com/1.jpg"}],
          [{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"https://img.1688.com/1.jpg"}]
        ]"#,
        r#"[
          [1],
          [1],
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary: RunTaskSummary =
        run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
            .expect("run task should succeed");

    assert_eq!(summary.status, "completed");
    assert_eq!(summary.excel_path, excel_path.to_string_lossy());
    assert_eq!(summary.total_rows, 2);
    assert_eq!(summary.processed_rows, 2);
    assert!(
        summary.result_path.is_some(),
        "result.xlsx path should be returned"
    );
    assert!(sink.events.contains(&EVENT_LOG.to_string()));
    assert!(sink.events.contains(&EVENT_PROGRESS.to_string()));
    assert!(sink.events.contains(&EVENT_ROW_RESULT.to_string()));
    assert!(sink.events.contains(&EVENT_TASK_DONE.to_string()));

    let row_events = row_event_payloads(&sink);
    let final_row_events = final_row_event_payloads(&sink);
    assert!(row_events.len() > 2, "staged row updates should be emitted");
    assert_eq!(row_events[0]["stage"], "queued");
    assert_eq!(row_events[1]["stage"], "planning_search_image");
    assert_eq!(final_row_events.len(), 2);
    assert_eq!(final_row_events[0]["sku"], "SKU-001");
    assert_eq!(final_row_events[1]["sku"], "SKU-002");
    assert_eq!(final_row_events[0]["status"], "AI比对成功(主搜索图召回)");
    assert_eq!(final_row_events[0]["price"], "¥12.34");
    assert_eq!(
        final_row_events[0]["item_url"],
        "https://detail.1688.com/offer/1.html"
    );

    let progress_events: Vec<&Value> = sink
        .payloads
        .iter()
        .filter(|(name, _)| name == EVENT_PROGRESS)
        .map(|(_, payload)| payload)
        .collect();
    assert_eq!(progress_events.len(), 3);
    assert_eq!(progress_events[0]["processed"], 0);
    assert_eq!(progress_events[0]["total"], 2);
    assert_eq!(progress_events[2]["processed"], 2);

    let logs = log_messages(&sink);
    assert!(logs.iter().any(|line| line.contains("正在生成搜索图")));
    assert!(logs.iter().any(|line| line.contains("主搜索图搜索中")));

    clear_mock_pipeline_env();
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_rejects_relative_paths() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    let mut sink = CollectingSink::default();
    let err =
        run_task_with_sink("./1.xlsx", &mut sink).expect_err("relative paths should be rejected");

    assert!(err.contains("absolute"));
}

#[test]
fn run_task_rejects_workbook_without_extractable_images() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();

    let excel_path = make_temp_excel_path();
    create_sample_workbook(&excel_path);

    let mut sink = CollectingSink::default();
    let err = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect_err("workbook without images should be rejected");

    assert!(err.contains("未提取到可搜索图片"));

    remove_if_exists(&excel_path);
}

#[test]
fn sidecar_runtime_dir_lives_outside_watched_source_tree() {
    let runtime_dir = sidecar_runtime_dir_for_base(&PathBuf::from("/tmp/desktop-app-cache"));

    assert_eq!(
        runtime_dir,
        PathBuf::from("/tmp/desktop-app-cache/sidecar-runtime")
    );
    assert!(
        !runtime_dir.to_string_lossy().contains("src-tauri"),
        "runtime dir must not point into the watched source tree"
    );
}

#[test]
fn sidecar_profile_base_prefers_local_data_over_cache() {
    let local_data = PathBuf::from("/tmp/desktop-app-local");
    let cache = PathBuf::from("/tmp/desktop-app-cache");
    let fallback = PathBuf::from("/tmp/desktop-app-fallback");

    assert_eq!(
        choose_sidecar_profile_base_dir(Some(&local_data), Some(&cache), &fallback),
        local_data
    );
    assert_eq!(
        choose_sidecar_profile_base_dir(None, Some(&cache), &fallback),
        cache
    );
    assert_eq!(
        choose_sidecar_profile_base_dir(None, None, &fallback),
        fallback
    );
}

#[test]
fn sidecar_profile_dir_lives_under_dedicated_state_folder() {
    let profile_dir = sidecar_profile_dir_for_base(&PathBuf::from("/tmp/desktop-app-local"));

    assert_eq!(
        profile_dir,
        PathBuf::from("/tmp/desktop-app-local/sidecar-profile/1688_profile")
    );
    assert!(
        !profile_dir.to_string_lossy().contains("src-tauri"),
        "profile dir must not point into the watched source tree"
    );
}

#[test]
fn run_task_uses_fallback_search_image_when_primary_has_no_match() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_sample_workbook(&excel_path);
    set_mock_pipeline_env(
        r#"[
          [{"title":"first-pass","price":"¥99.99","itemUrl":"https://detail.1688.com/offer/first.html","imageUrl":"https://img.1688.com/first.jpg"}],
          [{"title":"second-pass","price":"¥8.88","itemUrl":"https://detail.1688.com/offer/second.html","imageUrl":"https://img.1688.com/second.jpg"}],
          [{"title":"first-pass","price":"¥99.99","itemUrl":"https://detail.1688.com/offer/first.html","imageUrl":"https://img.1688.com/first.jpg"}],
          [{"title":"second-pass","price":"¥8.88","itemUrl":"https://detail.1688.com/offer/second.html","imageUrl":"https://img.1688.com/second.jpg"}]
        ]"#,
        r#"[
          [],
          [1],
          [1],
          [],
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("run task should succeed");

    let row_events = final_row_event_payloads(&sink);

    assert_eq!(summary.status, "completed");
    assert_eq!(row_events[0]["status"], "AI比对成功(备用搜索图召回)");
    assert_eq!(row_events[0]["price"], "¥8.88");
    assert_eq!(
        row_events[0]["item_url"],
        "https://detail.1688.com/offer/second.html"
    );
    let logs = log_messages(&sink);
    assert!(logs.iter().any(|line| line.contains("备用搜索图搜索中")));

    clear_mock_pipeline_env();
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_writes_result_workbook_with_brain_core_columns() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_sample_workbook(&excel_path);
    let result_path = excel_path.with_file_name("result.xlsx");

    set_mock_pipeline_env(
        r#"[
          [{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"https://img.1688.com/1.jpg"}],
          [{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"https://img.1688.com/1.jpg"}]
        ]"#,
        r#"[
          [1],
          [1],
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("run task should succeed");

    assert_eq!(
        summary.result_path.as_deref(),
        Some(result_path.to_string_lossy().as_ref())
    );

    let mut workbook: Xlsx<_> =
        open_workbook(&result_path).expect("result workbook should be readable");
    let range = workbook
        .worksheet_range_at(0)
        .expect("worksheet should exist")
        .expect("worksheet should be readable");

    assert_eq!(
        range.get_value((0, 0)).map(|v| v.to_string()),
        Some("title".to_string())
    );
    assert_eq!(
        range.get_value((0, 1)).map(|v| v.to_string()),
        Some("sku".to_string())
    );
    assert_eq!(
        range.get_value((0, 2)).map(|v| v.to_string()),
        Some("1688成本价".to_string())
    );
    assert_eq!(
        range.get_value((0, 3)).map(|v| v.to_string()),
        Some("1688链接".to_string())
    );
    assert_eq!(
        range.get_value((0, 4)).map(|v| v.to_string()),
        Some("AI分析结论".to_string())
    );
    assert_eq!(
        range.get_value((0, 5)).map(|v| v.to_string()),
        Some("图像比对耗时".to_string())
    );
    assert_eq!(
        range.get_value((1, 0)).map(|v| v.to_string()),
        Some("sample-1".to_string())
    );
    assert_eq!(
        range.get_value((1, 1)).map(|v| v.to_string()),
        Some("SKU-001".to_string())
    );
    assert_eq!(
        range.get_value((1, 2)).map(|v| v.to_string()),
        Some("¥12.34".to_string())
    );
    assert_eq!(
        range.get_value((1, 3)).map(|v| v.to_string()),
        Some("https://detail.1688.com/offer/1.html".to_string())
    );
    assert_eq!(
        range.get_value((1, 4)).map(|v| v.to_string()),
        Some("AI比对成功(主搜索图召回)".to_string())
    );
    assert!(
        range
            .get_value((1, 5))
            .map(|v| v.to_string())
            .unwrap_or_default()
            .ends_with('s'),
        "compare elapsed text should keep the root format like 0.01s"
    );

    clear_mock_pipeline_env();
    remove_if_exists(&excel_path);
    remove_if_exists(&result_path);
}

#[test]
fn run_task_reports_when_both_search_images_return_no_candidates() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_single_row_workbook(&excel_path);
    set_mock_pipeline_env(r#"[[],[]]"#, r#"[]"#);
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("run task should succeed");

    let row_events = final_row_event_payloads(&sink);

    assert_eq!(summary.status, "completed");
    assert_eq!(
        row_events[0]["status"],
        "无可比对候选(双搜索图未召回有效1688结果)"
    );

    clear_mock_pipeline_env();
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_reports_when_candidates_are_recalled_but_initial_screen_finds_no_strict_match() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_single_row_workbook(&excel_path);
    set_mock_pipeline_env(
        r#"[
          [{"title":"first-pass","price":"¥19.99","itemUrl":"https://detail.1688.com/offer/first.html","imageUrl":"https://img.1688.com/first.jpg"}],
          [{"title":"second-pass","price":"¥9.99","itemUrl":"https://detail.1688.com/offer/second.html","imageUrl":"https://img.1688.com/second.jpg"}]
        ]"#,
        r#"[
          [],
          []
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("run task should succeed");

    let row_events = final_row_event_payloads(&sink);

    assert_eq!(summary.status, "completed");
    assert_eq!(
        row_events[0]["status"],
        "候选已召回，但AI初筛未判定为高相似候选"
    );

    clear_mock_pipeline_env();
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_reports_when_final_review_rejects_all_recalled_candidates() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_single_row_workbook(&excel_path);
    set_mock_pipeline_env(
        r#"[
          [{"title":"first-pass","price":"¥19.99","itemUrl":"https://detail.1688.com/offer/first.html","imageUrl":"https://img.1688.com/first.jpg"}],
          [{"title":"second-pass","price":"¥9.99","itemUrl":"https://detail.1688.com/offer/second.html","imageUrl":"https://img.1688.com/second.jpg"}]
        ]"#,
        r#"[
          [1],
          [],
          [1],
          []
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("run task should succeed");

    let row_events = final_row_event_payloads(&sink);

    assert_eq!(summary.status, "completed");
    assert_eq!(row_events[0]["status"], "候选已召回，但终选复核未通过");

    clear_mock_pipeline_env();
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_persists_diagnostics_for_initial_screen_no_match() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_single_row_workbook(&excel_path);
    let diagnostics_root = std::env::temp_dir().join(format!(
        "desktop-app-diagnostics-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos()
    ));

    std::env::set_var("RUN_TASK_DIAGNOSTICS_ROOT", &diagnostics_root);
    set_mock_pipeline_env(
        r#"[
          [{"title":"first-pass","price":"¥19.99","itemUrl":"https://detail.1688.com/offer/first.html","imageUrl":"https://img.1688.com/first.jpg","cosScore":0.91}],
          [{"title":"second-pass","price":"¥9.99","itemUrl":"https://detail.1688.com/offer/second.html","imageUrl":"https://img.1688.com/second.jpg","cosScore":0.88}]
        ]"#,
        r#"[
          [],
          []
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("run task should succeed");

    assert_eq!(summary.status, "completed");
    let final_row_event_index = sink
        .payloads
        .iter()
        .position(|(name, payload)| {
            name == EVENT_ROW_RESULT && payload["is_final"].as_bool() == Some(true)
        })
        .expect("final row result should be emitted");
    let diagnostics_log_index = sink
        .payloads
        .iter()
        .position(|(name, payload)| {
            name == EVENT_LOG
                && payload["message"]
                    .as_str()
                    .map(|message| message.contains("诊断产物已写出"))
                    .unwrap_or(false)
        })
        .expect("diagnostics completion log should exist");
    assert!(
        final_row_event_index < diagnostics_log_index,
        "final row result should be emitted before diagnostics persistence finishes"
    );
    assert!(diagnostics_root.exists(), "diagnostics root should exist");

    let session_dirs = std::fs::read_dir(&diagnostics_root)
        .expect("read diagnostics root")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    assert_eq!(session_dirs.len(), 1, "expected one diagnostics session");

    let row_dirs = std::fs::read_dir(&session_dirs[0])
        .expect("read diagnostics session")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    assert_eq!(row_dirs.len(), 1, "expected one row diagnostics directory");

    let manifest_path = row_dirs[0].join("manifest.json");
    assert!(manifest_path.exists(), "manifest.json should exist");
    let manifest = std::fs::read_to_string(&manifest_path).expect("read manifest");
    assert!(manifest.contains("AI初筛未判定为高相似候选"));

    assert!(
        row_dirs[0].join("source_image.png").exists()
            || row_dirs[0].join("source_image.jpg").exists()
    );
    assert!(row_dirs[0].join("search_primary.png").exists());
    assert!(row_dirs[0].join("search_fallback.png").exists());
    assert!(row_dirs[0].join("primary_candidates.json").exists());

    std::env::remove_var("RUN_TASK_DIAGNOSTICS_ROOT");
    clear_mock_pipeline_env();
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
    remove_dir_if_exists(&diagnostics_root);
}

#[test]
fn run_task_logs_stage_timing_breakdown_for_each_row() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_single_row_workbook(&excel_path);
    set_mock_pipeline_env(
        r#"[
          [{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"https://img.1688.com/1.jpg"}]
        ]"#,
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("run task should succeed");

    assert_eq!(summary.status, "completed");
    let logs = log_messages(&sink);
    let timing_log = logs
        .iter()
        .find(|message| message.contains("阶段耗时"))
        .expect("timing summary log should exist");
    assert!(timing_log.contains("搜索图规划="));
    assert!(timing_log.contains("搜索图生成="));
    assert!(timing_log.contains("主搜="));
    assert!(timing_log.contains("AI初筛="));
    assert!(timing_log.contains("终审="));

    clear_mock_pipeline_env();
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_starts_next_row_before_diagnostics_finish() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_sample_workbook(&excel_path);
    let diagnostics_root = std::env::temp_dir().join(format!(
        "desktop-app-diagnostics-async-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos()
    ));

    std::env::set_var("RUN_TASK_DIAGNOSTICS_ROOT", &diagnostics_root);
    std::env::set_var("RUN_TASK_ALWAYS_WRITE_DIAGNOSTICS", "1");
    std::env::set_var("RUN_TASK_DIAGNOSTICS_DELAY_MS", "200");
    set_mock_pipeline_env(
        r#"[
          [{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"https://img.1688.com/1.jpg"}],
          [{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"https://img.1688.com/1.jpg"}]
        ]"#,
        r#"[
          [1],
          [1],
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("run task should succeed");

    assert_eq!(summary.status, "completed");
    let second_row_queued_index = sink
        .payloads
        .iter()
        .position(|(name, payload)| {
            name == EVENT_ROW_RESULT
                && payload["row_index"].as_u64() == Some(3)
                && payload["stage"].as_str() == Some("queued")
        })
        .expect("second row queued event should exist");
    let first_diagnostics_log_index = sink
        .payloads
        .iter()
        .position(|(name, payload)| {
            name == EVENT_LOG
                && payload["message"]
                    .as_str()
                    .map(|message| message.contains("诊断产物已写出"))
                    .unwrap_or(false)
        })
        .expect("diagnostics completion log should exist");
    assert!(
        second_row_queued_index < first_diagnostics_log_index,
        "next row should start before diagnostics persistence finishes"
    );

    std::env::remove_var("RUN_TASK_DIAGNOSTICS_ROOT");
    std::env::remove_var("RUN_TASK_ALWAYS_WRITE_DIAGNOSTICS");
    std::env::remove_var("RUN_TASK_DIAGNOSTICS_DELAY_MS");
    clear_mock_pipeline_env();
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
    remove_dir_if_exists(&diagnostics_root);
}

#[test]
fn run_task_uses_original_source_directory_for_result_and_diagnostics_when_provided() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let source_dir = std::env::temp_dir().join(format!(
        "desktop-app-source-dir-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos()
    ));
    let upload_dir = std::env::temp_dir().join(format!(
        "desktop-app-upload-dir-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos()
    ));
    std::fs::create_dir_all(&source_dir).expect("create source dir");
    std::fs::create_dir_all(&upload_dir).expect("create upload dir");

    let source_excel = source_dir.join("1.xlsx");
    let uploaded_excel = upload_dir.join("copied-1.xlsx");
    create_single_row_workbook(&source_excel);
    std::fs::copy(&source_excel, &uploaded_excel).expect("copy uploaded excel");

    set_mock_pipeline_env(
        r#"[
          [{"title":"first-pass","price":"¥19.99","itemUrl":"https://detail.1688.com/offer/first.html","imageUrl":"https://img.1688.com/first.jpg","cosScore":0.91}],
          [{"title":"second-pass","price":"¥9.99","itemUrl":"https://detail.1688.com/offer/second.html","imageUrl":"https://img.1688.com/second.jpg","cosScore":0.88}]
        ]"#,
        r#"[
          [],
          []
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_original_source_and_sink(
        uploaded_excel.to_string_lossy().as_ref(),
        Some(source_excel.to_string_lossy().as_ref()),
        &mut sink,
    )
    .expect("run task should succeed");

    assert_eq!(
        summary.result_path.as_deref(),
        Some(source_dir.join("result.xlsx").to_string_lossy().as_ref())
    );
    assert!(source_dir.join("desktop_app_diagnostics").exists());
    assert!(!upload_dir.join("desktop_app_diagnostics").exists());

    clear_mock_pipeline_env();
    remove_if_exists(&source_excel);
    remove_if_exists(&uploaded_excel);
    remove_if_exists(&source_dir.join("result.xlsx"));
    remove_dir_if_exists(&source_dir.join("desktop_app_diagnostics"));
    remove_dir_if_exists(&source_dir);
    remove_dir_if_exists(&upload_dir);
}

#[test]
fn shutdown_managed_sidecar_posts_shutdown_even_without_owned_child_process() {
    let _guard = lock_env();

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    listener
        .set_nonblocking(true)
        .expect("set listener nonblocking");
    let address = listener.local_addr().expect("resolve listener addr");
    let (tx, rx) = mpsc::channel();

    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    let request = String::from_utf8_lossy(&buffer).to_string();
                    let _ = stream.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\n\r\n{\"success\":true}",
                    );
                    tx.send(request).expect("send request body");
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        tx.send(String::new()).expect("send empty request");
                        return;
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                Err(_) => {
                    tx.send(String::new()).expect("send empty request");
                    return;
                }
            }
        }
    });

    std::env::set_var("SIDECAR_SHUTDOWN_URL", format!("http://{address}/shutdown"));
    shutdown_managed_sidecar();
    std::env::remove_var("SIDECAR_SHUTDOWN_URL");

    let request = rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive request probe");
    handle.join().expect("join listener thread");

    assert!(
        request.starts_with("POST /shutdown HTTP/1.1"),
        "expected shutdown POST request, got: {request:?}"
    );
}
