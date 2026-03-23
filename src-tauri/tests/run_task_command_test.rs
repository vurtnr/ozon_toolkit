use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
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
    sidecar_runtime_dir_for_base, RunTaskSummary,
};
use desktop_app_lib::core::ozon_cache::cache_root_for_output_anchor;
use desktop_app_lib::events::{
    EventSink, EVENT_BLOCKING_ALERT, EVENT_LOG, EVENT_PROGRESS, EVENT_ROW_RESULT, EVENT_TASK_DONE,
};
use desktop_app_lib::recovery::GLOBAL_RECOVERY_GATE;
use rust_xlsxwriter::Workbook;
use serde_json::Value;
use zip::ZipArchive;

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

fn make_temp_work_dir(name: &str) -> PathBuf {
    let unique = format!(
        "{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should move forward")
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

fn create_url_mode_workbook(path: &PathBuf, rows: &[(&str, &str, &str)]) {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();

    worksheet
        .write_string(0, 0, "ozon链接")
        .expect("write header");
    worksheet.write_string(0, 1, "sku").expect("write header");
    worksheet
        .write_string(0, 2, "产品重量")
        .expect("write header");

    for (index, (url, sku, weight)) in rows.iter().enumerate() {
        let row = (index + 1) as u32;
        worksheet.write_string(row, 0, *url).expect("write url");
        worksheet.write_string(row, 1, *sku).expect("write sku");
        worksheet
            .write_string(row, 2, *weight)
            .expect("write weight");
    }

    workbook.save(path).expect("save workbook");
}

fn spawn_ozon_antibot_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ozon antibot listener");
    listener
        .set_nonblocking(true)
        .expect("set ozon antibot listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve ozon antibot listener address");

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 4096];
                    let _ = stream.read(&mut buffer);
                    let body = r#"<html><head><title>Antibot Captcha</title></head><body><input id="captcha-input" type="hidden" value="challenge"></body></html>"#;
                    let response = format!(
                        "HTTP/1.1 403 Forbidden\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nozon-antibot: 1\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if started_at.elapsed() >= Duration::from_secs(5) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}"), handle)
}

fn spawn_ozon_status_server(
    status_line: &'static str,
    body: &'static str,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ozon status listener");
    listener
        .set_nonblocking(true)
        .expect("set ozon status listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve ozon status listener address");

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 4096];
                    let _ = stream.read(&mut buffer);
                    let response = format!(
                        "HTTP/1.1 {status_line}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if started_at.elapsed() >= Duration::from_secs(5) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}"), handle)
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
    assert_eq!(
        build_match_hint("sample title", "sample title"),
        "sample title"
    );
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

fn spawn_image_server() -> (String, thread::JoinHandle<()>) {
    const SAMPLE_PNG_BYTES: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240,
        31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind image listener");
    listener
        .set_nonblocking(true)
        .expect("set image listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve image listener address");

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        let mut last_activity = Instant::now();
        let mut served_any = false;

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        SAMPLE_PNG_BYTES.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(SAMPLE_PNG_BYTES);
                    served_any = true;
                    last_activity = Instant::now();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if served_any && last_activity.elapsed() >= Duration::from_millis(250) {
                        return;
                    }
                    if started_at.elapsed() >= Duration::from_secs(5) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}/sample.png"), handle)
}

fn set_mock_vlm_env(vlm_replies_json: &str) {
    std::env::set_var("RUN_TASK_MOCK_VLM_REPLIES_JSON", vlm_replies_json);
}

fn clear_sidecar_fixture_env() {
    std::env::remove_var("SIDECAR_HEALTH_URL");
    std::env::remove_var("SIDECAR_SESSION_URL");
    std::env::remove_var("SIDECAR_SEARCH_URL");
    std::env::remove_var("SIDECAR_OZON_RESOLVE_URL");
}

