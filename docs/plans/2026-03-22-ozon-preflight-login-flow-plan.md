# Ozon Preflight Login-Gated Flow Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Reorder the desktop runtime so URL-mode Ozon rows are resolved headlessly before any Chrome/1688 startup, launch sidecar only when executable rows remain, block all 1688 work behind confirmed login readiness, and surface the task phase clearly in the monitor UI.

**Architecture:** Split the current `run_task` backend into a preflight stage and an execution stage. The preflight stage validates local prerequisites and resolves Ozon product title + first image for every URL row without touching sidecar; the execution stage launches sidecar only when needed, waits for 1688 session readiness, and then runs the existing serial 1688 + VLM pipeline only for executable rows. Add a first-class task-phase event so the frontend can explain “why Chrome has not opened yet” and “why the task is waiting”.

**Tech Stack:** Rust/Tauri backend, existing Rust integration tests under `src-tauri/tests/`, Bun/Vue frontend, `bun:test`, existing desktop sidecar session-state endpoints.

---

## Reference

- Approved design: `docs/superpowers/specs/2026-03-22-ozon-preflight-login-flow-design.md`

## Task 1: Lock the new orchestration contract with backend tests

**Files:**
- Modify: `src-tauri/tests/run_task_command_test.rs`
- Modify: `src-tauri/src/commands/run_task.rs`

**Step 1: Write the failing orchestration tests**

Add tests that pin the required runtime order:

```rust
#[test]
fn run_task_exports_directly_when_all_ozon_rows_fail_preflight() {}

#[test]
fn run_task_resolves_ozon_rows_before_any_1688_search_stage() {}

#[test]
fn run_task_does_not_emit_search_stage_before_login_becomes_ready() {}

#[test]
fn run_task_keeps_source_failures_and_executable_rows_in_the_same_export() {}
```

Test intent:

- all rows fail Ozon resolution:
  - summary is `completed`
  - final workbook is exported
  - no row event ever reaches `planning_search_image`
- mixed workbook:
  - source-failure rows finalize early
  - executable rows continue to search stages later
- login gate:
  - while sidecar session fixture returns `login_required`, no row event reaches `planning_search_image` or `searching_1688_primary`
  - after fixture flips to `ready`, execution continues

**Step 2: Run the targeted Rust tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_exports_directly_when_all_ozon_rows_fail_preflight -- --exact
cargo test --test run_task_command_test run_task_resolves_ozon_rows_before_any_1688_search_stage -- --exact
cargo test --test run_task_command_test run_task_does_not_emit_search_stage_before_login_becomes_ready -- --exact
```

Expected:

- first two tests fail because preflight is still inside the row loop after sidecar/session startup
- login-gate test fails because search stages can start before the new explicit orchestration boundary exists

**Step 3: Add the minimal test seam needed by the backend**

In `run_task.rs`, add the smallest `pub(crate)` or `#[cfg(test)]` seam required for tests to drive orchestration without a real Tauri window, for example:

```rust
struct PreparedTaskRows {
    total_rows: u32,
    executable_rows: Vec<TaskRow>,
    finalized_rows: Vec<TaskOutputRow>,
}

fn prepare_task_rows_for_execution(...) -> Result<PreparedTaskRows, String> { ... }
```

Do not implement full behavior yet. Only add enough structure for the new tests to compile and fail on behavior.

**Step 4: Re-run the targeted tests**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_exports_directly_when_all_ozon_rows_fail_preflight -- --exact
```

Expected: tests compile and fail on assertions, not on missing symbols.

**Step 5: Commit**

```bash
git add src-tauri/tests/run_task_command_test.rs src-tauri/src/commands/run_task.rs
git commit -m "test: pin ozon preflight orchestration"
```

## Task 2: Split preflight resolution from browser execution

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

**Step 1: Write one more failing test for the “all failed, no browser work” path**

Add an assertion-heavy test that proves preflight-only completion:

```rust
#[test]
fn run_task_finishes_without_browser_execution_when_no_executable_rows_remain() {
    // assert final rows exist
    // assert progress reaches total
    // assert no planning_search_image/searching_1688_primary events were emitted
}
```

**Step 2: Run the single targeted test and verify it fails**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_finishes_without_browser_execution_when_no_executable_rows_remain -- --exact
```

Expected: FAIL because current code still validates sidecar/session before row preflight completes.

**Step 3: Implement the preflight/execution split**

Refactor `run_task.rs` into two explicit stages:

```rust
fn validate_preflight_prerequisites(task_rows: &[TaskRow], use_mock_candidates: bool) -> Result<(), String> {
    // no sidecar ping here
}

fn prepare_task_rows_for_execution(
    sink: &mut dyn EventSink,
    client: &Client,
    workbook: &TaskWorkbook,
) -> Result<PreparedTaskRows, String> {
    // emit queued + resolving_ozon_product
    // build executable_rows
    // build finalized_rows for source failures
}

fn run_prepared_rows(
    sink: &mut dyn EventSink,
    prepared: PreparedTaskRows,
    ...
) -> Result<RunTaskSummary, String> {
    // existing 1688 + VLM loop for executable rows only
}
```

