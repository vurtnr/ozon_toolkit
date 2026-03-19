# Ozon URL Source Integration Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Support URL-based Ozon workbooks by resolving each row's first-column Ozon product URL into a title and first main image, then feeding that image into the existing 1688 + VLM matching pipeline while preserving the legacy embedded-image workbook path.

**Architecture:** Add a focused Ozon product resolver module in Rust that fetches and parses detail pages over HTTP, then integrate it as a pre-processing stage before the current search-image/1688/VLM flow. Keep the existing matching core intact, add URL-mode row loading plus legacy fallback, and split exported row output into `处理状态` vs. `AI分析结论` so source-resolution failures never pollute the AI column.

**Tech Stack:** Rust (`reqwest::blocking`, `serde_json`, `regex`, optional `scraper` if needed), existing Tauri desktop backend, existing `rust_xlsxwriter` export path, existing Rust integration tests under `src-tauri/tests/`.

---

## File Map

**Create**

- `src-tauri/src/core/ozon_product.rs`
  - Detect URL-mode rows.
  - Resolve an Ozon product URL into:
    - normalized title
    - first main image URL
    - first main image bytes
    - failure classification
- `src-tauri/tests/ozon_product_resolver_test.rs`
  - HTML fixture / local HTTP tests for URL parsing, unavailable products, missing title, missing image, and invalid URL cases.

**Modify**

- `src-tauri/src/core/mod.rs`
  - Export the new `ozon_product` module.
- `src-tauri/src/commands/run_task.rs`
  - Split raw row loading from resolved task rows.
  - Add URL-mode detection.
  - Add per-row Ozon resolution stage and cache.
  - Keep legacy embedded-image loading as fallback.
  - Split `TaskOutputRow` into:
    - row `status`
    - optional `ai_analysis_conclusion`
  - Add workbook `处理状态` column and keep `AI分析结论` empty for source-resolution failures.
- `src-tauri/tests/run_task_command_test.rs`
  - Add URL-mode pipeline coverage.
  - Update workbook header assertions.
  - Add source-failure export assertions.
- `src-tauri/Cargo.toml`
  - Only if needed for a small HTML selector library such as `scraper`.
- `product_matching_algorithm.md`
  - Update the “input original image” section after implementation so docs reflect URL mode.

**No frontend file changes expected in the first pass**

- Existing monitor UI can already display arbitrary stage/status strings.
- The new stage should flow through the existing `row_result` event pipeline unchanged.

---

## Chunk 1: Ozon Detail Resolver

### Task 1: Add the Ozon resolver module boundary

**Files:**
- Create: `src-tauri/src/core/ozon_product.rs`
- Modify: `src-tauri/src/core/mod.rs`
- Modify: `src-tauri/Cargo.toml` if `scraper` is introduced

- [ ] **Step 1: Write the failing compile-level test import**

Add a new test file that imports these planned items:

```rust
use desktop_app_lib::core::ozon_product::{
    classify_ozon_url_mode,
    resolve_ozon_product,
    OzonProductResolution,
    OzonResolutionFailure,
};
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --test ozon_product_resolver_test -- --nocapture`

Expected: FAIL because `ozon_product` module and symbols do not exist yet.

- [ ] **Step 3: Write the minimal module shell**

Add:

```rust
pub enum OzonResolutionFailure {
    InvalidUrl,
    Unavailable,
    MissingTitle,
    MissingImage,
    FetchFailed(String),
}

pub struct OzonProductResolution {
    pub title: String,
    pub image_url: String,
    pub image_bytes: Vec<u8>,
}

pub fn classify_ozon_url_mode(_value: &str) -> bool { false }

pub fn resolve_ozon_product(
    _client: &reqwest::blocking::Client,
    _product_url: &str,
) -> Result<OzonProductResolution, OzonResolutionFailure> {
    Err(OzonResolutionFailure::InvalidUrl)
}
```

- [ ] **Step 4: Run test to verify it compiles and now fails for behavior**

Run: `cd src-tauri && cargo test --test ozon_product_resolver_test -- --nocapture`

Expected: FAIL on behavioral assertions instead of missing module errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/mod.rs src-tauri/src/core/ozon_product.rs src-tauri/tests/ozon_product_resolver_test.rs src-tauri/Cargo.toml
git commit -m "test: scaffold ozon product resolver"
```

### Task 2: Parse URL-mode and structured Ozon detail payloads

**Files:**
- Modify: `src-tauri/src/core/ozon_product.rs`
- Test: `src-tauri/tests/ozon_product_resolver_test.rs`

- [ ] **Step 1: Write the failing parser tests**

Cover:

```rust
#[test]
fn classify_ozon_url_mode_accepts_ozon_product_urls() {}

