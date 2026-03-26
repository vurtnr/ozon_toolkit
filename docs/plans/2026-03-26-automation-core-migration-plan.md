# Desktop Automation Core Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Migrate `desktop_app` from a Rust-orchestrated HTTP sidecar architecture to a Tauri shell plus internal TypeScript automation core with a managed browser runtime, while keeping current user-visible behavior stable throughout the transition.

**Architecture:** Keep Tauri and the existing Vue UI. Introduce a new `automation/` TypeScript package as the long-term business core, first behind a compatibility worker, then as the owner of browser lifecycle, pipeline orchestration, LLM/image logic, and Excel I/O. Replace Rust-to-sidecar HTTP calls with process IPC, then replace the system Chrome dependency with an app-managed Chromium runtime.

**Tech Stack:** Tauri 2, Rust 2021, Vue 3, TypeScript ESM, Node-compatible worker runtime, Puppeteer-based browser automation, JSON-line IPC over stdio, existing GitHub Actions packaging.

---

## Requirements Summary

### Functional Requirements

- Keep the current desktop product shape on macOS and Windows.
- Preserve existing user flows:
  - select an Excel file
  - run the task
  - surface row-level progress and blocking alerts
  - wait for manual login/challenge resolution
  - export a final Excel workbook
- Preserve current site behavior support:
  - Ozon source resolution
  - 1688 image search
  - DashScope-backed image planning/comparison
  - final result export with row monitoring
- Move business logic toward one language, with Rust left as a thin shell.
- Remove the hard dependency on a locally installed Chrome in the final state.

### Non-Functional Requirements

- Do not break the currently working desktop flow during migration.
- Keep rollback easy at every stage.
- Keep the browser visible for login and captcha handling.
- Avoid introducing more platform-specific divergence between macOS and Windows.
- Eliminate fixed-port fragility from the current sidecar model.

### Constraints

- Do not rewrite the product into a new app or a new desktop framework.
- Do not rewrite browser automation into Rust.
- Do not mix major architecture changes in the same phase.
- Preserve current frontend event names where possible.

## Target Architecture

```text
Vue UI
  ->
Tauri Shell (Rust)
  ->
Automation Worker Host (Rust runtime adapter)
  ->
Automation Core (TypeScript package)
  ->
Browser Runtime Manager
  ->
App-managed Chromium + persisted profiles
```

### Final Responsibility Split

- `src/`: Vue UI only
- `src-tauri/`: windowing, file dialogs, worker process management, app lifecycle
- `automation/`: all business logic, browser logic, LLM orchestration, Excel read/write
- bundled browser resources: application-managed Chromium runtime

## Key Decisions

### Decision 1: Keep Tauri, do not start a greenfield rewrite

- Chosen because the current app shell, packaging, and frontend are already usable.
- Avoids revalidating Windows/macOS packaging, install flow, and event wiring from scratch.
- Trade-off: Tauri remains a permanent Rust shell, so the system cannot be literally single-language end to end.

### Decision 2: Unify business logic in TypeScript, not Rust

- Chosen because browser automation, DOM work, anti-bot iteration, and Puppeteer-based workflows are already in TS.
- Avoids a high-risk Rust rewrite of site-specific logic.
- Trade-off: a Rust shell still exists for Tauri integration.

### Decision 3: Replace HTTP sidecar with stdio-based worker IPC

- Chosen to eliminate fixed-port failures, health endpoint fragility, and shutdown desynchronization.
- Trade-off: requires a custom message protocol and process lifecycle handling.

### Decision 4: Centralize browser ownership before changing the browser runtime

- Chosen because current instability mostly comes from scattered browser/tab ownership.
- Trade-off: browser runtime replacement is deferred until later.

### Decision 5: Ship an app-managed browser runtime in the final stage

- Chosen so users no longer need a preinstalled Chrome.
- Trade-off: larger installers and stricter packaging/versioning requirements.

## Risks And Mitigations

- Risk: regressions during multi-stage migration.
  - Mitigation: freeze a baseline and add phase-specific tests before each refactor.
- Risk: worker protocol churn breaks UI progress handling.
  - Mitigation: keep frontend event names stable until the migration is complete.
- Risk: browser lifecycle regressions on Windows.
  - Mitigation: centralize page/tab ownership and add explicit Windows-focused tests.
- Risk: packaged browser size and startup complexity.
  - Mitigation: defer bundling until the architecture is already stable.
