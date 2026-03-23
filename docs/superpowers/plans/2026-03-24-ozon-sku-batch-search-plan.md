# Ozon SKU Batch Search Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the current URL-mode / per-row Ozon hydration flow with a SKU-only, two-phase pipeline that batches all Ozon source-image acquisition before any 1688 image-search work begins.

**Architecture:** Rust remains the orchestration layer and becomes responsible for a strict two-phase task order: first batch Ozon SKU resolution, then batch 1688 + AI matching. The sidecar owns Ozon browser automation through one persistent Ozon tab, exposes a dedicated SKU-resolution API, and closes the Ozon tab before the 1688 phase starts. Existing 1688 search, AI comparison, diagnostics, and result export behavior remain intact for rows that successfully resolve on Ozon.

**Tech Stack:** Tauri, Rust (`calamine`, existing task orchestrator/events/cache), Bun/TypeScript sidecar, Puppeteer, existing monitor UI and recovery gate.

---

## File Structure

**Primary files to modify**
- Modify: `src-sidecar/src/ozon_session.ts`
  Purpose: replace URL-direct product hydration with SKU search, page classification, first-image extraction, and explicit Ozon tab lifecycle.
- Modify: `src-sidecar/src/ozon_session.test.ts`
  Purpose: lock the new Ozon SKU search behavior with focused unit tests.
- Modify: `src-sidecar/src/server.ts`
  Purpose: expose a dedicated `/resolve-ozon-sku` sidecar API, route requests into the Ozon session module, and close the Ozon tab before 1688 work.
- Modify: `src-sidecar/src/server.test.ts`
  Purpose: verify the new sidecar route contract and page lifecycle helpers.
- Modify: `src-tauri/src/commands/run_task.rs`
  Purpose: parse SKU-only workbooks, batch Ozon resolution first, switch Ozon cache identity to `sku`, and only then enter 1688 execution.
- Modify: `src-tauri/src/core/ozon_cache.rs`
  Purpose: change cache keying from `product_url` to `sku` and retain disk cache semantics.
- Modify: `src-tauri/tests/run_task_command_test.rs`
  Purpose: replace URL-mode orchestration coverage with SKU-mode two-phase execution coverage.
- Modify: `src-tauri/tests/ozon_cache_test.rs`
  Purpose: verify cache lookup/write behavior under SKU identity.
- Modify: `src/views/monitorViewModel.ts`
  Purpose: map new Ozon row statuses into sensible stage presentation.
- Modify: `src/views/__tests__/monitorViewModel.test.ts`
  Purpose: cover the new row-status presentation.

**Files to keep read-only unless required**
- `src-sidecar/src/1688_engine.ts`
  Only touch if Ozon-tab teardown needs a shared browser utility.
- `src/views/MonitorView.vue`
  Avoid structural UI changes unless status rendering clearly requires it.
- `src-tauri/src/events.rs`
  Current event payloads should be sufficient; only touch if a new field becomes unavoidable.

---

## Chunk 1: Freeze SKU-Only Ozon Behavior in Tests

### Task 1: Replace URL-mode assumptions with SKU-mode expectations

**Files:**
- Modify: `src-tauri/tests/run_task_command_test.rs`
- Modify: `src-tauri/tests/ozon_cache_test.rs`
- Modify: `src-sidecar/src/ozon_session.test.ts`
- Modify: `src-sidecar/src/server.test.ts`

- [ ] **Step 1: Write the failing sidecar tests**

Add focused tests in `src-sidecar/src/ozon_session.test.ts` for:

```ts
test("classifies an Ozon not-found error page as unavailable for SKU search", () => {
  expect(classifyOzonSkuSearchSnapshot({
    url: "https://www.ozon.ru/search/?text=SKU-404",
    documentTitle: "Такой страницы не существует",
    title: null,
    imageUrl: null,
    bodyText: "Такой страницы не существует Вернуться на главную",
    hasAntiBotChallenge: false,
    isUnavailable: false,
  })).toBe("not_found");
});

test("extracts the first main image from a resolved product detail page", () => {
  expect(classifyOzonSkuSearchSnapshot({
    url: "https://www.ozon.ru/product/3552213000/",
    documentTitle: "SKU Product",
    title: "SKU Product",
    imageUrl: "https://cdn.ozon.ru/main.jpeg",
    bodyText: "Описание товара",
    hasAntiBotChallenge: false,
    isUnavailable: false,
  })).toBe("resolved");
});
```