Required behavior:

- `validate_preflight_prerequisites` checks only:
  - absolute Excel path
  - workbook readability
  - at least one embedded image or product URL
  - `DASHSCOPE_API_KEY`
  - sidecar binary presence if real browser execution may be needed later
- `prepare_task_rows_for_execution`:
  - resolves every URL row headlessly
  - caches Ozon resolutions by URL
  - returns two groups:
    - `finalized_rows`
    - `executable_rows`
- if `executable_rows.is_empty()`:
  - skip sidecar startup entirely
  - write `result.xlsx` directly from finalized rows
  - emit `task_done`
- if executable rows remain:
  - keep finalized source-failure rows in memory so they are included in final export together with later successful/failed executed rows

**Step 4: Run the backend orchestration tests**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_exports_directly_when_all_ozon_rows_fail_preflight -- --exact
cargo test --test run_task_command_test run_task_keeps_source_failures_and_executable_rows_in_the_same_export -- --exact
```

Expected: PASS. The tests should show that source failures finish before browser work and still appear in the exported workbook.

**Step 5: Commit**

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs
git commit -m "feat: preflight ozon rows before browser execution"
```

## Task 3: Gate browser execution behind 1688 login readiness

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`
- Reference only: `../src-sidecar/src/server.ts`

**Step 1: Write the failing login-gate test**

Add a test that simulates session state changing from `login_required` to `ready`:

```rust
#[test]
fn run_task_blocks_row_execution_until_1688_session_is_ready() {
    // fixture server returns login_required twice, then ready
    // assert no planning_search_image/searching_1688_primary stage before ready
    // assert blocking alert/log emitted while waiting
}
```

Add one more conservative test:

```rust
#[test]
fn run_task_starts_sidecar_only_after_preflight_finds_executable_rows() {}
```

The second test can use a lightweight hook or counter around the launcher path so the call order is observable in Rust tests.

**Step 2: Run the targeted tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_blocks_row_execution_until_1688_session_is_ready -- --exact
cargo test --test run_task_command_test run_task_starts_sidecar_only_after_preflight_finds_executable_rows -- --exact
```

Expected: FAIL because the current orchestration launches sidecar before preflight and does not expose the new ordering boundary clearly enough for the tests to pass.

**Step 3: Implement conditional launcher ordering and explicit login gate**

Refactor the blocking command path:

```rust
#[tauri::command]
pub async fn run_task(...) -> Result<RunTaskSummary, String> {
    // 1. resolve api key
    // 2. prepare task rows first
    // 3. if none executable -> export immediately
    // 4. ensure_sidecar_running(...)
    // 5. wait_for_sidecar_ready_session(...)
    // 6. execute prepared rows
}
```

Required behavior:

- `ensure_sidecar_running(...)` is called only after preflight proves there is work for 1688
- `wait_for_sidecar_ready_session(...)` stays ahead of any `planning_search_image` / `searching_1688_primary` event
- if session state is `login_required` or `anti_bot_challenge`:
  - emit blocking alert
  - remain paused until state becomes `ready`
  - do not consume row execution time yet

If the cleanest implementation needs one orchestration helper, add:

```rust
fn run_prepared_task_with_launcher(
    prepared: PreparedTaskRows,
    launcher: &mut dyn FnMut() -> Result<(), String>,
    ...
) -> Result<RunTaskSummary, String> { ... }
```

This is preferred over duplicating logic across command and test paths.

**Step 4: Run the login/order test slice**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_resolves_ozon_rows_before_any_1688_search_stage -- --exact
cargo test --test run_task_command_test run_task_blocks_row_execution_until_1688_session_is_ready -- --exact
cargo test --test run_task_command_test run_task_starts_sidecar_only_after_preflight_finds_executable_rows -- --exact
```

Expected: PASS. Search-stage events should appear only after preflight and login readiness.

**Step 5: Commit**

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs
git commit -m "feat: gate 1688 execution behind login readiness"
```

## Task 4: Surface task-level phases in the monitor UI

**Files:**
- Modify: `src-tauri/src/events.rs`
- Modify: `src-tauri/src/commands/run_task.rs`
- Modify: `src/types/events.ts`
- Modify: `src/composables/useTaskEvents.ts`
- Modify: `src/views/monitorViewModel.ts`
- Modify: `src/views/MonitorView.vue`
- Test: `src/views/__tests__/monitorViewModel.test.ts`
- Test: `src/views/__tests__/MonitorView.test.ts`

**Step 1: Write the failing frontend tests**

Add tests for a new task-level phase model:

```ts
test("summarizes monitor board with task-level phase before row execution starts", () => {})

test("renders resolving ozon phase without requiring active row data", () => {})

test("renders waiting for login phase when blocking alert is active", () => {})
```

