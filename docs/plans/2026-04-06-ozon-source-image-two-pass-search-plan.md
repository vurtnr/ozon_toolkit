# Ozon Source Image Two-Pass Search Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the current "planned search image" default path with a two-pass 1688 image-search flow that starts from the Ozon source image and only performs a full-canvas crop retry when the first-pass crop coverage cannot be confidently proven to already cover the full source image.

**Architecture:** The Rust task runner will continue to orchestrate Ozon resolution, 1688 candidate fetching, and staged AI review, but the search input will pivot from VLM-generated `primary/fallback` images to the resolved Ozon source image. The sidecar will own the two-pass 1688 recall behavior: first search with the raw source image, then inspect the crop UI state, and only if full-image coverage is not confidently established, expand the crop box to the full canvas and trigger a second search. Downstream candidate scraping and AI review will consume only the final pass results.

**Tech Stack:** Rust (`src-tauri`), Bun/TypeScript (`src-sidecar`), Puppeteer, Bun test, Cargo test

---

## Problem Statement

Current step 2 is optimized around a search-image planning pipeline:

- Rust resolves Ozon title + source image
- Rust asks the VLM to plan `primary_bbox` and `fallback_bbox`
- Rust renders `primary` and `fallback` search images
- sidecar uploads those generated files to 1688
- sidecar optionally executes `forceFullCrop`
- orchestrator compares primary and fallback recall quality

This no longer matches the desired product behavior. The desired recall strategy is:

1. Prefer the resolved Ozon source image as the first search input.
2. Let 1688 run its default first-pass image search.
3. After result-page entry, inspect the crop UI state.
4. If the crop box is not clearly covering the entire uploaded image, perform a deterministic full-canvas crop expansion and trigger a second search.
5. Use the latest successful search results as the only candidate set for downstream filtering and staged AI review.

The decision rule is intentionally conservative:

- If the system cannot clearly prove "full-image coverage already exists", it must execute the second-pass full-crop retry.

## Non-Goals

- Do not add automatic captcha solving or anti-bot bypass logic.
- Do not redesign staged AI review prompts in this change.
- Do not immediately delete all search-image planning code; first remove it from the default path and keep cleanup incremental.

## Current Code Hotspots

**Rust orchestration**

- Modify: `src-tauri/src/commands/run_task.rs`
- Current responsibility:
  - writes source image temp files
  - calls VLM search-image planner
  - renders primary/fallback search images
  - runs `orchestrate_match(...)` across two search passes

**Search-image rendering**

- Modify later or leave temporarily unused: `src-tauri/src/core/search_image.rs`

**1688 image search execution**

- Modify: `src-sidecar/src/1688_engine.ts`
- Current responsibility:
  - upload local image file
  - enter result page
  - optionally run full-crop path behind `forceFullCrop`
  - scrape result cards

**1688 endpoint contract**

- Modify: `src-sidecar/src/server.ts`
- Current request body:
  - `imagePath`
  - `forceFullCrop`

## Target Behavior

### User-visible behavior

- Step 2 should say it is searching with the Ozon source image, not "generating search image".
- The first 1688 search should use the Ozon source image file as-is.
- If first-pass crop coverage cannot be confidently verified as full-image coverage, the sidecar should automatically open or reuse the crop UI, expand the crop box to the full canvas, confirm, wait for result refresh, and continue with the refreshed result set.
- Downstream matching should only see the final result set from the sidecar.

### State Machine

```text
resolved Ozon source image
  -> upload source image to 1688
  -> wait for first-pass result page
  -> inspect crop coverage
      -> confidently full-image covered
         -> scrape first-pass results
      -> not confidently full-image covered
         -> expand crop to full canvas
         -> wait for second-pass refresh
         -> scrape second-pass results
```

### Failure policy

- If first-pass result page is never reached: fail with existing image-search entry error.
- If crop coverage cannot be read: treat as "not confidently covered" and attempt full-crop.
- If full-crop expansion is required but cannot be applied: fail with explicit crop-related error; do not silently continue on ambiguous results.
- Existing `LOGIN_REQUIRED` and `ANTI_BOT_CHALLENGE` semantics remain unchanged.

## Design Decisions

### 1. Keep the sidecar request contract path-based

Keep sending a local `imagePath` to the sidecar. Do not introduce `imageUrl` or inline base64 as the primary search request payload in this change.