#[test]
fn resolve_ozon_product_extracts_title_and_first_main_image_from_structured_html() {}

#[test]
fn resolve_ozon_product_prefers_first_image_only() {}
```

Use local HTML fixtures with:

- embedded product title
- multiple image URLs
- expected first image URL

- [ ] **Step 2: Run only the parser tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test --test ozon_product_resolver_test classify_ozon_url_mode_accepts_ozon_product_urls -- --exact
cargo test --test ozon_product_resolver_test resolve_ozon_product_extracts_title_and_first_main_image_from_structured_html -- --exact
```

Expected: FAIL because parser behavior is not implemented.

- [ ] **Step 3: Implement minimal parser logic**

In `ozon_product.rs`:

- detect Ozon product URLs via exact host + `/product/` path pattern
- extract title from embedded structured data / page-state JSON first
- extract image list from the same structured payload first
- select only the first image
- download image bytes with the existing blocking `Client`

- [ ] **Step 4: Run parser tests to verify they pass**

Run: `cd src-tauri && cargo test --test ozon_product_resolver_test -- --nocapture`

Expected: PASS for the new parser tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/ozon_product.rs src-tauri/tests/ozon_product_resolver_test.rs
git commit -m "feat: parse ozon detail title and first image"
```

### Task 3: Add unavailable and missing-data failure classification

**Files:**
- Modify: `src-tauri/src/core/ozon_product.rs`
- Test: `src-tauri/tests/ozon_product_resolver_test.rs`

- [ ] **Step 1: Write the failing failure-mode tests**

Add tests for:

- unavailable / off-shelf HTML -> `OzonResolutionFailure::Unavailable`
- valid page without title -> `MissingTitle`
- valid page without image -> `MissingImage`
- malformed or unsupported URL -> `InvalidUrl`

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test --test ozon_product_resolver_test resolve_ozon_product_returns_unavailable_for_off_shelf_html -- --exact
```

Expected: FAIL with mismatched error classification.

- [ ] **Step 3: Implement conservative failure classification**

Rules:

- explicit unavailable markers return `Unavailable`
- missing title returns `MissingTitle`
- missing image returns `MissingImage`
- invalid URL returns `InvalidUrl`
- transport / non-200 / decode errors return `FetchFailed(String)`

- [ ] **Step 4: Run the full resolver test file**

Run: `cd src-tauri && cargo test --test ozon_product_resolver_test -- --nocapture`

Expected: PASS with all resolver scenarios green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/core/ozon_product.rs src-tauri/tests/ozon_product_resolver_test.rs
git commit -m "feat: classify ozon source resolution failures"
```

## Chunk 2: Integrate URL Mode Into the Task Pipeline

### Task 4: Split raw Excel rows from resolved task rows

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Write failing tests for URL-mode row loading and legacy fallback**

Add tests covering:

- URL workbook row loads `product_url` from column 1 and `sku` from column 2
- legacy workbook still loads `ozon_name` and embedded image

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_loads_url_mode_rows_from_input_workbook -- --exact
```

Expected: FAIL because `load_task_rows()` still treats column 1 as product name.

- [ ] **Step 3: Introduce a raw row model and loader split**

Refactor `run_task.rs`:

- add a raw input row struct for workbook data
- keep `original_cells`
- store:
  - `product_url`
  - `sku`
  - `legacy_ozon_name`
  - `legacy_image_bytes`

- [ ] **Step 4: Run the targeted row-loading tests**

Run: `cd src-tauri && cargo test --test run_task_command_test run_task_loads_url_mode_rows_from_input_workbook -- --exact`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs
git commit -m "refactor: split raw workbook rows from task rows"
```

### Task 5: Resolve Ozon URL rows before 1688 processing

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Write failing pipeline tests for URL-mode resolution**

Add tests for:

- URL-mode success row enters the existing search pipeline
- source-resolution stage emits `resolving_ozon_product`
- repeated identical URLs use a per-run in-memory cache

- [ ] **Step 2: Run the targeted tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_url_mode_successfully_resolves_ozon_source_before_1688 -- --exact
```

Expected: FAIL because the current pipeline never resolves Ozon URLs.

- [ ] **Step 3: Implement the resolver stage**

In `run_task.rs`:

- instantiate an in-memory `HashMap<String, Result<...>>` cache
- before `planning_search_image`, detect URL mode
- emit row event:
  - stage: `resolving_ozon_product`
  - status: `正在解析 Ozon 商品页`
- resolve title + first image via `core::ozon_product`
- hydrate the normalized downstream row with:
  - `ozon_name`
  - `image_bytes`

