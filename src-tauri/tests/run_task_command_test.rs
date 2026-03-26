use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
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
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
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

    worksheet.write_string(0, 0, "ozon链接").expect("write header");
    worksheet.write_string(0, 1, "sku").expect("write header");
    worksheet
        .write_string(1, 0, "https://www.ozon.ru/product/1000001")
        .expect("write row 1 url");
    worksheet
        .write_string(1, 1, "SKU-001")
        .expect("write row 1 sku");
    worksheet
        .write_string(2, 0, "https://www.ozon.ru/product/1000002")
        .expect("write row 2 url");
    worksheet
        .write_string(2, 1, "SKU-002")
        .expect("write row 2 sku");

    workbook.save(path).expect("save workbook");
}

fn create_single_row_workbook(path: &PathBuf) {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();

    worksheet.write_string(0, 0, "ozon链接").expect("write header");
    worksheet.write_string(0, 1, "sku").expect("write header");
    worksheet
        .write_string(1, 0, "https://www.ozon.ru/product/1000001")
        .expect("write row 1 url");
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

fn create_sku_mode_workbook(path: &PathBuf, rows: &[(&str, &str)]) {
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();

    worksheet.write_string(0, 0, "title").expect("write header");
    worksheet.write_string(0, 1, "sku").expect("write header");

    for (index, (title, sku)) in rows.iter().enumerate() {
        let row = (index + 1) as u32;
        worksheet.write_string(row, 0, *title).expect("write title");
        worksheet.write_string(row, 1, *sku).expect("write sku");
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
                    if started_at.elapsed() >= Duration::from_secs(30) {
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
                    if started_at.elapsed() >= Duration::from_secs(30) {
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

fn build_png_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
        width,
        height,
        Rgba([48, 96, 160, 255]),
    ));
    let mut cursor = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut cursor, ImageFormat::Png)
        .expect("png should encode");
    cursor.into_inner()
}

fn spawn_image_server() -> (String, thread::JoinHandle<()>) {
    let sample_png_bytes = build_png_bytes(320, 320);

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
                        sample_png_bytes.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.write_all(&sample_png_bytes);
                    served_any = true;
                    last_activity = Instant::now();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if served_any && last_activity.elapsed() >= Duration::from_millis(250) {
                        return;
                    }
                    // Use a longer initial timeout (30s) to accommodate the random 3-8s human-browsing delay
                    if started_at.elapsed() >= Duration::from_secs(30) {
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
    std::env::remove_var("SIDECAR_OZON_CLOSE_URL");
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
        let deadline = Instant::now() + Duration::from_secs(30);
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
                    if started_at.elapsed() >= Duration::from_secs(30) {
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

fn spawn_sidecar_recording_session_server(
    bodies: Vec<&'static str>,
    order: Arc<Mutex<Vec<String>>>,
) -> (String, thread::JoinHandle<()>) {
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
                    order
                        .lock()
                        .expect("order lock")
                        .push("session".to_string());
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
                    if started_at.elapsed() >= Duration::from_secs(30) {
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

fn spawn_sidecar_ozon_close_server(
    order: Arc<Mutex<Vec<String>>>,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ozon close listener");
    listener
        .set_nonblocking(true)
        .expect("set ozon close listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve ozon close listener address");

    let handle = thread::spawn(move || {
        let started_at = Instant::now();
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    order
                        .lock()
                        .expect("order lock")
                        .push("close".to_string());
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
                    if started_at.elapsed() >= Duration::from_secs(30) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}/close-ozon-session"), handle)
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
                    if started_at.elapsed() >= Duration::from_secs(30) {
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
        // Use a longer timeout (30s) to accommodate the random 3-8s human-browsing delay per row
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
                    if started_at.elapsed() >= Duration::from_secs(30) {
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

fn spawn_sidecar_ozon_resolve_sequence_server(
    response_bodies: Vec<String>,
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
        let mut remaining = response_bodies.into_iter();

        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let Some(response_body) = remaining.next() else {
                        return;
                    };

                    let mut buffer = [0u8; 4096];
                    let _ = stream.read(&mut buffer);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response_body.len(),
                        response_body
                    );
                    let _ = stream.write_all(response.as_bytes());

                    if remaining.len() == 0 {
                        return;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if started_at.elapsed() >= Duration::from_secs(30) {
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

fn spawn_sidecar_ozon_sku_resolve_server(
    response_body: String,
    max_requests: usize,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ozon sku resolve listener");
    listener
        .set_nonblocking(true)
        .expect("set ozon sku resolve listener nonblocking");
    let address = listener
        .local_addr()
        .expect("resolve ozon sku resolve listener address");

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
                    if started_at.elapsed() >= Duration::from_secs(30) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return,
            }
        }
    });

    (format!("http://{address}/resolve-ozon-sku"), handle)
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
fn run_task_emits_ozon_sku_resolution_stage_before_matching() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    create_url_mode_workbook(&excel_path, &[("https://www.ozon.ru/product/3552213100", "SKU-PREFLIGHT-READY", "200 g")]);
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
        .expect("sku-mode rows should continue into search stage after sidecar hydration");

    assert_eq!(summary.status, "completed");
    assert!(
        row_event_payloads(&sink)
            .iter()
            .any(|payload| payload["stage"] == "resolving_ozon_product"),
        "ozon product resolution stage should be emitted before matching"
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
fn run_task_batches_ozon_resolution_for_all_skus_before_1688_login_gate() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    create_url_mode_workbook(
        &excel_path,
        &[("https://www.ozon.ru/product/3552213001", "SKU-BATCH-001", "200 g"), ("https://www.ozon.ru/product/3552213002", "SKU-BATCH-002", "200 g")],
    );

    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (session_url, session_handle) = spawn_sidecar_session_server(vec![
        r#"{"success":true,"status":"login_required"}"#,
        r#"{"success":true,"status":"ready"}"#,
    ]);
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        2,
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
          [1],
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("sku-mode rows should all resolve on Ozon before the 1688 login gate");

    assert_eq!(summary.status, "completed");
    let second_ozon_stage_index = sink
        .payloads
        .iter()
        .position(|(name, payload)| {
            name == EVENT_ROW_RESULT
                && payload["sku"] == "SKU-BATCH-002"
                && payload["stage"] == "resolving_ozon_product"
        })
        .expect("second sku should finish the Ozon stage before 1688 login wait");
    let waiting_login_index = sink
        .payloads
        .iter()
        .position(|(name, payload)| name == "task_phase" && payload["phase"] == "waiting_for_1688_login")
        .expect("1688 login wait should be emitted after Ozon batch completion");
    let first_search_stage_index = sink
        .payloads
        .iter()
        .position(|(name, payload)| {
            name == EVENT_ROW_RESULT && payload["stage"] == "planning_search_image"
        })
        .expect("1688 search stage should eventually start");

    assert!(
        second_ozon_stage_index < waiting_login_index,
        "all sku rows should complete the Ozon phase before waiting for 1688 login",
    );
    assert!(
        waiting_login_index < first_search_stage_index,
        "1688 login wait must happen before any search-image planning starts",
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
fn run_task_closes_ozon_session_after_1688_session_check_starts() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    create_url_mode_workbook(&excel_path, &[("https://www.ozon.ru/product/3552213003", "SKU-CLOSE-ORDER-001", "200 g")]);

    let order = Arc::new(Mutex::new(Vec::<String>::new()));
    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (health_url, health_handle) = spawn_sidecar_health_server();
    let (session_url, session_handle) = spawn_sidecar_recording_session_server(
        vec![r#"{"success":true,"status":"ready"}"#],
        Arc::clone(&order),
    );
    let (close_url, close_handle) = spawn_sidecar_ozon_close_server(Arc::clone(&order));
    let (search_url, search_handle) = spawn_sidecar_search_server(
        format!(
            r#"{{"success":true,"data":[{{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]}}"#
        ),
        1,
    );
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url);
    std::env::set_var("SIDECAR_OZON_CLOSE_URL", &close_url);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    set_mock_vlm_env(
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("task should finish successfully");
    assert_eq!(summary.status, "completed");

    let recorded = order.lock().expect("order lock").clone();
    let session_index = recorded
        .iter()
        .position(|value| value == "session")
        .expect("session-state should be requested");
    let close_index = recorded
        .iter()
        .position(|value| value == "close")
        .expect("ozon close endpoint should be called");
    assert!(
        session_index < close_index,
        "ozon session should stay open until the 1688 readiness check has started"
    );

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    image_server_handle.join().expect("join image server");
    health_handle.join().expect("join health server");
    session_handle.join().expect("join session server");
    close_handle.join().expect("join close server");
    search_handle.join().expect("join search server");
    resolve_handle.join().expect("join resolve server");
    remove_if_exists(&excel_path);
    remove_if_exists(&excel_path.with_file_name("result.xlsx"));
}

#[test]
fn run_task_finalizes_ozon_not_found_rows_without_entering_1688() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let result_path = excel_path.with_file_name("result.xlsx");
    create_url_mode_workbook(&excel_path, &[("https://www.ozon.ru/product/3552213004", "SKU-NOT-FOUND-001", "200 g")]);

    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        r#"{"success":false,"code":"OZON_SKU_NOT_FOUND","error":"[OZON_SKU_NOT_FOUND] SKU not found on Ozon"}"#.to_string(),
        1,
    );
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    set_mock_vlm_env(r#"[]"#);

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("ozon not-found sku rows should finalize directly without entering 1688");

    assert_eq!(summary.status, "completed");
    assert_eq!(
        summary.result_path.as_deref(),
        Some(result_path.to_string_lossy().as_ref())
    );

    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "Ozon 未找到 SKU");
    assert!(
        row_event_payloads(&sink)
            .iter()
            .all(|payload| payload["stage"] != "planning_search_image"),
        "rows unresolved on Ozon must not enter the 1688 image-search stages"
    );

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    resolve_handle.join().expect("join ozon resolve server");
    remove_if_exists(&excel_path);
    remove_if_exists(&result_path);
}

#[test]
fn run_task_uses_sku_cache_without_calling_ozon_sidecar_again() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let work_dir = make_temp_work_dir("run-task-ozon-url-cache");
    std::fs::create_dir_all(&work_dir).expect("create work dir");
    let excel_path = work_dir.join("input.xlsx");
    create_url_mode_workbook(&excel_path, &[("https://www.ozon.ru/product/3552213005", "SKU-CACHE-001", "200 g")]);

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
    let (resolve_url_1, resolve_handle_1) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url_1);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url_1);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url_1);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url_1);
    set_mock_vlm_env(
        r#"[
          [1],
          [1]
        ]"#,
    );
    set_mock_search_image_plan_env(default_mock_search_image_plan_json());

    let mut first_sink = CollectingSink::default();
    let first_summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut first_sink)
        .expect("first url-mode run should resolve Ozon source and succeed");
    assert_eq!(first_summary.status, "completed");

    clear_sidecar_fixture_env();
    health_handle_1.join().expect("join health server");
    session_handle_1.join().expect("join session server");
    search_handle_1.join().expect("join search server");
    resolve_handle_1.join().expect("join ozon resolve server");

    // Second run: cache is cleared on task start, so the resolve endpoint MUST be called again
    let (health_url_2, health_handle_2) = spawn_sidecar_health_server();
    let (session_url_2, session_handle_2) =
        spawn_sidecar_session_server(vec![r#"{"success":true,"status":"ready"}"#]);
    let (search_url_2, search_handle_2) = spawn_sidecar_search_server(
        format!(
            r#"{{"success":true,"data":[{{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"{candidate_image_url}"}}]}}"#
        ),
        1,
    );
    let (resolve_url_2, resolve_handle_2) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url_2);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url_2);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url_2);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url_2);
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
            .expect("second url-mode run should also succeed after cache is cleared on start");
    assert_eq!(second_summary.status, "completed");
    // Cache is cleared on start, so the second run should call the ozon resolve sidecar again
    assert!(
        second_sink
            .payloads
            .iter()
            .filter(|(name, _)| name == EVENT_ROW_RESULT)
            .map(|(_, payload)| payload)
            .any(|payload| payload["stage"] == "resolving_ozon_product"),
        "second run should call the ozon resolve sidecar since cache is cleared on task start"
    );

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    image_server_handle.join().expect("join image server");
    health_handle_2.join().expect("join health server");
    session_handle_2.join().expect("join session server");
    search_handle_2.join().expect("join search server");
    resolve_handle_2.join().expect("join ozon resolve server");
    remove_dir_if_exists(&cache_root_for_output_anchor(&excel_path));
    remove_if_exists(&excel_path);
    remove_if_exists(&work_dir.join("result.xlsx"));
    remove_dir_if_exists(&work_dir);
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
    let embedded_png = BASE64_STANDARD.encode(build_png_bytes(320, 320));
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
fn run_task_falls_back_to_redownload_when_sidecar_returns_tiny_ozon_image_bytes() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let (ozon_base_url, ozon_handle) = spawn_ozon_antibot_server();
    create_url_mode_workbook(
        &excel_path,
        &[(
            &format!("{ozon_base_url}/product/3552213011"),
            "SKU-OZON-TINY-BYTES",
            "230 g",
        )],
    );

    let (candidate_image_url, image_server_handle) = spawn_image_server();
    let (health_url, health_handle) = spawn_sidecar_health_server();
    let (session_url, session_handle) =
        spawn_sidecar_session_server(vec![r#"{"success":true,"status":"ready"}"#]);
    let tiny_embedded_png = BASE64_STANDARD.encode(build_png_bytes(68, 68));
    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Recovered title","imageUrl":"{candidate_image_url}","imageBase64":"{tiny_embedded_png}"}}}}"#
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
        .expect("tiny sidecar image bytes should fall back to redownloading the ozon image url");

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
fn run_task_pauses_and_skips_row_after_max_ozon_antibot_retries() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let result_path = excel_path.with_file_name("result.xlsx");
    create_url_mode_workbook(&excel_path, &[("https://www.ozon.ru/product/3552213006", "SKU-ANTIBOT-001", "200 g")]);

    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_server(
        r#"{"success":false,"code":"ANTI_BOT_CHALLENGE","error":"[ANTI_BOT_CHALLENGE] Ozon page blocked"}"#.to_string(),
        4,
    );
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    set_mock_vlm_env(r#"[]"#);

    // Background thread: resume gate each time it becomes paused
    let gate_resume_handle = std::thread::spawn(|| {
        for _ in 0..4 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while std::time::Instant::now() < deadline {
                if GLOBAL_RECOVERY_GATE.is_paused() {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    GLOBAL_RECOVERY_GATE.resume();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    });

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("task should complete (skip row) after max anti-bot retries, not terminate");

    assert_eq!(summary.status, "completed");

    // Should have emitted blocking_alert at least once
    assert!(
        sink.payloads.iter().any(|(name, payload)| {
            name == EVENT_BLOCKING_ALERT && payload["code"] == "ANTI_BOT_CHALLENGE"
        }),
        "blocking alert should be emitted when ozon captcha is detected",
    );

    // The row should be finalized (skipped) not left hanging
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert!(
        final_rows[0]["status"].as_str().unwrap().contains("验证"),
        "skipped row status should mention verification failure"
    );

    gate_resume_handle.join().expect("join gate resume thread");
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    resolve_handle.join().expect("join ozon resolve server");
    remove_if_exists(&excel_path);
    remove_if_exists(&result_path);
}

#[test]
fn run_task_finalizes_row_when_ozon_antibot_retry_ends_in_unavailable() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let result_path = excel_path.with_file_name("result.xlsx");
    create_url_mode_workbook(
        &excel_path,
        &[("https://www.ozon.ru/product/3570411009", "SKU-ANTIBOT-UNAVAILABLE", "400 g")],
    );

    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_resolve_sequence_server(vec![
        r#"{"success":false,"code":"ANTI_BOT_CHALLENGE","error":"[ANTI_BOT_CHALLENGE] Ozon page blocked"}"#.to_string(),
        r#"{"success":false,"error":"[OZON_PRODUCT_UNAVAILABLE] Ozon 商品页显示为不可访问或已下架"}"#.to_string(),
    ]);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    set_mock_vlm_env(r#"[]"#);

    let gate_resume_handle = std::thread::spawn(|| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            if GLOBAL_RECOVERY_GATE.is_paused() {
                std::thread::sleep(std::time::Duration::from_millis(50));
                GLOBAL_RECOVERY_GATE.resume();
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    });

    let mut sink = CollectingSink::default();
    let summary = run_task_with_sink(excel_path.to_string_lossy().as_ref(), &mut sink)
        .expect("task should complete and finalize the row after retrying ozon anti-bot");

    assert_eq!(summary.status, "completed");
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "Ozon商品已下架或不可访问");

    gate_resume_handle.join().expect("join gate resume thread");
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    resolve_handle.join().expect("join ozon resolve sequence server");
    remove_if_exists(&excel_path);
    remove_if_exists(&result_path);
}

#[test]
fn run_task_sku_mode_successfully_resolves_ozon_source_before_1688() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    create_url_mode_workbook(&excel_path, &[("https://www.ozon.ru/product/3552213007", "SKU-URL-001", "200 g")]);

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
    clear_sidecar_fixture_env();
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
    // Cache is cleared on task start, so the second run MUST call the resolve sidecar again
    let (resolve_url_2, resolve_handle_2) = spawn_sidecar_ozon_resolve_server(
        format!(
            r#"{{"success":true,"data":{{"title":"Морская верёвочная лестница","imageUrl":"{candidate_image_url}"}}}}"#
        ),
        1,
    );

    std::env::set_var("SIDECAR_HEALTH_URL", &health_url_2);
    std::env::set_var("SIDECAR_SESSION_URL", &session_url_2);
    std::env::set_var("SIDECAR_SEARCH_URL", &search_url_2);
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url_2);
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
            .expect("second run should also succeed after cache is cleared on start");
    assert_eq!(second_summary.status, "completed");
    // Cache is cleared on start, so the second run must call the ozon resolve sidecar again
    assert!(
        second_sink
            .payloads
            .iter()
            .filter(|(name, _)| name == EVENT_ROW_RESULT)
            .map(|(_, payload)| payload)
            .any(|payload| payload["stage"] == "resolving_ozon_product"),
        "second run should call the ozon resolve sidecar since cache is cleared on task start"
    );

    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();
    image_server_handle.join().expect("join image server");
    health_handle_2.join().expect("join health server");
    session_handle_2.join().expect("join session server");
    search_handle_2.join().expect("join search server");
    resolve_handle_2.join().expect("join ozon resolve server");
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
fn run_task_accepts_sku_only_workbook_without_embedded_images() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    create_single_row_workbook(&excel_path);
    let (candidate_image_url, image_server_handle) = spawn_image_server();
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
        .expect("url-mode workbook should resolve ozon source and produce a match");

    assert_eq!(summary.status, "completed");
    let final_rows = final_row_event_payloads(&sink);
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0]["status"], "AI比对成功(主搜索图召回)");

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
        Some("ozon链接".to_string())
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
        Some("https://www.ozon.ru/product/1000001".to_string())
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