Reasoning:

- Rust already has validated Ozon source bytes.
- Rust already knows how to materialize temp files.
- Keeping the contract path-based isolates the behavioral change to orchestration and sidecar recall logic.

### 2. Remove `primary/fallback` from the default path

The default path should become single-input, potentially two-pass:

- input: Ozon source image temp file
- search pass count:
  - one pass if coverage is already full
  - two passes if full-crop correction is required

This means the following concepts are no longer meaningful in the default path:

- `primary_search_ms`
- `fallback_search_ms`
- `used_fallback_image`
- statuses that mention "主搜索图召回" or "备用搜索图召回"

These should be replaced by semantics tied to the new flow, such as:

- first-pass only
- second-pass full-crop corrected

### 3. Use conservative coverage detection

Coverage inspection must answer one question only:

- "Can we confidently prove that the crop already covers the whole uploaded image?"

If the answer is anything other than a strong yes, execute full-crop.

This is intentionally asymmetric. False positives are worse than false negatives here because a mistaken "already full" decision suppresses the corrective second search entirely.

## Proposed Sidecar Changes

### New recall API shape inside `1688_engine.ts`

Replace the current boolean gate:

- `forceFullCrop: false` -> scrape directly
- `forceFullCrop: true` -> crop then scrape

With an internal two-pass flow:

1. Enter first-pass result page.
2. Run `inspectCropCoverage(...)`.
3. If coverage is confidently full:
   - return first-pass scraped results
4. Otherwise:
   - open crop dialog if needed
   - expand crop bounds to full canvas
   - confirm crop
   - wait for refreshed results
   - return refreshed scraped results

### New helper responsibilities

Add helpers in `src-sidecar/src/1688_engine.ts`:

- `inspectCropCoverage(resultPage): Promise<"full" | "unknown" | "partial">`
- `ensureFullCanvasCropRecall(resultPage): Promise<"full-crop-applied">`
- `waitForRecallRefresh(resultPage): Promise<void>`

Implementation note:

- `inspectCropCoverage` can reuse the existing cursor-probe and bounds utilities.
- If the crop box bounds cannot be read reliably, return `"unknown"`, not `"full"`.
- `partial` and `unknown` both trigger full-crop.

## Proposed Rust Changes

### `run_task.rs` orchestration changes

Default path should become:

1. Resolve Ozon metadata and source image bytes.
2. Materialize source image to temp file.
3. Send source image path to sidecar.
4. Receive final candidate set from sidecar.
5. Run screening + final AI review on that candidate set.

The following code path should be removed from the default flow:

- `resolve_search_image_plan(...)`
- `generate_search_images(...)`
- `orchestrate_match(...)` with primary/fallback search passes

The surrounding workbook preview logic for the original Ozon image should remain.

### Metrics/status changes

Replace or deprecate fields that assume dual generated search images. Suggested replacements:

- `search_plan_ms` -> remove from default path
- `search_image_render_ms` -> remove from default path
- `primary_search_ms` / `fallback_search_ms` -> replace with `first_search_ms` and optional `second_search_ms`
- `used_fallback_image` -> replace with `used_second_pass_full_crop`

Status text should become:

- `AI比对成功(源图首搜)`
- `AI比对成功(整图重搜纠偏)`

or equivalent copy decided during implementation.

## Testing Strategy

### Sidecar tests

**Files:**
- Modify: `src-sidecar/src/1688_engine.test.ts`

Add coverage for:

- first-pass full coverage -> no crop retry
- unknown coverage -> crop retry runs
- partial coverage -> crop retry runs
- crop retry failure preserves `[FULL_CROP_NOT_APPLIED]`

### Rust tests

**Files:**
- Modify: `src-tauri/tests/run_task_command_test.rs`
- Modify as needed: `src-tauri/tests/search_image_pipeline_test.rs`

Add or update coverage for:

- task runner no longer requires mocked search-image plan for default path success
- candidate fetch uses source image temp file path
- status text reflects first-pass vs corrected second-pass behavior
- existing anti-bot and login recovery behavior still pauses correctly

## Task Breakdown

### Task 1: Lock sidecar two-pass recall behavior

**Files:**
- Modify: `src-sidecar/src/1688_engine.ts`
- Test: `src-sidecar/src/1688_engine.test.ts`

