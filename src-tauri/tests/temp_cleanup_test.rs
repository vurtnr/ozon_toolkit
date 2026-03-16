use std::path::PathBuf;

use desktop_app_lib::lifecycle::cleanup::run_with_task_guard;

fn temp_dir_path() -> PathBuf {
    let unique = format!(
        "desktop-app-temp-cleanup-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time should be monotonic")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

#[test]
fn cleanup_removes_stale_files_before_run_and_after_success() {
    let temp_dir = temp_dir_path();
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");
    std::fs::write(temp_dir.join("stale.txt"), b"stale").expect("write stale file");

    run_with_task_guard(temp_dir.clone(), || {
        assert!(temp_dir.exists());
        assert!(!temp_dir.join("stale.txt").exists());
        std::fs::write(temp_dir.join("new.txt"), b"new").expect("write file in task");
        Ok(())
    })
    .expect("task should succeed");

    assert!(!temp_dir.exists());
}

#[test]
fn cleanup_runs_even_when_task_fails() {
    let temp_dir = temp_dir_path();

    let result: Result<(), String> = run_with_task_guard(temp_dir.clone(), || {
        std::fs::write(temp_dir.join("inflight.txt"), b"data")
            .map_err(|e| format!("write failed: {e}"))?;
        Err("forced failure".to_string())
    });

    assert!(result.is_err());
    assert!(!temp_dir.exists());
}