- [ ] **Step 2: Write the failing Rust orchestration tests**

Add or rename tests in `src-tauri/tests/run_task_command_test.rs` for:

```rust
#[test]
fn run_task_batches_ozon_resolution_for_all_skus_before_1688_login_gate() { /* ... */ }

#[test]
fn run_task_finalizes_ozon_not_found_rows_without_entering_1688() { /* ... */ }

#[test]
fn run_task_uses_sku_cache_without_calling_ozon_sidecar_again() { /* ... */ }
```

Test fixture shape should switch from `create_url_mode_workbook(...)` to a SKU-only workbook helper:

```rust
fn create_sku_mode_workbook(path: &PathBuf, rows: &[(&str, &str)]) {
    // col0 can remain original merchant name or blank
    // col1 is the sku header and runtime source of truth
}
```

- [ ] **Step 3: Verify the red phase**

Run:

```bash
cd src-sidecar
bun test src/ozon_session.test.ts src/server.test.ts
```

Expected: FAIL because SKU-search-specific helpers and route contract do not exist yet.

Run:

```bash
cd ../src-tauri
cargo test --test run_task_command_test -- --nocapture
cargo test --test ozon_cache_test -- --nocapture
```

Expected: FAIL because the runtime still assumes URL-mode Ozon hydration and cache keying by `product_url`.

- [ ] **Step 4: Commit the test-only contract**

Run:

```bash
git add src-sidecar/src/ozon_session.test.ts src-sidecar/src/server.test.ts src-tauri/tests/run_task_command_test.rs src-tauri/tests/ozon_cache_test.rs
git commit -m "test: lock sku mode ozon batch flow"
```

---

## Chunk 2: Implement Sidecar Ozon SKU Search Session

### Task 2: Add one persistent Ozon tab and a dedicated SKU resolve API

**Files:**
- Modify: `src-sidecar/src/ozon_session.ts`
- Modify: `src-sidecar/src/ozon_session.test.ts`
- Modify: `src-sidecar/src/server.ts`
- Modify: `src-sidecar/src/server.test.ts`

- [ ] **Step 1: Introduce the new Ozon SKU resolve contract**

Add request/response types in `src-sidecar/src/server.ts`:

```ts
interface OzonSkuResolveRequestBody {
  sku?: string;
}

type OzonSkuResolvePayload = {
  title: string;
  imageUrl: string;
  imageBase64?: string;
};
```

The new route should be `POST /resolve-ozon-sku`.

- [ ] **Step 2: Add explicit SKU-search page classification in `ozon_session.ts`**

Define a SKU-search-specific page state:

```ts
type OzonSkuSearchState =
  | "resolved"
  | "not_found"
  | "anti_bot_challenge"
  | "incomplete";
```

Required rules:
- `resolved`: real product detail page with title + first main image
- `not_found`: `div[data-widget="error"]` and text containing `Такой страницы не существует`
- `anti_bot_challenge`: existing blocked-page signals
- `incomplete`: still loading / waiting

- [ ] **Step 3: Implement one-tab Ozon session flow**

Add a sidecar function such as:

```ts
export async function resolveOzonSkuViaSession(
  dependencies: ResolveOzonProductDependencies,
  sku: string,
): Promise<OzonResolvePayload> { /* ... */ }
```

Minimal behavior:
- acquire or reuse one persistent Ozon page
- navigate to `https://www.ozon.ru/`
- wait for ready landing state
- fill the top search input with the SKU
- submit the search
- poll until `resolved`, `not_found`, or `anti_bot_challenge`
- on success, capture title + first main image

- [ ] **Step 4: Add explicit Ozon tab teardown**