fn spawn_sidecar_health_server() -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind health listener");
    listener
        .set_nonblocking(true)
        .expect("set health listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve health listener address");

    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    let body = r#"{"success":true}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}/health"), handle)
}

fn spawn_sidecar_session_server(bodies: Vec<&'static str>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind session listener");
    listener
        .set_nonblocking(true)
        .expect("set session listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve session listener address");

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        let mut body_iter = bodies.into_iter();

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    let Some(body) = body_iter.next() else {
                        return;
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if started_at.elapsed() >= Duration::from_secs(5) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}/session-state"), handle)
}

fn spawn_sidecar_search_server(
    response_body: String,
    max_requests: usize,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind search listener");
    listener
        .set_nonblocking(true)
        .expect("set search listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve search listener address");

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        let mut served = 0usize;

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 4096];
                    let _ = stream.read(&mut buffer);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response_body.len(),
                        response_body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    served += 1;
                    if served >= max_requests {
                        return;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if started_at.elapsed() >= Duration::from_secs(5) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}/search"), handle)
}

fn spawn_sidecar_ozon_resolve_server(
    response_body: String,
    max_requests: usize,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ozon resolve listener");
    listener
        .set_nonblocking(true)
        .expect("set ozon resolve listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve ozon resolve listener address");

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        let mut served = 0usize;

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 4096];
                    let _ = stream.read(&mut buffer);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response_body.len(),
                        response_body
                    );
                    let _ = stream.write_all(response.as_bytes());
                    served += 1;
                    if served >= max_requests {
                        return;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if started_at.elapsed() >= Duration::from_secs(5) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}/resolve-ozon-product"), handle)
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
    assert!(
        row_events
            .iter()
            .any(|payload| payload["stage"] == "planning_search_image"),
        "row event stream should eventually enter planning_search_image"
    );
    assert_eq!(final_row_events.len(), 2);
    assert_eq!(final_row_events[0]["sku"], "SKU-001");
    assert_eq!(final_row_events[1]["sku"], "SKU-002");
    assert_eq!(final_row_events[0]["status"], "AI比对成功(主搜索图召回)");
    assert_eq!(final_row_events[0]["price"], "¥12.34");
    assert_eq!(
        final_row_events[0]["item_url"],
        "https://detail.1688.com/offer/1.html"
    );
    assert!(
        final_row_events[0]["original_image_url"]
            .as_str()
            .unwrap_or_default()
            .starts_with("data:image/"),
        "final row event should carry the original thumbnail"
    );
    assert_eq!(
        final_row_events[0]["matched_image_url"],
        "https://img.1688.com/1.jpg"
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
fn run_task_exports_directly_when_all_ozon_rows_fail_preflight() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let result_path = excel_path.with_file_name("result.xlsx");
    create_url_mode_workbook(
        &excel_path,
        &[(
            "http://127.0.0.1:9/product/3570411011",
            "SKU-PREFLIGHT-404",
            "500 g",
        )],
    );
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        r#"{"success":false,"error":"[OZON_PRODUCT_UNAVAILABLE] Ozon 商品页显示为不可访问或已下架"}"#.to_string(),
        1,
    );
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    set_mock_vlm_env(r#"[]"#);

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("all source failures should export result without entering 1688 matching");

    assert_eq!(summary.status, "completed");
    assert_eq!(
        summary.result_path.as_deref(),
        Some(result_path.to_string_lossy().as_ref())
    );

    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "Ozon商品已下架或不可访问");
    assert!(
        row_event_payloads(&sink)
            .iter()
            .all(|payload| payload["stage"] != "planning_search_image"),
        "preflight-only failure rows must not enter 1688 search stages"
    );

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    resolve_handle.join().expect("join ozon resolve server");
    remove_if_exists(&excel_path);
    remove_if_exists(&result_path);
}

