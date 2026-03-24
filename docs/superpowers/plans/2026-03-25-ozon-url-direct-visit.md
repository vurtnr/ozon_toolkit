# Ozon URL Direct Visit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace SKU search with direct Ozon product URL navigation, add human behavior simulation, and clear all caches on each task start.

**Architecture:** The Rust backend (`run_task.rs`) reads column 0 as `ozon_url` instead of `ozon_name`, calls the sidecar's existing `/resolve-ozon-product` endpoint instead of `/resolve-ozon-sku`, and clears cache+output at task start. The sidecar (`ozon_session.ts`) adds mouse/scroll simulation after `page.goto`.

**Tech Stack:** Rust (Tauri 2), TypeScript (Bun sidecar), Puppeteer

**Spec:** `docs/superpowers/specs/2026-03-25-ozon-url-direct-visit-design.md`

---

### Task 1: Add `rand` dependency to Rust

**Files:**
- Modify: `src-tauri/Cargo.toml:33` (add `rand` to `[dependencies]`)

- [ ] **Step 1: Add the rand dependency**

Add after the `zip` line in `src-tauri/Cargo.toml`:

```toml
rand = "0.8"
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: add rand dependency for inter-row random delays"
```

---

### Task 2: Add `simulateHumanBrowsing` to `ozon_session.ts`

**Files:**
- Modify: `src-sidecar/src/ozon_session.ts`
- Test: `src-sidecar/src/ozon_session.test.ts`

- [ ] **Step 1: Add `simulateHumanBrowsing` function**

Insert before `resolveOzonProductViaSession` (before line 796) in `src-sidecar/src/ozon_session.ts`:

```typescript
export async function simulateHumanBrowsing(
  page: Page,
  delayFn: (ms: number) => Promise<void>,
): Promise<void> {
  try {
    // Random mouse movements (2-4 points)
    const moveCount = 2 + Math.floor(Math.random() * 3);
    for (let i = 0; i < moveCount; i++) {
      const x = 200 + Math.floor(Math.random() * 800);
      const y = 150 + Math.floor(Math.random() * 500);
      await page.mouse.move(x, y, { steps: 5 + Math.floor(Math.random() * 10) });
      await delayFn(200 + Math.floor(Math.random() * 400));
    }

    // Scroll down 100-400px
    const scrollDown = 100 + Math.floor(Math.random() * 300);
    await page.evaluate((amount: number) => window.scrollBy(0, amount), scrollDown);
    await delayFn(500 + Math.floor(Math.random() * 1000));

    // Sometimes scroll back up a bit (50% chance)
    if (Math.random() > 0.5) {
      const scrollUp = 50 + Math.floor(Math.random() * 150);
      await page.evaluate((amount: number) => window.scrollBy(0, -amount), scrollUp);
      await delayFn(300 + Math.floor(Math.random() * 500));
    }
  } catch {
    // Non-critical — if simulation fails (e.g. page navigated), silently continue
  }
}
```

- [ ] **Step 2: Call `simulateHumanBrowsing` in `resolveOzonProductViaSession`**

In `resolveOzonProductViaSession`, after the `page.goto()` call (after line 810) and before the polling loop (`const deadline = ...` at line 812), insert:

```typescript
  // Simulate human browsing behavior to avoid anti-bot detection
  await simulateHumanBrowsing(page, dependencies.delay);
```

- [ ] **Step 3: Run sidecar tests**

Run: `cd src-sidecar && bun test`
Expected: All 74 tests pass (no behavior change for existing tests)

- [ ] **Step 4: Commit**

```bash
git add src-sidecar/src/ozon_session.ts
git commit -m "feat: add human browsing simulation after Ozon page.goto

Adds mouse movement, scrolling, and random delays to mimic human
browsing behavior and reduce anti-bot detection risk."
```

---

