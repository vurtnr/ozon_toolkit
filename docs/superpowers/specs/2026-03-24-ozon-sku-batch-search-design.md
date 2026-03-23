# Ozon SKU Batch Search Design

## Context

The current desktop flow mixes Ozon source hydration and 1688 image search row by row:

1. Resolve one Ozon row.
2. Immediately switch into the 1688 image-search pipeline for that row.
3. Repeat for the next row.

That design no longer matches the required operator workflow. The input workbook should now be treated as a SKU-only task source. The app must first finish all Ozon SKU lookups and collect all available source images, then close the Ozon browser tab, then enter the existing 1688 image-search and AI comparison flow.

## Goals

- Accept a workbook whose useful input is only the `sku` column.
- Open Chrome and start from `https://www.ozon.ru/`.
- Reuse a single Ozon tab to search every SKU through the top search bar.
- Detect the Ozon "page does not exist" error page and mark that SKU as unresolved.
- Collect the first main image and product title for every SKU that resolves successfully.
- Close the Ozon tab after the Ozon phase completes.
- Only then open or switch to the 1688 tab and run the existing image-search + AI comparison pipeline.
- Preserve the current result workbook export, monitor view, diagnostics, and AI comparison behavior for rows that do reach the 1688 phase.

## Non-Goals

- No support for mixed `product_url` and `sku` input modes in the main flow.
- No attempt to support multiple Ozon result cards per SKU. SKU search is assumed to be unique and to auto-navigate into the product detail page when found.
- No anti-bot bypass logic. If Ozon presents a challenge, the task pauses and waits for manual intervention.
- No change to the current 1688 comparison algorithm in this design.

## Confirmed Business Rules

- The workbook should be treated as SKU-only input.
- The existing `product_url`-driven Ozon source hydration is no longer the primary flow.
- Entering a SKU into the Ozon search bar and submitting should either:
  - auto-navigate into the product detail page if the SKU exists, or
  - render an error page containing `div[data-widget="error"]` with a heading like `Такой страницы не существует` if it does not exist.
- When a SKU is unresolved on Ozon, the app should not use AI to infer the result. The row should be finalized directly as an Ozon-side miss.
- Ozon source caching should be retained, but keyed by `sku` instead of `product_url`.

## User-Facing Flow

### Phase A: Ozon Batch Resolve

1. User selects the workbook and starts the task.
2. The app launches Chrome and opens `https://www.ozon.ru/`.
3. A single Ozon tab is reused for the entire Ozon phase.
4. For each row:
   - read the `sku`
   - enter the SKU into the Ozon top search bar
   - submit the search
   - wait for one of three outcomes:
     - product detail page is loaded
     - not-found error page is loaded
     - anti-bot / restricted page is loaded
5. If a product detail page is reached:
   - capture the resolved product title
   - capture the first main image
   - write the data into the in-memory task row
6. If the not-found page is reached:
   - finalize that row immediately with an Ozon-specific status
   - do not send it into the 1688 phase
7. If Ozon presents a challenge:
   - pause the task
   - keep the Ozon tab visible for manual handling
   - resume after the user clears the challenge

### Phase B: 1688 Image Search

1. After all rows have completed the Ozon phase:
   - close the Ozon tab
   - keep only rows with a resolved source image for execution
2. Open or switch to the 1688 tab.
3. Check login state before starting any 1688 image-search work.
4. For every Ozon-resolved row:
   - run the existing search-image generation flow
   - run the existing 1688 image search flow
   - run the existing AI screening/final review flow
   - emit existing monitor events
   - export existing result columns and image columns

## Architecture

The current split remains valid:

- Rust remains the task orchestrator and state owner.
- The sidecar remains the browser automation worker.

The change is a workflow refactor, not an ownership rewrite.

### Rust Responsibilities

- Parse the SKU workbook.
- Maintain row lifecycle state.
- Drive the two-phase task order.
- Persist and look up Ozon source cache by `sku`.
- Finalize Ozon-miss rows before entering 1688.
- Export the final workbook.
- Emit monitor events and task-phase events.

### Sidecar Responsibilities

- Keep a persistent Ozon tab during the Ozon phase.
- Open Ozon home, operate the top search bar, and classify the resulting page.
- Return either:
  - resolved product title + first main image
  - not-found status
  - anti-bot status
- Close the Ozon tab at the end of the Ozon phase.
- Reuse the existing 1688 browser/session handling for the second phase.