#[test]
fn run_task_resolves_ozon_rows_before_requiring_sidecar() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    create_url_mode_workbook(
        &excel_path,
        &[(
            "http://127.0.0.1:9/product/3570411012",
            "SKU-PREFLIGHT-READY",
            "400 g",
        )],
    );
    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (session_url, session_handle) =
        spawn_sidecar_session_server(vec![r#"{"success":true,"status":"ready"}"#]);
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );
    std::env::set_var("SIDECAR_SESSION_URL", &session_url);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    std::env::set_var("SIDECAR_SEARCH_URL", "http://127.0.0.1:9/search");
    set_mock_vlm_env(r#"[]"#);
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("URL-mode rows should continue into search stage after sidecar hydration");

    assert_eq!(summary.status, "completed");
    assert!(
        row_event_payloads(&sink)
            .iter()
            .any(|payload| payload["stage"] == "resolving_ozon_product"),
        "ozon source resolution stage should be emitted before matching"
    );
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "Node爬虫获取失败");

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    image_server_handle.join().expect("join image server");
    session_handle.join().expect("join session server");
    resolve_handle.join().expect("join ozon resolve server");
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_waits_for_login_before_search_stages() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    create_url_mode_workbook(
        &excel_path,
        &[(
            "http://127.0.0.1:9/product/3570411013",
            "SKU-LOGIN-WAIT",
            "300 g",
        )],
    );

    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (session_url, session_handle) = spawn_sidecar_session_server(vec![
        r#"{"success":true,"status":"login_required"}"#,
        r#"{"success":true,"status":"login_required"}"#,
        r#"{"success":true,"status":"ready"}"#,
    ]);
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );
    let (search_url, search_handle) = spawn_sidecar_search_server(
        format!(
            r#"{{"success":true,"data":[{{"title":"candidate","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]}}"#
        ),
        2,
    );

    std::env::set_var("SIDECAR_SESSION_URL", &session_url);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url);
    std::env::set_var(
        "RUN_TASK_MOCK_VLM_REPLIES_JSON",
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("login wait should resume once the sidecar session becomes ready");

    assert_eq!(summary.status, "completed");
    let blocking_alert_index = sink
        .payloads
        .iter()
        .position(|(name, _)| name == EVENT_BLOCKING_ALERT)
        .expect("login-required alert should be emitted");
    let task_phase_events: Vec<&Value> = sink
        .payloads
        .iter()
        .filter(|(name, _)| name == "task_phase")
        .map(|(_, payload)| payload)
        .collect();
    assert!(
        task_phase_events
            .iter()
            .any(|payload| payload["phase"] == "resolving_ozon_products"),
        "task phase stream should expose ozon preflight"
    );
    assert!(
        task_phase_events
            .iter()
            .any(|payload| payload["phase"] == "waiting_for_1688_login"),
        "task phase stream should expose login wait"
    );
    let first_search_stage_index = sink
        .payloads
        .iter()
        .position(|(name, payload)| {
            name == EVENT_ROW_RESULT && payload["stage"] == "planning_search_image"
        })
        .expect("search stages should start after login is ready");
    assert!(
        blocking_alert_index < first_search_stage_index,
        "blocking login alert must arrive before any 1688 search stage"
    );

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    image_server_handle.join().expect("join image server");
    session_handle.join().expect("join session server");
    resolve_handle.join().expect("join ozon resolve server");
    search_handle.join().expect("join search server");
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_uses_browser_fallback_when_ozon_http_prefetch_hits_antibot() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let (ozon_base_url, ozon_handle) = spawn_ozon_antibot_server();
    create_url_mode_workbook(
        &excel_path,
        &[(
            &format!("{ozon_base_url}/product/3552213000"),
            "SKU-OZON-ANTIBOT",
            "200 g",
        )],
    );

    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (health_url, health_handle) = spawn_sidecar_health_server();
    let (session_url, session_handle) =
        spawn_sidecar_session_server(vec![r#"{"success":true,"status":"ready"}"#]);
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );
    let (search_url, search_handle) = spawn_sidecar_search_server(
        format!(
            r#"{{"success":true,"data":[{{"title":"candidate","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]}}"#
        ),
        2,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url);
    std::env::set_var(
        "RUN_TASK_MOCK_VLM_REPLIES_JSON",
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("antibot rows should fall back to browser-assisted ozon resolve");

    assert_eq!(summary.status, "completed");
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "AI比对成功(主搜索图召回)");

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    ozon_handle.join().expect("join ozon fixture server");
    image_server_handle.join().expect("join image server");
    health_handle.join().expect("join health server");
    session_handle.join().expect("join session server");
    resolve_handle.join().expect("join ozon resolve server");
    search_handle.join().expect("join search server");
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_uses_browser_fallback_for_503_ozon_challenge_pages() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let (ozon_base_url, ozon_handle) = spawn_ozon_status_server(
        "503 Service Unavailable",
        r#"<html><head><title>Antibot Challenge Page</title></head><body><input id="captcha-input" type="hidden" value="challenge"></body></html>"#,
    );
    create_url_mode_workbook(
        &excel_path,
        &[(
            &format!("{ozon_base_url}/product/3552213000"),
            "SKU-OZON-503-CHALLENGE",
            "210 g",
        )],
    );

    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (health_url, health_handle) = spawn_sidecar_health_server();
    let (session_url, session_handle) =
        spawn_sidecar_session_server(vec![r#"{"success":true,"status":"ready"}"#]);
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );
    let (search_url, search_handle) = spawn_sidecar_search_server(
        format!(
            r#"{{"success":true,"data":[{{"title":"candidate","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]}}"#
        ),
        2,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url);
    std::env::set_var(
        "RUN_TASK_MOCK_VLM_REPLIES_JSON",
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("503 challenge rows should fall back to browser-assisted ozon resolve");

    assert_eq!(summary.status, "completed");
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "AI比对成功(主搜索图召回)");

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    ozon_handle.join().expect("join ozon status server");
    image_server_handle.join().expect("join image server");
    health_handle.join().expect("join health server");
    session_handle.join().expect("join session server");
    resolve_handle.join().expect("join ozon resolve server");
    search_handle.join().expect("join search server");
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_does_not_depend_on_rust_ozon_prefetch_unavailable_result() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let (ozon_base_url, ozon_handle) = spawn_ozon_status_server("404 Not Found", "not found");
    create_url_mode_workbook(
        &excel_path,
        &[(
            &format!("{ozon_base_url}/product/3552213000"),
            "SKU-OZON-BROWSER-ONLY",
            "220 g",
        )],
    );

    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (health_url, health_handle) = spawn_sidecar_health_server();
    let (session_url, session_handle) =
        spawn_sidecar_session_server(vec![r#"{"success":true,"status":"ready"}"#]);
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );
    let (search_url, search_handle) = spawn_sidecar_search_server(
        format!(
            r#"{{"success":true,"data":[{{"title":"candidate","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]}}"#
        ),
        2,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url);
    std::env::set_var(
        "RUN_TASK_MOCK_VLM_REPLIES_JSON",
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("URL-mode rows should not be finalized from Rust-side unavailable prefetch");

    assert_eq!(summary.status, "completed");
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "AI比对成功(主搜索图召回)");

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    ozon_handle.join().expect("join ozon status server");
    image_server_handle.join().expect("join image server");
    health_handle.join().expect("join health server");
    session_handle.join().expect("join session server");
    resolve_handle.join().expect("join ozon resolve server");
    search_handle.join().expect("join search server");
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_prefers_sidecar_returned_ozon_image_bytes_over_rust_redownload() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let (ozon_base_url, ozon_handle) = spawn_ozon_antibot_server();
    create_url_mode_workbook(
        &excel_path,
        &[(
            &format!("{ozon_base_url}/product/3552213000"),
            "SKU-OZON-IMAGE-BYTES",
            "230 g",
        )],
    );

    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (health_url, health_handle) = spawn_sidecar_health_server();
    let (session_url, session_handle) =
        spawn_sidecar_session_server(vec![r#"{"success":true,"status":"ready"}"#]);
    let embedded_png = BASE64_STANDARD.encode(vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6,
        0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240,
        31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ]);
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"https://example.invalid/unreachable.png","imageBase64":"{embedded_png}"}}}}"#
        ),
        1,
    );
    let (search_url, search_handle) = spawn_sidecar_search_server(
        format!(
            r#"{{"success":true,"data":[{{"title":"candidate","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]}}"#
        ),
        2,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url);
    std::env::set_var(
        "RUN_TASK_MOCK_VLM_REPLIES_JSON",
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("browser-assisted ozon resolve should use returned image bytes directly");

    assert_eq!(summary.status, "completed");
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "AI比对成功(主搜索图召回)");

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    ozon_handle.join().expect("join ozon fixture server");
    image_server_handle.join().expect("join image server");
    health_handle.join().expect("join health server");
    session_handle.join().expect("join session server");
    resolve_handle.join().expect("join ozon resolve server");
    search_handle.join().expect("join search server");
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_stops_when_browser_assisted_ozon_resolve_remains_blocked() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let (ozon_base_url, ozon_handle) = spawn_ozon_antibot_server();
    create_url_mode_workbook(
        &excel_path,
        &[(
            &format!("{ozon_base_url}/product/3552213000"),
            "SKU-OZON-STILL-BLOCKED",
            "260 g",
        )],
    );

    let (health_url, health_handle) = spawn_sidecar_health_server();
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        r#"{"success":false,"code":"ANTI_BOT_CHALLENGE","error":"[ANTI_BOT_CHALLENGE] Ozon page remains restricted"}"#.to_string(),
        1,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    std::env::set_var("SIDECAR_SESSION_URL", "http://127.0.0.1:9/session-state");
    std::env::set_var("SIDECAR_SEARCH_URL", "http://127.0.0.1:9/search");
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let err = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect_err("task should stop when ozon browser fallback remains blocked");

    assert_eq!(err, "ANTI_BOT_CHALLENGE");
    let task_phase_events: Vec<&Value> = sink
        .payloads
        .iter()
        .filter(|(name, _)| name == "task_phase")
        .map(|(_, payload)| payload)
        .collect();
    assert!(
        task_phase_events
            .iter()
            .any(|payload| payload["phase"] == "warming_ozon_session"),
        "task phase stream should expose ozon session warm-up before browser hydration"
    );
    assert!(
        task_phase_events
            .iter()
            .any(|payload| payload["phase"] == "waiting_for_ozon_verification"),
        "task phase stream should expose ozon verification wait when hydration remains blocked"
    );
    assert!(
        sink.payloads.iter().any(|(name, payload)| {
            name == EVENT_BLOCKING_ALERT && payload["code"] == "ANTI_BOT_CHALLENGE"
        }),
        "blocking alert should be emitted when ozon remains blocked",
    );
    assert!(
        !sink.payloads.iter().any(|(name, payload)| {
            name == EVENT_ROW_RESULT && payload["stage"] == "planning_search_image"
        }),
        "1688 planning stage must not start while ozon hydration remains blocked",
    );

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    ozon_handle.join().expect("join ozon fixture server");
    health_handle.join().expect("join health server");
    resolve_handle.join().expect("join ozon resolve server");
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_url_mode_successfully_resolves_ozon_source_before_1688() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_url_mode_workbook(
        &excel_path,
        &[(
            "http://127.0.0.1:9/product/3570411009",
            "SKU-URL-001",
            "400 g",
        )],
    );

    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Морская верёвочная лестница","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    set_mock_pipeline_env(
        &format!(
            r#"[
              [{{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]
            ]"#
        ),
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("url-mode workbook should resolve ozon source before matching");

    assert_eq!(summary.status, "completed");
    let row_events = row_event_payloads(&sink);
    assert!(
        row_events
            .iter()
            .any(|payload| payload["stage"] == "resolving_ozon_product"),
        "url-mode rows should emit a dedicated ozon resolve stage"
    );
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["sku"], "SKU-URL-001");
    assert_eq!(final_rows[0]["status"], "AI比对成功(主搜索图召回)");

    clear_mock_pipeline_env();
    image_server_handle.join().expect("join image server");
    resolve_handle.join().expect("join ozon resolve server");
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_reuses_disk_cached_ozon_source_without_sidecar_resolve() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let work_dir = make_temp_work_dir("run-task-ozon-cache");
    std::fs::create_dir_all(&work_dir).expect("create work dir");
    let excel_path = work_dir.join("input.xlsx");
    create_url_mode_workbook(
        &excel_path,
        &[(
            "http://127.0.0.1:9/product/3570411009",
            "SKU-OZON-CACHE-001",
            "400 g",
        )],
    );

    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (health_url_1, health_handle_1) = spawn_sidecar_health_server();
    let (session_url_1, session_handle_1) =
        spawn_sidecar_session_server(vec![r#"{"success":true,"status":"ready"}"#]);
    let (search_url_1, search_handle_1) = spawn_sidecar_search_server(
        format!(
            r#"{{"success":true,"data":[{{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]}}"#
        ),
        1,
    );
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Морская верёвочная лестница","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url_1);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url_1);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url_1);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    set_mock_vlm_env(
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut first_sink = CollectingSink::default();
    let first_summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut first_sink)
        .expect("first run should hydrate ozon source and write cache");
    assert_eq!(first_summary.status, "completed");
    assert!(
        cache_root_for_output_anchor(&excel_path).exists(),
        "ozon cache root should be created after first hydration"
    );

    clear_sidecar_fixture_env();
    health_handle_1.join().expect("join health server");
    session_handle_1.join().expect("join session server");
    search_handle_1.join().expect("join search server");
    resolve_handle.join().expect("join ozon resolve server");

    let (health_url_2, health_handle_2) = spawn_sidecar_health_server();
    let (session_url_2, session_handle_2) =
        spawn_sidecar_session_server(vec![r#"{"success":true,"status":"ready"}"#]);
    let (search_url_2, search_handle_2) = spawn_sidecar_search_server(
        format!(
            r#"{{"success":true,"data":[{{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]}}"#
        ),
        1,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url_2);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url_2);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url_2);
    std::env::set_var(
        "SIDECAR_OZON_RESOLVE_URL",
        "http://127.0.0.1:9/resolve-ozon-product",
    );
    set_mock_vlm_env(
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut second_sink = CollectingSink::default();
    let second_summary =
        run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut second_sink)
            .expect("second run should reuse disk cache without hitting ozon resolve sidecar");
    assert_eq!(second_summary.status, "completed");
    assert!(
        !second_sink
            .payloads
            .iter()
            .filter(|(name, _)| name == "task_phase")
            .map(|(_, payload)| payload)
            .any(|payload| payload["phase"] == "warming_ozon_session"),
        "cache hit should skip ozon browser warm-up on subsequent runs"
    );

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    image_server_handle.join().expect("join image server");
    health_handle_2.join().expect("join health server");
    session_handle_2.join().expect("join session server");
    search_handle_2.join().expect("join search server");
    remove_dir_if_exists(&cache_root_for_output_anchor(&excel_path));
    remove_if_exists(&excel_path);
    remove_if_exists(&work_dir.join("result.xlsx"));
    remove_dir_if_exists(&work_dir);
}