### Task 3: Change Excel parsing — column 0 becomes `ozon_url`

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs:82-95` (TaskRow struct)
- Modify: `src-tauri/src/commands/run_task.rs:560-607` (read_task_workbook parsing)

- [ ] **Step 1: Add `ozon_url` field to `TaskRow` struct**

In `src-tauri/src/commands/run_task.rs`, change the `TaskRow` struct (lines 88-95):

```rust
#[derive(Debug, Clone)]
struct TaskRow {
    excel_row_index: u32,
    ozon_url: String,
    ozon_name: String,
    sku: String,
    original_cells: Vec<String>,
    image_bytes: Option<Vec<u8>>,
}
```

- [ ] **Step 2: Update `load_task_rows` to read column 0 as `ozon_url`**

In the `load_task_rows` function (around line 568), change:

```rust
// Before:
let ozon_name = first_cell;

// After:
let ozon_url = first_cell;
let ozon_name = String::new();
```

And update the `TaskRow` construction (around line 601):

```rust
rows.push(TaskRow {
    excel_row_index: (idx + 1) as u32,
    ozon_url,
    ozon_name,
    sku,
    original_cells,
    image_bytes,
});
```

- [ ] **Step 3: Fix all compilation errors from the new field**

The `ozon_url` field addition will cause compilation errors wherever `TaskRow` is constructed or accessed. Fix each one:

- `resolve_task_row_source` (line 1329): Change the validation from checking `row.sku` to checking `row.ozon_url`:

```rust
fn resolve_task_row_source(
    sink: &mut dyn EventSink,
    row: &TaskRow,
) -> Result<TaskRow, OzonResolutionFailure> {
    if row.image_bytes.is_some() {
        return Ok(row.clone());
    }

    emit_row_stage_event(sink, row, "resolving_ozon_product", "正在访问 Ozon 商品页")
        .map_err(|_| OzonResolutionFailure::FetchFailed("emit resolving event failed".to_string()))?;
    let _ = emit_event(
        sink,
        EVENT_LOG,
        &LogEvent {
            level: "info".to_string(),
            message: format!("正在访问 Ozon 商品页: {}", row.ozon_url),
        },
    );

    if !row.ozon_url.trim().is_empty() {
        Ok(row.clone())
    } else {
        Err(OzonResolutionFailure::FetchFailed(
            "empty ozon url".to_string(),
        ))
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles (some warnings about unused variables are OK for now)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/run_task.rs
git commit -m "refactor: add ozon_url field to TaskRow, read column 0 as URL

Column 0 is now parsed as ozon_url instead of ozon_name.
ozon_name starts empty and will be populated from resolved product page."
```

---

### Task 4: Switch resolution path from SKU to URL

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs` (multiple sections)

- [ ] **Step 1: Change `SidecarOzonResolveRequest` to use `productUrl`**

At line 188-191, change:

```rust
#[derive(Debug, Serialize)]
struct SidecarOzonResolveRequest {
    #[serde(rename = "productUrl")]
    product_url: String,
}
```

- [ ] **Step 2: Change `DEFAULT_SIDECAR_OZON_RESOLVE_URL`**

At line 50, change:

```rust
const DEFAULT_SIDECAR_OZON_RESOLVE_URL: &str = "http://127.0.0.1:8266/resolve-ozon-product";
```

- [ ] **Step 3: Update `resolve_ozon_product_via_sidecar` to take `product_url`**

At line 1414, change the function signature and body:

```rust
fn resolve_ozon_product_via_sidecar(
    client: &Client,
    product_url: &str,
) -> Result<OzonProductResolution, OzonResolutionFailure> {
    let response = client
        .post(sidecar_ozon_resolve_url())
        .json(&SidecarOzonResolveRequest {
            product_url: product_url.to_string(),
        })
        .send()
        // ... rest unchanged
```

- [ ] **Step 4: Update `hydrate_ozon_source_via_browser` — parameter name `sku` → `ozon_url`**

At line 1493, change the parameter:

```rust
fn hydrate_ozon_source_via_browser<F>(
    sink: &mut dyn EventSink,
    client: &Client,
    ozon_url: &str,
    ozon_disk_cache: &OzonSourceCache,
    ozon_session_warmed: &mut bool,
    ensure_browser_ready: &mut F,
) -> Result<OzonProductResolution, OzonResolutionFailure>
```

And the body at line 1517:
```rust
    let resolved = resolve_ozon_product_via_sidecar(client, ozon_url);
    if let Ok(resolution) = &resolved {
        if let Err(error) = ozon_disk_cache.store(ozon_url, resolution) {
```

- [ ] **Step 5: Update `prepare_task_rows_for_execution` — use `ozon_url` instead of `sku` for resolution**

At line 1573, change the condition and cache key:

```rust
        let resolved_row = if !use_mock_candidates
            && validated_row.image_bytes.is_none()
            && !validated_row.ozon_url.trim().is_empty()
        {
            let ozon_url = validated_row.ozon_url.as_str();
            let resolution = if let Some(cached) = ozon_source_cache.get(ozon_url) {
                cached.clone()
            } else {
                let cache_lookup = ozon_disk_cache.lookup(ozon_url);
```

Update all subsequent references in this block from `sku` to `ozon_url`:
- `ozon_source_cache.insert(sku.to_string(), ...)` → `ozon_source_cache.insert(ozon_url.to_string(), ...)`
- `hydrate_ozon_source_via_browser(sink, client, sku, ...)` → `hydrate_ozon_source_via_browser(sink, client, ozon_url, ...)`
- Log messages referencing SKU → reference URL
- Anti-bot retry block: `ozon_source_cache.remove(validated_row.sku.as_str())` → `ozon_source_cache.remove(validated_row.ozon_url.as_str())`
- `validated_row.sku.as_str()` in retry calls → `validated_row.ozon_url.as_str()`

- [ ] **Step 6: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles successfully

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands/run_task.rs
git commit -m "feat: switch Ozon resolution from SKU search to direct URL visit

Uses /resolve-ozon-product endpoint instead of /resolve-ozon-sku.
Cache key is now the product URL instead of SKU."
```

---

### Task 5: Add cache and output clearing on task start

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs:2222-2236` (inside `run_task_inner`)

- [ ] **Step 1: Add clearing logic after `result_path` is resolved**

In `run_task_inner`, after line 2226 (where `result_path` is defined) and before line 2227 (`let task_workbook = load_task_rows(&excel)?;`), insert:

```rust
    // Clear all historical data on each task start
    {
        let cache_root = output_anchor_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".desktop_app_cache");
        if cache_root.exists() {
            let _ = std::fs::remove_dir_all(&cache_root);
        }
        if result_path.exists() {
            let _ = std::fs::remove_file(&result_path);
        }
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cd src-tauri && cargo check`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands/run_task.rs
git commit -m "feat: clear Ozon cache and previous output on each task start

Deletes .desktop_app_cache directory and result.xlsx at the beginning
of every run_task invocation to ensure a clean slate."
```

---

### Task 6: Add random inter-row delay

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs` (in `prepare_task_rows_for_execution` loop)

- [ ] **Step 1: Add `use rand::Rng;` import**

At the top of `src-tauri/src/commands/run_task.rs`, add to the imports:

```rust
use rand::Rng;
```

- [ ] **Step 2: Add random delay before each Ozon URL resolution**

In `prepare_task_rows_for_execution`, just before the `hydrate_ozon_source_via_browser` calls, add a random delay. Insert before the cache lookup block (before `let cache_lookup = ozon_disk_cache.lookup(ozon_url);` inside the `OzonSourceCacheLookup::Miss` arm, and similarly for the Corrupted/Err arms):

Actually, the simplest approach is to add the delay right after the row validation and before the resolution condition. Insert just before line 1573 (`let resolved_row = if !use_mock_candidates`):

```rust
        // Random delay between rows to mimic human browsing pace (3-8 seconds)
        if !use_mock_candidates && validated_row.image_bytes.is_none() && !validated_row.ozon_url.trim().is_empty() {
            let delay_ms = rand::thread_rng().gen_range(3_000u64..=8_000);
            std::thread::sleep(Duration::from_millis(delay_ms));
        }
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cd src-tauri && cargo check`
Expected: compiles successfully

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands/run_task.rs
git commit -m "feat: add random 3-8s delay between Ozon product page visits

Mimics human browsing pace to reduce anti-bot detection risk."
```

---

### Task 7: Update Rust tests for URL-mode changes

**Files:**
- Modify: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Update SKU-mode test helpers to use URL-mode format**

The tests that currently use `create_sku_mode_workbook` need to be converted to `create_url_mode_workbook` since the app no longer supports SKU-mode resolution. For each test that creates a SKU-mode workbook:

- `run_task_emits_ozon_sku_resolution_stage_before_matching` (line 830): Change to URL-mode workbook, update assertions from `resolving_ozon_sku` to `resolving_ozon_product`
- `run_task_batches_ozon_resolution_for_all_skus_before_1688_login_gate` (line 976): Convert to URL-mode, change mock server from `spawn_sidecar_ozon_sku_resolve_server` to `spawn_sidecar_ozon_resolve_server`
- `run_task_closes_ozon_session_after_1688_session_check_starts` (line 1067): Same conversion
- `run_task_finalizes_ozon_not_found_rows_without_entering_1688` (line 1142): Same conversion
- `run_task_uses_sku_cache_without_calling_ozon_sidecar_again` (line 1187): Same conversion, change env var `SIDECAR_OZON_RESOLVE_URL` to point to `/resolve-ozon-product`
- `run_task_pauses_and_skips_row_after_max_ozon_antibot_retries` (line 1576): Same conversion
- `run_task_sku_mode_successfully_resolves_ozon_source_before_1688` (line 1639): Same conversion

Key changes for each:
1. Replace `create_sku_mode_workbook(&excel_path, &[("sample-1", "SKU-001")])` with `create_url_mode_workbook(&excel_path, &[("https://www.ozon.ru/product/3552213000", "SKU-001", "200 g")])`
2. Replace `spawn_sidecar_ozon_sku_resolve_server(...)` with `spawn_sidecar_ozon_resolve_server(...)`
3. Update assertion strings from `resolving_ozon_sku` to `resolving_ozon_product`
4. Update env vars from `SIDECAR_OZON_RESOLVE_URL` pointing to `/resolve-ozon-sku` to `/resolve-ozon-product`

- [ ] **Step 2: Update `create_sample_workbook` and `create_single_row_workbook` to URL format**

These helpers use `title` as column 0 header. Change them to match the new format:

```rust
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
```

- [ ] **Step 3: Run all Rust tests**

Run: `cd src-tauri && cargo test`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tests/run_task_command_test.rs
git commit -m "test: update Rust tests for URL-mode Ozon resolution

Convert all SKU-mode test workbooks to URL-mode format.
Update mock server endpoints from /resolve-ozon-sku to /resolve-ozon-product.
Update assertion strings from resolving_ozon_sku to resolving_ozon_product."
```

---

### Task 8: Run all tests and verify

**Files:** (no changes)

- [ ] **Step 1: Run sidecar tests**

Run: `cd src-sidecar && bun test`
Expected: All 74 tests pass

- [ ] **Step 2: Run Rust tests**

Run: `cd src-tauri && cargo test`
Expected: All tests pass

- [ ] **Step 3: Verify frontend builds**

Run: `bun run build`
Expected: `vue-tsc --noEmit && vite build` succeeds

- [ ] **Step 4: Final commit (if any fixes needed)**

If any test fixes were needed, commit them.