## Data Model Changes

### Task Input Row

The effective runtime input becomes:

- `sku`
- original spreadsheet cells
- optional resolved Ozon title
- optional resolved Ozon image bytes

`product_url` should be removed from the primary execution path. Compatibility helpers may remain temporarily during migration, but the main task runner should no longer depend on URL-mode branching.

### Ozon Cache Key

Change cache identity from:

- `product_url -> OzonProductResolution`

to:

- `sku -> OzonProductResolution`

This keeps repeated runs fast while reducing repeat traffic to Ozon.

## Browser and Tab Lifecycle

### Ozon Phase

- Create or reuse one sidecar-managed Ozon page.
- Always begin from Ozon home.
- For each SKU:
  - return to a stable Ozon home/search-ready state
  - fill the search field
  - submit the search
  - inspect the destination page

The sidecar must not open one Ozon tab per row.

### Transition

- Once the Ozon phase is complete, explicitly close the Ozon page.
- Only then create or focus the 1688 page.

### 1688 Phase

- Reuse the existing 1688 browser logic.
- Preserve the current login gate and manual recovery behavior.

## Page Classification Rules

### Ozon Search Success

Treat the page as resolved when:

- the page is a real product detail page
- a valid product title is present
- a valid first main image is present

### Ozon Search Miss

Treat the page as a not-found miss when:

- the page contains `div[data-widget="error"]`
- the visible heading or body contains `Такой страницы не существует`

Rows in this state are finalized without AI inference.

### Ozon Challenge

Treat the page as blocked when anti-bot or restricted signals appear. The existing manual recovery model stays in place:

- emit a blocking event
- pause task execution
- wait for resume

## Status Model

### Task Phases

- `validating_runtime`
- `resolving_ozon_products`
- `waiting_for_ozon_verification`
- `waiting_for_1688_login`
- `running_1688_and_ai`
- `exporting_results`

### Row-Level States

Add or rename row statuses to reflect the two-phase design:

- `正在 Ozon 搜索 SKU`
- `Ozon 未找到 SKU`
- `Ozon 主图抓取失败`
- `已获取 Ozon 主图，等待 1688`

Rows that enter the 1688 phase continue using the current downstream statuses.

## Error Handling

### Ozon Miss

- Final row status should clearly indicate the SKU was not found on Ozon.
- The row should not enter search-image planning or 1688.

### Ozon Image Failure

- If the product detail page opens but the main image cannot be captured, finalize the row directly with an Ozon image failure status.

### Ozon Verification

- Pause the task and keep the Ozon tab open.
- Resume from the current Ozon phase after manual clearance.

### 1688 Login

- Do not check 1688 login before the Ozon phase is complete.
- Perform the existing login gate only once the task is ready to enter the 1688 phase.

## Export Behavior

The result workbook continues to be generated exactly as today:

- same output location
- same result file naming
- same result columns
- same embedded image columns

Rows finalized in the Ozon phase should still appear in the final workbook with:

- original row metadata
- Ozon-specific final status
- empty 1688 result fields

## Testing Strategy

### Sidecar Tests

- resolves a SKU search that auto-navigates into a product detail page
- classifies the Ozon not-found error page correctly
- keeps one persistent Ozon page across multiple SKU requests
- closes the Ozon page before the 1688 phase starts

### Rust Tests

- workbook loading accepts SKU-only input and no longer requires URL mode
- task preparation performs a full Ozon batch pass before any 1688 work begins
- Ozon misses are finalized immediately and excluded from the executable 1688 set
- Ozon cache is keyed by `sku`
- 1688 login gating happens only after the Ozon batch phase completes

## Migration Notes

- Existing URL-mode tests and helpers will need either removal or narrowing into legacy-only coverage.
- The main run-task tests should be rewritten around SKU-only spreadsheets.
- The monitor UI should continue to work from emitted events; it should not need structural changes beyond consuming the new status strings.

## Recommended Implementation Order

1. Add failing tests for SKU-only workbook loading and two-phase execution order.
2. Add failing sidecar tests for Ozon SKU search success and not-found classification.
3. Implement sidecar SKU search API and persistent Ozon page lifecycle.
4. Refactor Rust task preparation into a full Ozon batch stage.
5. Switch Ozon cache identity from `product_url` to `sku`.
6. Re-enable the existing 1688 + AI pipeline only for Ozon-resolved rows.
7. Verify export and monitor behavior end to end.