- Risk: mixed old/new paths live too long.
  - Mitigation: define explicit deletion criteria in the final cleanup phase.

## Reference Files

- Current Rust orchestration: `src-tauri/src/commands/run_task.rs`
- Current Rust domain modules: `src-tauri/src/core/`
- Current Tauri app entry: `src-tauri/src/lib.rs`
- Current TS browser sidecar: `src-sidecar/src/server.ts`
- Current 1688 automation: `src-sidecar/src/1688_engine.ts`
- Current Ozon automation: `src-sidecar/src/ozon_session.ts`
- Current frontend runner: `src/composables/useTaskRunner.ts`
- Current frontend event sink: `src/composables/useTaskEvents.ts`

## Task 1: Freeze The Baseline And Add Migration Safety Rails

**Files:**
- Create: `docs/plans/2026-03-26-automation-core-migration-plan.md`
- Create: `docs/plans/2026-03-26-automation-core-test-matrix.md`
- Create: `src-tauri/tests/runtime_baseline_test.rs`
- Modify: `src-tauri/tests/`

**Step 1: Write the baseline test matrix document**

Capture the manual and automated acceptance matrix:

- macOS happy path
- Windows happy path
- 1688 login required
- captcha/manual challenge required
- Ozon source unavailable
- Ozon multi-product page
- result workbook export

**Step 2: Add failing Rust baseline coverage**

Create a thin regression suite that locks today’s contract:

```rust
#[test]
fn run_task_emits_progress_and_done_events() {}

#[test]
fn run_task_surfaces_blocking_alert_when_login_is_required() {}

#[test]
fn run_task_keeps_row_order_in_final_output() {}
```

**Step 3: Run the targeted tests and record the baseline**

Run:

```bash
cd src-tauri
cargo test --test runtime_baseline_test
```

Expected: either PASS if the baseline is already covered, or FAIL with clear missing-contract assertions that will be fixed before refactoring proceeds.

**Step 4: Save manual verification notes**

Record the exact manual run steps for macOS and Windows in `docs/plans/2026-03-26-automation-core-test-matrix.md`.

**Step 5: Commit**

```bash
git add docs/plans/2026-03-26-automation-core-test-matrix.md src-tauri/tests/runtime_baseline_test.rs
git commit -m "test: lock desktop automation migration baseline"
```

## Task 2: Extract A Rust Runtime Layer From `run_task`

**Files:**
- Create: `src-tauri/src/runtime/mod.rs`
- Create: `src-tauri/src/runtime/protocol.rs`
- Create: `src-tauri/src/runtime/process_manager.rs`
- Create: `src-tauri/src/runtime/worker_client.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/runtime_baseline_test.rs`

**Step 1: Write the failing Rust tests for the new seam**

Add tests that assert `run_task.rs` no longer directly owns sidecar transport details:

```rust
#[test]
fn worker_client_owns_transport_contract() {}

#[test]
fn process_manager_owns_sidecar_lifecycle() {}
```