Add one backend assertion in Rust if needed to pin the emitted phase names:

```rust
assert_eq!(phase.payload["phase"], "resolving_ozon_products");
```

**Step 2: Run the targeted frontend tests and verify they fail**

Run:

```bash
cd ..
bun test src/views/__tests__/monitorViewModel.test.ts
bun test src/views/__tests__/MonitorView.test.ts
```

Expected: FAIL because `MonitorState` has no task-phase field and the view currently derives “current stage” only from row activity.

**Step 3: Implement the task-phase event end-to-end**

Add a dedicated event and payload:

```rust
pub const EVENT_TASK_PHASE: &str = "task_phase";

#[derive(Debug, Clone, Serialize)]
pub struct TaskPhaseEvent {
    pub phase: String,
    pub label: String,
    pub detail: String,
    pub blocking: bool,
}
```

Add matching TypeScript types:

```ts
export interface TaskPhaseEventPayload {
  phase: string;
  label: string;
  detail: string;
  blocking: boolean;
}
```

Emit phases from `run_task.rs` at minimum:

- `validating_runtime`
- `resolving_ozon_products`
- `waiting_for_1688_login`
- `running_1688_and_ai`
- `exporting_results`

UI behavior:

- when no active row exists yet, the header and focus card should still explain the current task phase
- while `waiting_for_1688_login`, prioritize the task phase over row-stage wording
- keep the existing row table unchanged except for better empty-state copy during Ozon preflight

**Step 4: Run frontend and Rust verification for this slice**

Run:

```bash
bun test src/views/__tests__/monitorViewModel.test.ts
bun test src/views/__tests__/MonitorView.test.ts
cd src-tauri
cargo test --test run_task_command_test run_task_resolves_ozon_rows_before_any_1688_search_stage -- --exact
```

Expected: PASS. The monitor should now expose the current task phase even before the first executable row enters 1688.

**Step 5: Commit**

```bash
git add src-tauri/src/events.rs src-tauri/src/commands/run_task.rs src/types/events.ts src/composables/useTaskEvents.ts src/views/monitorViewModel.ts src/views/MonitorView.vue src/views/__tests__/monitorViewModel.test.ts src/views/__tests__/MonitorView.test.ts
git commit -m "feat: show task-level preflight and login phases"
```

## Task 5: Full regression verification and handoff

**Files:**
- Modify if needed: `src-tauri/tests/run_task_command_test.rs`
- Modify if needed: `src/views/__tests__/MonitorView.test.ts`
- Optional note update: `docs/superpowers/specs/2026-03-22-ozon-preflight-login-flow-design.md`

**Step 1: Add any missing regression tests found while wiring the flow**

Typical additions:

```rust
#[test]
fn run_task_preserves_row_order_when_preflight_failures_finish_before_executable_rows() {}

#[test]
fn run_task_exports_result_even_when_every_row_stops_in_preflight() {}
```

**Step 2: Run the automated verification suite**

Run:

```bash
cd src-tauri
cargo test --test ozon_product_resolver_test
cargo test --test run_task_command_test
cd ..
bun test src/views/__tests__/monitorViewModel.test.ts
bun test src/views/__tests__/MonitorView.test.ts
bun run build
```

Expected:

- all Rust integration tests pass
- frontend monitor tests pass
- frontend production build passes

**Step 3: Manual verification on desktop**

Run:

```bash
cd desktop_app
bun run tauri dev
```

Verify these flows manually:

1. URL workbook, all rows off-shelf:
   - no Chrome launch
   - monitor shows `解析 Ozon 商品源`
   - task finishes and exports `result.xlsx`
2. URL workbook, at least one executable row, 1688 logged out:
   - preflight completes first
   - then Chrome launches
   - monitor enters `等待 1688 登录`
   - no row reaches `1688 首轮搜索` before login
3. URL workbook, already logged in:
   - preflight completes
   - Chrome launches once
   - row stages continue into `生成搜索图` and `1688 首轮搜索`
4. Mixed workbook:
   - source-failure rows finalize early in the table
   - executable rows continue later
   - final Excel contains both groups

**Step 4: If the manual behavior matches, make the final commit**

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/src/events.rs src-tauri/tests/run_task_command_test.rs src/types/events.ts src/composables/useTaskEvents.ts src/views/monitorViewModel.ts src/views/MonitorView.vue src/views/__tests__/monitorViewModel.test.ts src/views/__tests__/MonitorView.test.ts docs/superpowers/specs/2026-03-22-ozon-preflight-login-flow-design.md
git commit -m "feat: align desktop flow with ozon preflight and login gating"
```

**Step 5: Handoff notes**

Before merging, confirm:

- `run_task` never launches sidecar when `executable_rows.is_empty()`
- no 1688 search stage is emitted before login readiness
- Ozon fetching remains headless with no visible Ozon browser window
- the UI explains the current task phase even when no row is active yet
