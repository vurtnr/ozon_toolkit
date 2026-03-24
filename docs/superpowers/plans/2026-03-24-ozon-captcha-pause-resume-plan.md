# Ozon Captcha Pause-Resume — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When Ozon triggers an anti-bot challenge during SKU search, pause the task (instead of terminating), let the user complete the captcha, then retry the current SKU.

**Architecture:** Replace `return Err(CODE_ANTI_BOT_CHALLENGE)` in the Ozon preflight resolution with a wait-retry loop that polls `GLOBAL_RECOVERY_GATE` until the user clicks "continue". Max 3 retry attempts per row, then skip.

**Tech Stack:** Rust (Tauri commands), cargo test

**Spec:** `docs/superpowers/specs/2026-03-24-ozon-captcha-pause-resume-design.md`

---

### Task 1: Update existing anti-bot test to expect pause-resume behavior

**Files:**
- Modify: `src-tauri/tests/run_task_command_test.rs` (the `run_task_stops_when_browser_assisted_ozon_resolve_remains_blocked` test, around line 1575-1645)

The existing test sets up a mock sidecar that always returns `ANTI_BOT_CHALLENGE`, then asserts the task returns `Err("ANTI_BOT_CHALLENGE")`. With the new behavior, the task should pause (waiting on the gate), so we need to:
1. Resume the gate from a background thread after a short delay
2. The mock sidecar should return `ANTI_BOT_CHALLENGE` for the first 3 requests (max retries), then the task should skip the row and complete successfully

- [ ] **Step 1: Update the test**

Rename the test to `run_task_pauses_and_skips_row_after_max_ozon_antibot_retries` and rewrite it. The mock sidecar should serve at least 4 responses (3 anti-bot retries + buffer), and a background thread should resume the gate after each pause.

In `src-tauri/tests/run_task_command_test.rs`, replace the existing test:

```rust
#[test]
fn run_task_pauses_and_skips_row_after_max_ozon_antibot_retries() {
    let _guard = lock_env();
    GLOBAL_RECOVERY_GATE.resume();
    clear_mock_pipeline_env();
    clear_sidecar_fixture_env();

    let excel_path = make_temp_excel_path();
    let result_path = excel_path.with_file_name("result.xlsx");
    create_sku_mode_workbook(&excel_path, &[("sample-1", "SKU-ANTIBOT-001")]);

    let (resolve_url, resolve_handle) = spawn_sidecar_ozon_sku_resolve_server(
        r#"{"success":false,"code":"ANTI_BOT_CHALLENGE","error":"[ANTI_BOT_CHALLENGE] Ozon page blocked"}"#.to_string(),
        4,
    );
    std::env::set_var("SIDECAR_OZON_RESOLVE_URL", &resolve_url);
    set_mock_vlm_env(r#"[]"#);

    // Background thread: resume gate each time it becomes paused
    let gate_resume_handle = std::thread::spawn(|| {
        for _ in 0..4 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test run_task_command_test run_task_pauses_and_skips_row_after_max_ozon_antibot_retries -- --nocapture`
Expected: FAIL — the current code still does `return Err(CODE_ANTI_BOT_CHALLENGE)` which terminates the task.

- [ ] **Step 3: Commit test**

```bash
git add src-tauri/tests/run_task_command_test.rs
git commit -m "test: add pause-resume test for ozon anti-bot challenge

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Implement wait-retry loop for Ozon anti-bot challenge

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs` (around line 1665-1676)

- [ ] **Step 1: Replace `return Err` with wait-retry loop**

In `src-tauri/src/commands/run_task.rs`, find the block starting at line 1665:

```rust
                    if error == OzonResolutionFailure::AntiBotChallenge {
                        emit_task_phase_event(
                            sink,
                            "waiting_for_ozon_verification",
                            "等待 Ozon 验证",
                            "Ozon 商品页触发验证，Chrome 已打开验证页，完成验证后会自动继续。",
                            true,
                        )?;
                        GLOBAL_RECOVERY_GATE.pause();
                        emit_blocking_alert_if_needed(sink, CODE_ANTI_BOT_CHALLENGE)?;
                        return Err(CODE_ANTI_BOT_CHALLENGE.to_string());
                    }
```

Replace the entire `if error == OzonResolutionFailure::AntiBotChallenge { ... }` block AND the `finalize_preflight_row` + `continue` below it with a combined anti-bot-aware error handling block. The new `Err(error)` arm of the `match resolution` (starting at line 1656) should be:

```rust
                Err(error) => {
                    let _ = emit_event(
                        sink,
                        EVENT_LOG,
                        &LogEvent {
                            level: "warn".to_string(),
                            message: format!("Ozon SKU {} 解析失败: {:?}", validated_row.sku, error),
                        },
                    );
                    if error == OzonResolutionFailure::AntiBotChallenge {
                        const MAX_OZON_ANTIBOT_RETRIES: u32 = 3;
                        let mut antibot_attempts = 0u32;
                        let mut last_error = error;
                        while last_error == OzonResolutionFailure::AntiBotChallenge
                            && antibot_attempts < MAX_OZON_ANTIBOT_RETRIES
                        {
                            antibot_attempts += 1;
                            emit_task_phase_event(
                                sink,
                                "waiting_for_ozon_verification",
                                "等待 Ozon 验证",
                                "Ozon 触发验证，请在 Chrome 中完成滑块验证后点击「已验证，继续执行」。",
                                true,
                            )?;
                            GLOBAL_RECOVERY_GATE.pause();
                            emit_blocking_alert_if_needed(sink, CODE_ANTI_BOT_CHALLENGE)?;

                            // Wait until user clicks "continue"
                            while GLOBAL_RECOVERY_GATE.is_paused() {
                                std::thread::sleep(Duration::from_millis(500));
                            }

                            emit_task_phase_event(
                                sink,
                                "retrying_ozon_resolve",
                                "恢复 Ozon 搜索",
                                &format!(
                                    "用户已确认验证完成，重试 SKU {} (第 {} 次)",
                                    validated_row.sku, antibot_attempts
                                ),
                                false,
                            )?;
                            let _ = emit_event(
                                sink,
                                EVENT_LOG,
                                &LogEvent {
                                    level: "info".to_string(),
                                    message: format!(
                                        "Ozon 验证已恢复，重试 SKU: {} (第 {} 次)",
                                        validated_row.sku, antibot_attempts
                                    ),
                                },
                            );

                            // Remove cached anti-bot result so retry hits sidecar again
                            ozon_source_cache.remove(validated_row.sku.as_str());

                            match hydrate_ozon_source_via_browser(
                                sink,
                                client,
                                validated_row.sku.as_str(),
                                ozon_disk_cache,
                                &mut ozon_session_warmed,
                                ensure_browser_ready,
                            ) {
                                Ok(resolution) => {
                                    ozon_source_cache.insert(
                                        validated_row.sku.clone(),
                                        Ok(resolution.clone()),
                                    );
                                    let mut hydrated = validated_row.clone();
                                    hydrated.ozon_name = resolution.title;
                                    hydrated.image_bytes = Some(resolution.image_bytes);
                                    if hydrated.image_bytes.is_some() || use_mock_candidates {
                                        prepared.executable_rows.push(hydrated);
                                    } else {
                                        finalize_preflight_row(
                                            sink,
                                            &mut prepared,
                                            empty_output_row(&hydrated, "Ozon主图抓取失败"),
                                            total_rows,
                                        )?;
                                    }
                                    // Break out of retry loop on success — move to next row
                                    last_error = OzonResolutionFailure::InvalidUrl; // sentinel: not AntiBotChallenge
                                }
                                Err(retry_error) => {
                                    let _ = emit_event(
                                        sink,
                                        EVENT_LOG,
                                        &LogEvent {
                                            level: "warn".to_string(),
                                            message: format!(
                                                "Ozon SKU {} 重试失败: {:?}",
                                                validated_row.sku, retry_error
                                            ),
                                        },
                                    );
                                    last_error = retry_error;
                                }
                            }
                        }
                        // If we exhausted retries and it's still anti-bot, skip the row
                        if last_error == OzonResolutionFailure::AntiBotChallenge {
                            finalize_preflight_row(
                                sink,
                                &mut prepared,
                                empty_output_row(
                                    &validated_row,
                                    "Ozon 验证失败，已跳过",
                                ),
                                total_rows,
                            )?;
                        }
                        continue;
                    }
                    finalize_preflight_row(
                        sink,
                        &mut prepared,
                        empty_output_row(
                            &validated_row,
                            map_ozon_resolution_failure_to_status(&error).as_str(),
                        ),
                        total_rows,
                    )?;
                    continue;
                }
```

Note: `Duration` is already imported (`use std::time::Duration;`) — verify this import exists at the top of the file. Also, `ozon_source_cache` is a `&mut HashMap` in scope, so `.remove()` works.

- [ ] **Step 2: Run the new test**

Run: `cd src-tauri && cargo test --test run_task_command_test run_task_pauses_and_skips_row_after_max_ozon_antibot_retries -- --nocapture`
Expected: PASS — task completes with skipped row after 3 retries.

- [ ] **Step 3: Run all Rust tests**

Run: `cd src-tauri && cargo test --test run_task_command_test`
Expected: All tests PASS. Note: the old test `run_task_stops_when_browser_assisted_ozon_resolve_remains_blocked` was renamed in Task 1, so it should not conflict.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/run_task.rs
git commit -m "feat: pause and retry on ozon anti-bot challenge instead of terminating task

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Run full test suite and verify

**Files:**
- Read-only verification

- [ ] **Step 1: Run all Rust tests**

Run: `cd src-tauri && cargo test --test run_task_command_test`
Expected: All tests PASS (including the renamed anti-bot test)

- [ ] **Step 2: Run sidecar tests**

Run: `cd src-sidecar && bun test`
Expected: All tests PASS (no sidecar changes were made)

- [ ] **Step 3: Verify no other tests reference the old test name**

Search for the old test name `run_task_stops_when_browser_assisted_ozon_resolve_remains_blocked` in the codebase. It should only appear in the test file and should have been renamed.