**Step 1: Write the failing tests**

Add tests that express:

- result scraping is returned immediately when coverage inspection returns `full`
- result scraping is delayed until after crop expansion when coverage inspection returns `unknown`
- crop failure still throws `[FULL_CROP_NOT_APPLIED]`

**Step 2: Run test to verify it fails**

Run: `cd src-sidecar && bun test 1688_engine.test.ts`

Expected: existing recall-path assumptions fail because current logic only branches on `forceFullCrop`

**Step 3: Write minimal implementation**

- Replace `executeResultPageRecall(...)` with a two-pass decision flow
- Add a conservative coverage inspection helper
- Reuse existing crop expansion utilities

**Step 4: Run test to verify it passes**

Run: `cd src-sidecar && bun test 1688_engine.test.ts`

Expected: PASS

**Step 5: Commit**

```bash
git add src-sidecar/src/1688_engine.ts src-sidecar/src/1688_engine.test.ts
git commit -m "feat: add two-pass 1688 source-image recall"
```

### Task 2: Move Rust default path to source-image recall

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

**Step 1: Write the failing test**

Add a command test asserting:

- successful run does not require `MOCK_SEARCH_IMAGE_PLAN`
- sidecar candidate fetching is still called and final matching succeeds

**Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test run_task_accepts_absolute_excel_path_and_emits_all_events -- --nocapture`

Expected: FAIL because the current path still requires search-image planning and generated search images

**Step 3: Write minimal implementation**

- remove default-path dependence on `resolve_search_image_plan(...)`
- remove default-path dependence on `generate_search_images(...)`
- call sidecar with the source image temp file path
- preserve original image preview/writeback behavior

**Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test run_task_accepts_absolute_excel_path_and_emits_all_events -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs
git commit -m "feat: use ozon source image for default 1688 recall"
```

### Task 3: Rename statuses and timings to match the new model

**Files:**
- Modify: `src-tauri/src/commands/run_task.rs`
- Test: `src-tauri/tests/run_task_command_test.rs`

**Step 1: Write the failing test**

Add assertions for new status labels and updated timing fields.

**Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test run_task_ -- --nocapture`

Expected: FAIL because current output still refers to generated search-image semantics

**Step 3: Write minimal implementation**

- rename status text
- remove or deprecate `fallback`-specific wording from diagnostics
- map timings to first/second-pass semantics

**Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test run_task_ -- --nocapture`

Expected: PASS

**Step 5: Commit**

```bash
git add src-tauri/src/commands/run_task.rs src-tauri/tests/run_task_command_test.rs
git commit -m "refactor: align recall statuses with two-pass source-image search"
```

### Task 4: Cleanup and regression verification

**Files:**
- Modify if needed: `src-tauri/src/core/search_image.rs`
- Modify if needed: docs referring to "生成搜索图"

**Step 1: Write the failing test or doc assertion**

Identify any remaining default-path tests or docs that still assume generated search images.

**Step 2: Run targeted verification**

Run:

```bash
cd src-sidecar && bun test
cd ../src-tauri && cargo test
```

Expected: targeted recall, recovery, and run-task tests pass

**Step 3: Minimal cleanup**

- mark `search_image` module as non-default-path if still needed
- remove stale logs/messages that contradict the new behavior

**Step 4: Run final verification**

Run:

```bash
cd src-sidecar && bun test
cd ../src-tauri && cargo test
bun run build
```

Expected: PASS

**Step 5: Commit**

```bash
git add src-sidecar src-tauri docs
git commit -m "test: verify two-pass source-image recall flow"
```

## Open Questions Resolved In This Plan

- Should the system always force full-crop immediately?
  - No. First pass uses the raw Ozon source image.

- What if crop coverage cannot be read reliably?
  - Treat as ambiguous and execute the second-pass full-crop retry.

- Which search results feed the AI review pipeline?
  - Only the latest successful pass.

- Should anti-bot handling change?
  - No. Existing detection and recovery remain unchanged.

## Rollout Notes

- Keep old search-image planner code out of the default path first; remove dead code only after tests confirm no hidden dependence remains.
- Preserve request compatibility between Rust and sidecar while changing behavior internally.
- Prefer small commits because this change spans both Rust orchestration and sidecar browser automation.

Plan complete and saved to `docs/plans/2026-04-06-ozon-source-image-two-pass-search-plan.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**