In `src-sidecar/src/server.ts`, add a helper invoked from Rust-facing flow:

```ts
async function closeOzonSessionPage(): Promise<void> {
  if (globalOzonPage && !globalOzonPage.isClosed()) {
    await globalOzonPage.close();
  }
  globalOzonPage = null;
}
```

This must run before any 1688-phase login check begins.

- [ ] **Step 5: Run sidecar tests to green**

Run:

```bash
cd src-sidecar
bun test src/ozon_session.test.ts src/server.test.ts
```

Expected: PASS.

- [ ] **Step 6: Build sidecar binary**

Run:

```bash
cd src-sidecar
bun run build:sidecar
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
git add src-sidecar/src/ozon_session.ts src-sidecar/src/ozon_session.test.ts src-sidecar/src/server.ts src-sidecar/src/server.test.ts
git commit -m "feat: add ozon sku search sidecar flow"
```

---

## Chunk 3: Refactor Rust Into a Strict Two-Phase Pipeline

### Task 3: Make workbook parsing SKU-only and batch all Ozon work first

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Modify: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Replace URL-mode row parsing with SKU-only row parsing**

In `load_task_rows(...)`, stop deriving `product_url` from column 0. The runtime source should be the `sku` column only.

Implementation target:

```rust
rows.push(TaskRow {
    excel_row_index: (idx + 1) as u32,
    sku,
    ozon_name: String::new(),
    image_bytes,
    original_cells,
});
```

If workbook-embedded images are no longer part of the source path, do not require them for the main flow.

- [ ] **Step 2: Add a dedicated Ozon batch preparation stage**

Split current preparation into two steps:

```rust
fn prepare_rows_via_ozon_sku_batch(...) -> Result<PreparedTaskRows, String> { /* ... */ }
fn execute_prepared_rows_via_1688(...) -> Result<RunTaskSummary, String> { /* ... */ }
```

`prepare_rows_via_ozon_sku_batch(...)` must:
- iterate all rows once
- emit `正在 Ozon 搜索 SKU`
- resolve source image through the sidecar SKU API or cache
- finalize Ozon misses immediately
- collect only successful rows into `executable_rows`

- [ ] **Step 3: Move the 1688 login gate after the Ozon batch**

The sequence inside `run_task_with_original_source_and_sink_inner(...)` must become:

```rust
emit_task_phase_event(... "resolving_ozon_products", ...)?;
let prepared_rows = prepare_rows_via_ozon_sku_batch(...)?;
close_ozon_sidecar_page(...)?;
emit_task_phase_event(... "waiting_for_1688_login", ...)?;
ensure_browser_ready(&client)?;
wait_for_sidecar_ready_session(sink, &client)?;
emit_task_phase_event(... "running_1688_and_ai", ...)?;
```

There must be no 1688 login gating before Ozon batch completion.

- [ ] **Step 4: Preserve final workbook semantics**

Rows finalized during Ozon phase must still be emitted through:

```rust
emit_final_row_result_event(...)
```

with statuses like:
- `Ozon 未找到 SKU`
- `Ozon 主图抓取失败`

and empty 1688 result fields.