#[test]
fn run_task_leaves_ai_conclusion_empty_for_ozon_source_failures() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    let result_path = excel_path.with_file_name("result.xlsx");
    create_url_mode_workbook(
        &excel_path,
        &[(
            "http://127.0.0.1:9/product/3570411010",
            "SKU-URL-404",
            "500 g",
        )],
    );

    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        r#"{"success":false,"error":"[OZON_PRODUCT_UNAVAILABLE] Ozon 商品页显示为不可访问或已下架"}"#.to_string(),
        1,
    );
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    set_mock_vlm_env(r#"[]"#);

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("source failures should still produce a result workbook");

    assert_eq!(summary.status, "completed");
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "Ozon商品已下架或不可访问");

    let mut workbook: Xlsx<_> =
        open_workbook(&result_path).expect("result workbook should be readable");
    let range = workbook
        .worksheet_range_at(0)
        .expect("worksheet should exist")
        .expect("worksheet should be readable");

    assert_eq!(
        range.get_value((0, 5)).map(|v| v.to_string()),
        Some("处理状态".to_string())
    );
    assert_eq!(
        range.get_value((0, 6)).map(|v| v.to_string()),
        Some("AI分析结论".to_string())
    );
    assert_eq!(
        range.get_value((1, 5)).map(|v| v.to_string()),
        Some("Ozon商品已下架或不可访问".to_string())
    );
    assert!(
        range.get_value((1, 6)).is_none()
            || range.get_value((1, 6)).map(|v| v.to_string()) == Some(String::new()),
        "AI analysis column should remain empty when ozon source resolution fails"
    );

    clear_mock_pipeline_env();
    resolve_handle.join().expect("join ozon resolve server");
    remove_if_exists(&excel_path);
    remove_if_exists(&result_path);
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
fn run_task_writes_result_workbook_with_brain_core_columns_and_images() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();

    let excel_path = make_temp_excel_path();
    create_sample_workbook(&excel_path);
    let result_path = excel_path.with_file_name("result.xlsx");
    let (image_url, image_server_handle) = spawn_image_server();

    set_mock_pipeline_env(
        &format!(
            r#"[
              [{{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{image_url}"}}],
              [{{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{image_url}"}}]
            ]"#
        ),
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
        Some("处理状态".to_string())
    );
    assert_eq!(
        range.get_value((0, 5)).map(|v| v.to_string()),
        Some("AI分析结论".to_string())
    );
    assert_eq!(
        range.get_value((0, 6)).map(|v| v.to_string()),
        Some("图像比对耗时".to_string())
    );
    assert_eq!(
        range.get_value((0, 7)).map(|v| v.to_string()),
        Some("原图".to_string())
    );
    assert_eq!(
        range.get_value((0, 8)).map(|v| v.to_string()),
        Some("匹配图".to_string())
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
            .contains("AI比对成功"),
        "ai analysis conclusion should retain the original comparison summary"
    );
    assert!(
        range
            .get_value((1, 6))
            .map(|v| v.to_string())
            .unwrap_or_default()
            .ends_with('s'),
        "compare elapsed text should keep the root format like 0.01s"
    );

    let file = std::fs::File::open(&result_path).expect("result workbook should exist on disk");
    let mut archive = ZipArchive::new(file).expect("result workbook should be a zip archive");
    let drawing_names: Vec<String> = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index(index)
                .ok()
                .map(|entry| entry.name().to_string())
        })
        .filter(|name| name.starts_with("xl/drawings/drawing") && name.ends_with(".xml"))
        .collect();

    assert!(
        !drawing_names.is_empty(),
        "exported workbook should include drawing XML for thumbnails"
    );

    let mut picture_count = 0usize;
    for name in drawing_names {
        let mut xml = String::new();
        archive
            .by_name(&name)
            .expect("drawing xml should exist")
            .read_to_string(&mut xml)
            .expect("drawing xml should be readable");
        picture_count += xml.matches("<xdr:pic>").count();
    }

    assert!(
        picture_count >= 4,
        "expected at least 4 embedded pictures for 2 rows x 2 columns, got {picture_count}"
    );

    clear_mock_pipeline_env();
    image_server_handle.join().expect("join image server");
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