- [ ] **Step 4: Run the targeted pipeline tests**

Run: `cd src-tauri && cargo test --test run_task_command_test run_task_url_mode_successfully_resolves_ozon_source_before_1688 -- --exact`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs
git commit -m "feat: resolve ozon url rows before matching"
```

### Task 6: Split row status from AI conclusion

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Write failing tests for source-failure output semantics**

Add tests asserting:

- source-resolution failure rows stop before 1688
- `AI分析结论` is empty for those rows
- failure reason is recorded under a dedicated `处理状态` column

- [ ] **Step 2: Run the targeted export tests and verify they fail**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_leaves_ai_conclusion_empty_for_ozon_source_failures -- --exact
```

Expected: FAIL because current workbook writes all outcomes into `AI分析结论`.

- [ ] **Step 3: Change the output row model**

Refactor `TaskOutputRow` to carry:

- `status`
- `ai_analysis_conclusion: Option<String>`
- existing price/link/image fields

Rules:

- source-resolution failures populate `status` only
- 1688/VLM rows populate both:
  - `status`
  - optional `ai_analysis_conclusion`

- [ ] **Step 4: Run the targeted tests**

Run: `cd src-tauri && cargo test --test run_task_command_test run_task_leaves_ai_conclusion_empty_for_ozon_source_failures -- --exact`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs
git commit -m "feat: separate processing status from ai conclusion"
```

## Chunk 3: Export, Regression Coverage, and Docs

### Task 7: Update workbook schema and export behavior

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Write failing workbook header tests**

Update expected headers to:

- original workbook columns
- `1688成本价`
- `1688链接`
- `处理状态`
- `AI分析结论`
- `图像比对耗时`
- `原图`
- `匹配图`

- [ ] **Step 2: Run the workbook header test and verify it fails**

Run:

```bash
cd src-tauri
cargo test --test run_task_command_test run_task_writes_result_workbook_with_brain_core_columns_and_images -- --exact
```

Expected: FAIL because the current workbook still lacks `处理状态`.

- [ ] **Step 3: Update workbook writing logic**

In `write_result_workbook()`:

- add the `处理状态` column
- write `AI分析结论` only when present
- keep source-failure rows' AI column blank

- [ ] **Step 4: Re-run the workbook test**

Run: `cd src-tauri && cargo test --test run_task_command_test run_task_writes_result_workbook_with_brain_core_columns_and_images -- --exact`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs
git commit -m "feat: add processing status column to result workbook"
```

### Task 8: Run end-to-end regression coverage for both input modes

**Files:**
- Modify: `src-tauri/tests/run_task_command_test.rs`
- Test: `src-tauri/tests/ozon_product_resolver_test.rs`

- [ ] **Step 1: Add final regression cases**

Ensure the suite covers:

- URL-mode success row
- URL-mode unavailable row
- URL-mode missing-image row
- legacy embedded-image success row

- [ ] **Step 2: Run the full Rust regression slice**

Run:

```bash
cd src-tauri
cargo test --test ozon_product_resolver_test
cargo test --test run_task_command_test
```

Expected: PASS with zero failures.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/ozon_product_resolver_test.rs src-tauri/tests/run_task_command_test.rs
git commit -m "test: cover url mode and legacy workbook regressions"
```

### Task 9: Update docs and perform final verification

**Files:**
- Modify: `product_matching_algorithm.md`
- Optionally modify: `docs/superpowers/specs/2026-03-19-ozon-url-source-design.md` only if implementation deviates

- [ ] **Step 1: Update docs for the new source-image entry path**

Document:

- URL-mode first-column Ozon link support
- first main image only
- source-resolution failures stop before 1688
- `处理状态` vs `AI分析结论`

- [ ] **Step 2: Run full verification**

Run:

```bash
cd /Users/jiaoyumin/workspace/ozon_toolkit/desktop_app
bun run build
cd /Users/jiaoyumin/workspace/ozon_toolkit/desktop_app/src-tauri
cargo test --test ozon_product_resolver_test
cargo test --test run_task_command_test
```

Expected:

- frontend build PASS
- resolver tests PASS
- run-task tests PASS

- [ ] **Step 3: Commit**

```bash
git add product_matching_algorithm.md
git commit -m "docs: document ozon url source pipeline"
```

---

## Notes for Execution

- Use @superpowers:test-driven-development for every task.
- Do not replace the existing 1688/VLM core; only change the upstream source-image acquisition path.
- Keep the first version serial. Do not introduce Ozon concurrency.
- If Ozon parsing requires more than one new dependency, stop and reassess before expanding scope.
- Preserve legacy embedded-image support until URL mode is proven stable with real input files.
