# Ozon Anti-Bot Challenge: Pause-Resume Flow

## Problem

When searching Ozon by SKU, the Ozon site may trigger a slider captcha (anti-bot challenge). Currently, when this happens:

1. The sidecar detects `anti_bot_challenge` and returns the error to Rust
2. Rust emits a `blocking_alert` event and pauses the `GLOBAL_RECOVERY_GATE`
3. The frontend shows a "风控验证提醒" dialog with a "已验证，继续执行" button
4. But Rust also **terminates the entire task** with `return Err(CODE_ANTI_BOT_CHALLENGE)`

The user sees the alert, completes the captcha in Chrome, clicks "continue" — but the task has already ended. They must restart from scratch.

## Expected Behavior

- Task pauses when Ozon captcha is detected, showing the blocking alert
- User completes the captcha in Chrome, then clicks "已验证，继续执行" in the app
- Task resumes from the **current SKU row** (retries the same SKU)
- If the captcha triggers again after retry, repeat the pause-resume cycle (max 3 attempts per row)
- After 3 failed attempts, skip the row and continue to the next

## Design

### What Already Works (No Changes Needed)

- **Frontend `BlockingAlert.vue`**: Already handles `ANTI_BOT_CHALLENGE` code — shows the dialog, "已验证，继续执行" button calls `resume_after_challenge`
- **Frontend `blockingAlert.ts`**: `isResumeActionRequired` returns `true` for `ANTI_BOT_CHALLENGE`
- **Rust `recovery.rs`**: `GLOBAL_RECOVERY_GATE`, `blocking_alert_for_code`, all alert codes
- **Rust `resume_after_challenge` command**: Calls `GLOBAL_RECOVERY_GATE.resume()`
- **Sidecar `ozon_session.ts`**: Correctly detects and reports `anti_bot_challenge` state

### What Changes (Rust Only)

**File**: `src-tauri/src/commands/run_task.rs`

**Location**: The Ozon SKU preflight resolution loop, where `OzonResolutionFailure::AntiBotChallenge` is handled (around the code that currently does `GLOBAL_RECOVERY_GATE.pause()` + `return Err`).

**Change**: Replace `return Err(CODE_ANTI_BOT_CHALLENGE)` with a wait-retry loop:

```
When OzonResolutionFailure::AntiBotChallenge:
  1. emit task_phase("等待 Ozon 验证", ..., blocking: true)
  2. GLOBAL_RECOVERY_GATE.pause()
  3. emit_blocking_alert(ANTI_BOT_CHALLENGE)
  4. WAIT: loop { sleep 500ms } until GLOBAL_RECOVERY_GATE.is_paused() == false
  5. emit task_phase("恢复 Ozon 搜索", ..., blocking: false)
  6. Retry the current row's Ozon SKU resolution (call sidecar again)
  7. If anti_bot again → increment attempt counter, go to step 1
  8. If attempt counter > 3 → skip this row, finalize as "Ozon 验证失败，已跳过"
  9. If success → continue normal flow with resolved data
```

There are two locations in `run_task.rs` where `OzonResolutionFailure::AntiBotChallenge` triggers task termination. Both need the same wait-retry treatment:

1. **Preflight SKU resolution** (around line 1669-1675): During the `resolve_task_row_source` → `resolve_ozon_sku_via_sidecar` path
2. **Search stage** (around line 2456-2461): During 1688 image search when `fetch_sidecar_candidates` returns `ANTI_BOT_CHALLENGE`

The second location is for 1688 anti-bot, not Ozon. Only location 1 needs modification for this feature. Location 2 can be left as-is (1688 anti-bot already works differently).

### Data Flow

```
User starts task
  → Rust iterates Excel rows
    → For each SKU row, call sidecar POST /resolve-ozon-sku
      → Sidecar opens Ozon, searches SKU
        → Ozon shows captcha
      → Sidecar returns {success: false, code: "ANTI_BOT_CHALLENGE"}
    → Rust receives ANTI_BOT_CHALLENGE
    → Rust emits blocking_alert event to frontend
    → Frontend shows "风控验证提醒" dialog
    → Rust enters wait loop (polling GLOBAL_RECOVERY_GATE every 500ms)

    ... user completes captcha in Chrome ...
    ... user clicks "已验证，继续执行" in app ...

    → Frontend calls invoke("resume_after_challenge")
    → Rust: GLOBAL_RECOVERY_GATE.resume()
    → Wait loop exits
    → Rust retries the same SKU: call sidecar POST /resolve-ozon-sku again
    → Sidecar finds page no longer has captcha → proceeds with search
    → Success → continue to next row
```

## Files Changed

| File | Change |
|------|--------|
| `src-tauri/src/commands/run_task.rs` | Replace `return Err(ANTI_BOT_CHALLENGE)` in Ozon preflight with wait-retry loop |

## Out of Scope

- Frontend changes (BlockingAlert already handles this code)
- Sidecar changes (already correctly detects and reports anti_bot_challenge)
- 1688 anti-bot flow changes (separate mechanism)
