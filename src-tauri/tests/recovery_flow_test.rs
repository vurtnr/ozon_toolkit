use std::path::PathBuf;
use std::sync::Mutex;

use desktop_app_lib::commands::run_task::run_task_with_sink;
use desktop_app_lib::events::{EventSink, EVENT_BLOCKING_ALERT};
use desktop_app_lib::recovery::{
    blocking_alert_for_code, CODE_ANTI_BOT_CHALLENGE, CODE_CHROME_NOT_FOUND, CODE_LOGIN_REQUIRED,
    CODE_RESUME_REQUIRED, GLOBAL_RECOVERY_GATE,
};
use rust_xlsxwriter::Workbook;

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

    workbook.save(path).expect("save workbook");
}

fn temp_excel_path(file_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos(),
        file_name
    ))
}

#[test]
fn chrome_not_found_maps_to_blocking_alert_message() {
    let alert = blocking_alert_for_code(CODE_CHROME_NOT_FOUND).expect("alert should exist");
    assert!(alert.blocking);
    assert!(alert.message.contains("未能自动检测到 Chrome 浏览器"));
}

#[test]
fn login_required_maps_to_blocking_alert_message() {
    let alert = blocking_alert_for_code(CODE_LOGIN_REQUIRED).expect("alert should exist");
    assert!(alert.blocking);
    assert!(alert.message.contains("当前 1688 未登录"));
    assert_eq!(alert.action_label, None);
}

#[test]
fn anti_bot_challenge_pauses_and_resume_unblocks_queue() {
    let _guard = ENV_LOCK.lock().expect("env lock should acquire");
    GLOBAL_RECOVERY_GATE.resume();

    let anti_bot_file = temp_excel_path("anti-bot.xlsx");
    create_sample_workbook(&anti_bot_file);

    let mut sink = CollectingSink::default();
    let err = run_task_with_sink(anti_bot_file.to_string_lossy().as_ref(), &mut sink)
        .expect_err("anti bot should block task");
    assert_eq!(err, CODE_ANTI_BOT_CHALLENGE);
    assert!(sink.events.contains(&EVENT_BLOCKING_ALERT.to_string()));
    assert!(GLOBAL_RECOVERY_GATE.is_paused());

    let normal_file = temp_excel_path("normal.xlsx");
    create_sample_workbook(&normal_file);

    let mut paused_sink = CollectingSink::default();
    let paused_err = run_task_with_sink(normal_file.to_string_lossy().as_ref(), &mut paused_sink)
        .expect_err("queue should remain paused");
    assert_eq!(paused_err, CODE_RESUME_REQUIRED);

    GLOBAL_RECOVERY_GATE.resume();

    std::env::set_var(
        "RUN_TASK_MOCK_CANDIDATES_JSON",
        r#"[{"title":"sample","price":"¥12.34","itemUrl":"https://detail.1688.com/offer/1.html","imageUrl":"https://img.1688.com/1.jpg"}]"#,
    );
    let mut resumed_sink = CollectingSink::default();
    run_task_with_sink(normal_file.to_string_lossy().as_ref(), &mut resumed_sink)
        .expect("task should run after resume");
    std::env::remove_var("RUN_TASK_MOCK_CANDIDATES_JSON");

    let _ = std::fs::remove_file(anti_bot_file);
    let _ = std::fs::remove_file(normal_file);
}