- [ ] **Step 5: Run targeted Rust tests**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_batches_ozon_resolution_for_all_skus_before_1688_login_gate -- --exact --nocapture
cargo test --test run_task_command_test run_task_finalizes_ozon_not_found_rows_without_entering_1688 -- --exact --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs
git commit -m "refactor: batch ozon sku resolution before 1688"
```

---

## Chunk 4: Switch Ozon Disk Cache Identity From URL to SKU

### Task 4: Preserve fast reruns without re-hitting Ozon

**Files:**
- Modify: `src-tauri/src/core/ozon_cache.rs`
- Modify: `src-tauri/src/commands/run_task.rs`
- Modify: `src-tauri/tests/ozon_cache_test.rs`
- Modify: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Write cache contract around SKU identity**

Key functions should accept `sku`:

```rust
pub fn lookup(&self, sku: &str) -> Result<OzonSourceCacheLookup, String> { /* ... */ }
pub fn store(&self, sku: &str, resolution: &OzonProductResolution) -> Result<(), String> { /* ... */ }
```

- [ ] **Step 2: Update runtime cache usage**

Anywhere in `run_task.rs` that currently does:

```rust
ozon_disk_cache.lookup(product_url)
ozon_disk_cache.store(product_url, ...)
```

must switch to:

```rust
ozon_disk_cache.lookup(&row.sku)
ozon_disk_cache.store(&row.sku, ...)
```

- [ ] **Step 3: Green the cache tests**

Run:

```bash
cd src-tauri
cargo test --test ozon_cache_test -- --nocapture
cargo test --test run_task_command_test run_task_uses_sku_cache_without_calling_ozon_sidecar_again -- --exact --nocapture
```

Expected: PASS.

- [ ] **Step 4: Commit**

Run:

```bash
git add src-tauri/src/core/ozon_cache.rs src-tauri/src/commands/run_task.rs src-tauri/tests/ozon_cache_test.rs src-tauri/tests/run_task_command_test.rs
git commit -m "refactor: key ozon cache by sku"
```

---

## Chunk 5: Align Monitor Semantics and Remove Legacy URL-Mode Assumptions

### Task 5: Keep UI status presentation accurate under the new Ozon phase

**Files:**
- Modify: `src/views/monitorViewModel.ts`
- Modify: `src/views/__tests__/monitorViewModel.test.ts`
- Modify: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Extend terminal failure mapping**

Add row-status handling for:

```ts
status.includes("Ozon 未找到 SKU")
status.includes("Ozon 主图抓取失败")
status.includes("已获取 Ozon 主图，等待 1688")
status.includes("正在 Ozon 搜索 SKU")
```

- [ ] **Step 2: Replace or retire stale URL-mode test names**

Tests named around `url_mode` should be renamed or deleted once SKU-mode coverage fully replaces them, for example:
- `run_task_url_mode_successfully_resolves_ozon_source_before_1688`
- `run_task_exports_directly_when_all_ozon_rows_fail_preflight`

The final test suite should describe SKU-mode behavior only.

- [ ] **Step 3: Run UI and Rust tests**

Run:

```bash
bun test src/views/__tests__/monitorViewModel.test.ts
cd src-tauri
cargo test --test run_task_command_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Commit**

Run:

```bash
git add src/views/monitorViewModel.ts src/views/__tests__/monitorViewModel.test.ts src-tauri/tests/run_task_command_test.rs
git commit -m "test: align monitor states with ozon sku flow"
```

---

## Chunk 6: Full Verification and Manual Smoke Test

### Task 6: Verify sidecar, Rust runtime, and desktop UI together

**Files:**
- No new source files expected
- Use: `src-sidecar`, `src-tauri`, and root app commands for verification only

- [ ] **Step 1: Run sidecar verification**

Run:

```bash
cd src-sidecar
bun test
bun run build:sidecar
```

Expected: PASS.

- [ ] **Step 2: Run Rust verification**

Run:

```bash
cd ../src-tauri
cargo test
```

Expected: PASS.

- [ ] **Step 3: Run frontend verification if status mapping changed**

Run:

```bash
cd ..
bun test src/views/__tests__/monitorViewModel.test.ts src/views/__tests__/MonitorView.test.ts
```

Expected: PASS.

- [ ] **Step 4: Manual smoke test**

Run:

```bash
cd /Users/jiaoyumin/workspace/ozon_toolkit/desktop_app/.worktrees/ozon-url-source
bun run tauri dev
```

Manual expected flow:
- select a SKU-only workbook
- Chrome opens Ozon first
- every SKU is searched through the Ozon top bar
- Ozon miss rows finalize directly
- after Ozon batch completes, the Ozon tab closes
- 1688 tab opens or activates
- login is checked only at this point
- successful Ozon rows continue through 1688 + AI matching
- `result.xlsx` is generated in the existing output location

- [ ] **Step 5: Final commit for any verification-only fixture or wording changes**

Run:

```bash
git add .
git commit -m "chore: finalize ozon sku batch search rollout"
```