**Step 2: Run the targeted tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test --test runtime_baseline_test worker_client_owns_transport_contract -- --exact
cargo test --test runtime_baseline_test process_manager_owns_sidecar_lifecycle -- --exact
```

Expected: FAIL because `run_task.rs` still contains endpoint constants, HTTP structs, and process ownership.

**Step 3: Move transport structs and process ownership into `runtime/`**

Implement:

- `runtime/protocol.rs`: request/response structs and runtime error enums
- `runtime/process_manager.rs`: sidecar child ownership and shutdown
- `runtime/worker_client.rs`: temporary HTTP-backed worker adapter

**Step 4: Shrink `run_task.rs`**

Keep only:

- command entry points
- settings loading
- calls into `runtime::worker_client`
- calls into existing Rust `core/*`
- event emission

**Step 5: Run Rust verification**

Run:

```bash
cd src-tauri
cargo test
```

Expected: PASS. User-visible behavior must remain unchanged.

**Step 6: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/runtime src-tauri/src/commands/run_task.rs src-tauri/tests/runtime_baseline_test.rs
git commit -m "refactor: extract desktop runtime layer"
```

## Task 3: Introduce The TypeScript Automation Worker Package

**Files:**
- Create: `automation/package.json`
- Create: `automation/tsconfig.json`
- Create: `automation/src/shared/protocol.ts`
- Create: `automation/src/worker/index.ts`
- Create: `automation/src/worker/dispatcher.ts`
- Create: `automation/src/worker/task_runtime.ts`
- Create: `automation/src/worker/__tests__/dispatcher.test.ts`
- Modify: `package.json`
- Modify: `.gitignore`

**Step 1: Create the automation package skeleton**

Define a standalone TS package for the future business core. It should not depend on the Vue frontend bundle.

**Step 2: Write the failing worker protocol tests**

Add tests for the first protocol:

```ts
it("accepts start_task and shutdown messages", () => {})
it("serializes waiting_for_login and task_done messages", () => {})
```

**Step 3: Run the worker tests and verify they fail**

Run:

```bash
cd automation
npm test
```

Expected: FAIL because the package and dispatcher do not exist yet.

**Step 4: Implement the minimal worker shell**

Create:

- JSON-line message envelope types in `shared/protocol.ts`
- stdin/stdout loop in `worker/index.ts`
- message dispatch in `worker/dispatcher.ts`
- in-memory single-task state in `worker/task_runtime.ts`

**Step 5: Re-run worker tests**

Run:

```bash
cd automation
npm test
```

Expected: PASS for protocol serialization and single-task dispatch tests.

**Step 6: Commit**

```bash
git add automation package.json .gitignore
git commit -m "feat: add automation worker package skeleton"
```

## Task 4: Replace HTTP Sidecar Calls With Worker IPC

**Files:**
- Modify: `src-tauri/src/runtime/protocol.rs`
- Modify: `src-tauri/src/runtime/process_manager.rs`
- Modify: `src-tauri/src/runtime/worker_client.rs`
- Modify: `src-tauri/src/commands/run_task.rs`
- Modify: `automation/src/shared/protocol.ts`
- Modify: `automation/src/worker/index.ts`
- Modify: `automation/src/worker/dispatcher.ts`
- Create: `src-tauri/tests/worker_ipc_test.rs`

**Step 1: Write the failing IPC integration tests**

Add tests for:

```rust
#[test]
fn rust_can_start_the_worker_and_receive_task_done() {}

#[test]
fn rust_receives_waiting_for_login_without_using_http() {}
```

**Step 2: Run the targeted Rust tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test --test worker_ipc_test
```

Expected: FAIL because the Rust runtime still expects HTTP transport.

**Step 3: Implement stdio JSON-line IPC**

Replace temporary HTTP transport with:

- worker child process startup in `process_manager.rs`
- line-oriented stdin writes in `worker_client.rs`
- stdout reader loop that maps worker messages into Rust events

**Step 4: Keep the old browser code behind an adapter**

Do not rewrite browser logic yet. The worker may still proxy to existing browser code in the next task.

**Step 5: Run verification**

Run:

```bash
cd src-tauri
cargo test --test worker_ipc_test
cd ../automation
npm test
```

Expected: PASS. No fixed port should be required.

**Step 6: Commit**

```bash
git add src-tauri/src/runtime src-tauri/src/commands/run_task.rs src-tauri/tests/worker_ipc_test.rs automation/src
git commit -m "refactor: switch desktop runtime to worker ipc"
```

## Task 5: Centralize Browser Lifecycle In The Automation Core

**Files:**
- Create: `automation/src/browser/browser_manager.ts`
- Create: `automation/src/browser/profile_manager.ts`
- Create: `automation/src/browser/session_guard.ts`
- Create: `automation/src/browser/page_pool.ts`
- Create: `automation/src/browser/platform.ts`
- Create: `automation/src/browser/ozon_runtime.ts`
- Create: `automation/src/browser/alibaba_runtime.ts`
- Create: `automation/src/browser/__tests__/session_guard.test.ts`
- Create: `automation/src/browser/__tests__/page_pool.test.ts`
- Modify: `automation/src/worker/task_runtime.ts`
- Modify: `src-sidecar/src/1688_engine.ts`
- Modify: `src-sidecar/src/ozon_session.ts`
- Modify: `src-sidecar/src/chrome-path.ts`

**Step 1: Write failing browser ownership tests**

Add tests that pin the new ownership rules:

```ts
it("only browser_manager creates and closes browser instances", () => {})
it("page_pool reuses pages and closes stale tabs", () => {})
it("session_guard returns login_required before search begins", () => {})
```

**Step 2: Run browser unit tests and verify they fail**

Run:

```bash
cd automation
npm test -- browser
```

Expected: FAIL because browser lifecycle is still scattered.

**Step 3: Implement the browser core**

Create one owner for:

- browser launch/connect/close
- profile path resolution
- per-site page allocation
- login and anti-bot gating
- page reuse and cleanup

**Step 4: Wrap the legacy site logic**

Refactor `src-sidecar/src/1688_engine.ts` and `src-sidecar/src/ozon_session.ts` so they operate on provided pages/sessions instead of owning browser startup.

**Step 5: Run verification**

Run:

```bash
cd automation
npm test -- browser
cd ../src-tauri
cargo test
```

Expected: PASS. Manual checks should confirm:

- login gate blocks work before execution
- Windows no longer leaks tabs per row
- app shutdown closes worker-managed browser resources

**Step 6: Commit**

```bash
git add automation/src/browser automation/src/worker/task_runtime.ts src-sidecar/src/1688_engine.ts src-sidecar/src/ozon_session.ts src-sidecar/src/chrome-path.ts
git commit -m "refactor: centralize browser lifecycle"
```

## Task 6: Migrate Pipeline, Matcher, And LLM Logic To TypeScript

**Files:**
- Create: `automation/src/domain/types.ts`
- Create: `automation/src/pipeline/orchestrator.ts`
- Create: `automation/src/pipeline/matcher.ts`
- Create: `automation/src/pipeline/__tests__/orchestrator.test.ts`
- Create: `automation/src/llm/dashscope_client.ts`
- Create: `automation/src/llm/search_image.ts`
- Create: `automation/src/llm/__tests__/search_image.test.ts`
- Create: `automation/src/cache/ozon_source_cache.ts`
- Modify: `automation/src/worker/dispatcher.ts`
- Modify: `automation/src/worker/task_runtime.ts`
- Modify: `src-tauri/src/commands/run_task.rs`
- Reference: `src-tauri/src/core/`

**Step 1: Write failing pipeline tests**

Cover the core business contract:

```ts
it("selects the final candidate from ranked matches", () => {})
it("emits no-match reasons when no candidate survives review", () => {})
it("falls back correctly when source image planning fails", () => {})
```

**Step 2: Run the targeted automation tests and verify they fail**

Run:

```bash
cd automation
npm test -- pipeline llm
```

Expected: FAIL because the logic still lives in Rust.

**Step 3: Port the domain model and pipeline**

Mirror the existing Rust behavior, not a redesigned algorithm:

- candidate and row result types
- orchestration flow
- matching and final selection rules
- search image generation calls
- DashScope request/response mapping
- Ozon source cache behavior

**Step 4: Thin the Rust command layer**

Change `run_task.rs` so it no longer owns orchestration or matching decisions. It should only:

- validate the command input
- start the worker
- relay events
- return summary data

**Step 5: Run verification**

Run:

```bash
cd automation
npm test -- pipeline llm
cd ../src-tauri
cargo test
```

Expected: PASS. The worker now owns business decisions; Rust remains a shell.

**Step 6: Commit**

```bash
git add automation/src/domain automation/src/pipeline automation/src/llm automation/src/cache automation/src/worker src-tauri/src/commands/run_task.rs
git commit -m "feat: migrate automation pipeline to typescript"
```

## Task 7: Migrate Excel Input And Result Export To TypeScript

**Files:**
- Create: `automation/src/excel/input_reader.ts`
- Create: `automation/src/excel/result_writer.ts`
- Create: `automation/src/excel/__tests__/result_writer.test.ts`
- Modify: `automation/src/worker/dispatcher.ts`
- Modify: `automation/src/worker/task_runtime.ts`
- Modify: `src-tauri/src/commands/upload.rs`
- Modify: `src-tauri/src/commands/run_task.rs`
- Reference: `src-tauri/src/core/excel.rs`

**Step 1: Write failing Excel contract tests**

Cover:

```ts
it("reads the current workbook input contract", () => {})
it("writes result.xlsx with the current column order", () => {})
it("embeds original and matched image columns when available", () => {})
```

**Step 2: Run the Excel tests and verify they fail**

Run:

```bash
cd automation
npm test -- excel
```

Expected: FAIL because Excel I/O is still Rust-owned.

**Step 3: Port Excel input/output logic**

Implement TS readers/writers that preserve:

- current row ordering
- current workbook column contract
- image columns
- result path behavior

**Step 4: Reduce Rust to file handoff**

Leave `upload.rs` responsible for desktop-side file selection/upload only. Remove result workbook construction from Rust.

**Step 5: Run verification**

Run:

```bash
cd automation
npm test -- excel
cd ../src-tauri
cargo test
```

Expected: PASS. Final workbook generation now lives in TS.

**Step 6: Commit**

```bash
git add automation/src/excel src-tauri/src/commands/upload.rs src-tauri/src/commands/run_task.rs
git commit -m "feat: migrate excel io to typescript"
```

## Task 8: Bundle And Resolve An App-Managed Browser Runtime

**Files:**
- Create: `automation/src/browser/runtime_resolver.ts`
- Create: `automation/src/browser/runtime_installer.ts`
- Create: `automation/src/browser/runtime_manifest.ts`
- Create: `automation/src/browser/__tests__/runtime_resolver.test.ts`
- Create: `src-tauri/resources/browser/.gitkeep`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/build.rs`
- Modify: `automation/src/browser/browser_manager.ts`
- Modify: `.github/workflows/`

**Step 1: Write failing browser-runtime tests**

Add tests for:

```ts
it("resolves the bundled runtime path for the current platform", () => {})
it("creates or repairs the runtime install directory on first launch", () => {})
```

**Step 2: Run the automation browser-runtime tests and verify they fail**

Run:

```bash
cd automation
npm test -- runtime
```

Expected: FAIL because the app still depends on system Chrome.

**Step 3: Add bundled runtime resolution**

Implement:

- runtime manifest format
- per-platform executable resolution
- first-run install/extract flow
- persistent profile directories under app data

**Step 4: Update packaging**

Wire browser resources into Tauri packaging and GitHub Actions so Windows/macOS installers include the managed runtime.

**Step 5: Run verification**

Run:

```bash
cd automation
npm test -- runtime
cd ../src-tauri
cargo test
```

Manual verification:

- clean macOS machine without Chrome
- clean Windows machine without Chrome
- login persists across app restarts

**Step 6: Commit**

```bash
git add automation/src/browser src-tauri/tauri.conf.json src-tauri/build.rs .github/workflows
git commit -m "feat: bundle managed browser runtime"
```

## Task 9: Remove Legacy Sidecar Paths And Finalize The Thin-Shell Architecture

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/runtime/`
- Modify: `src-tauri/src/commands/run_task.rs`
- Modify: `src-sidecar/package.json`
- Delete: `src-sidecar/src/server.ts`
- Delete: legacy HTTP-sidecar-only files once fully unused
- Create: `docs/plans/2026-03-26-automation-core-cutover-checklist.md`

**Step 1: Write the failing cutover checklist**

Document the criteria for deleting the old path:

- no fixed-port runtime dependency
- no HTTP sidecar endpoint constants in Rust
- no direct browser launch from legacy modules
- no user-visible regression in export and monitoring

**Step 2: Remove the legacy entrypoints**

Delete or archive HTTP-only sidecar entry files after proving they are unused.

**Step 3: Update docs and CI**

Ensure developer startup, packaging, and test docs match the new architecture.

**Step 4: Run full verification**

Run:

```bash
cd automation
npm test
cd ../src-tauri
cargo test
cd ..
npm run build
```

Manual verification:

- macOS end-to-end run
- Windows end-to-end run
- app close cleans worker/browser
- result workbook opens with expected data

**Step 5: Commit**

```bash
git add docs/plans/2026-03-26-automation-core-cutover-checklist.md src-tauri src-sidecar automation
git commit -m "chore: remove legacy sidecar path"
```

## Acceptance Checklist

- `run_task.rs` is a thin command layer.
- Rust no longer depends on fixed-port HTTP sidecar calls.
- TS worker owns browser lifecycle.
- TS worker owns orchestration, matcher, LLM, and Excel I/O.
- The app can run on a clean macOS/Windows machine without a local Chrome install.
- Login, captcha, row monitoring, and result export remain functional.

## Recommended Execution Order

1. Task 1
2. Task 2
3. Task 3
4. Task 4
5. Task 5
6. Task 6
7. Task 7
8. Task 8
9. Task 9

## Notes For Implementation

- Use a dedicated worktree for execution.
- Keep one commit per task or subtask boundary.
- Do not merge phases. Finish verification for one phase before starting the next.
- Preserve existing behavior first; improve design second.
