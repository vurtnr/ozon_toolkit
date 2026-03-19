# Ozon URL Source Integration Design

## Goal

Replace the current "Excel embedded image" primary input path with an `Ozon product URL -> product detail fetch -> first main image` path for URL-based sourcing workbooks such as `/Users/jiaoyumin/Desktop/input.xlsx`.

The desktop app should:

1. Read the first column as an Ozon product URL when the workbook matches the new URL-based format.
2. Visit the Ozon product detail page programmatically without showing a separate Ozon browser window.
3. Extract:
   - Ozon product title
   - the first main product image
   - an availability result (normal vs. unavailable/off-shelf)
4. Use the extracted first main image as the original source image for the existing:
   - search-image generation
   - 1688 image search
   - VLM screening/final review
   - result export
5. Stop processing the row before 1688 if the Ozon product is unavailable or no usable main image can be extracted.

## Input Formats

The app must support two input modes.

### Mode A: URL-based Ozon workbook

Example structure from `/Users/jiaoyumin/Desktop/input.xlsx`:

- Column 1: `ozon链接`
- Column 2: `sku`
- Column 3+: other metadata such as weight

In this mode:

- column 1 is the canonical product URL
- column 2 remains the SKU
- the original image is fetched from the Ozon detail page
- the Ozon title is also fetched from the Ozon detail page

### Mode B: Legacy embedded-image workbook

Existing workbook behavior must remain available as a fallback:

- first column continues to map to the current `ozon_name`
- second column continues to map to `sku`
- images continue to come from Excel embedded media

The loader should prefer Mode A when column 1 clearly matches an Ozon product URL pattern such as `https://www.ozon.ru/product/...`.

## Recommended Architecture

### Chosen approach

Use `HTTP detail-page fetch + structured-data-first parsing`, then feed the resulting title and first main image into the existing Rust pipeline.

### Why this approach

- It preserves the current stable 1688 + VLM pipeline.
- It avoids introducing a second visible browser automation flow for Ozon.
- It isolates the change to the row-loading / source-resolution stage instead of the matching core.

### Rejected alternatives

- Full browser automation for Ozon detail scraping:
  - heavier
  - slower
  - unnecessary for first version
- HTTP first with browser fallback:
  - more robust in theory
  - too much complexity for the first rollout

## End-to-End Flow

### URL mode

1. Read workbook rows.
2. Detect that column 1 contains an Ozon product URL.
3. For each row:
   - emit `resolving_ozon_product`
   - fetch and parse the Ozon detail page
   - resolve title + first main image + availability
4. If resolution succeeds:
   - create the same downstream `TaskRow` shape currently expected by the matching pipeline
   - continue into:
     - search-image planning
     - search-image generation
     - 1688 search
     - VLM screening/final review
     - cheapest result selection
5. If resolution fails:
   - finalize the row immediately
   - do not call 1688
   - do not populate AI analysis conclusion

### Legacy mode

No behavioral change. Existing embedded-image logic continues to work.

## Data Model Changes

### Raw input row

Add a raw row stage before the current `TaskRow`.

Suggested shape:

- `excel_row_index`
- `product_url: Option<String>`
- `sku: String`
- `original_cells: Vec<String>`
- `legacy_ozon_name: Option<String>`
- `legacy_image_bytes: Option<Vec<u8>>`

### Resolved task row

The matching pipeline should still consume a normalized row shape:

- `excel_row_index`
- `product_url: Option<String>`
- `ozon_name: String`
- `sku: String`
- `original_cells: Vec<String>`
- `image_bytes: Option<Vec<u8>>`

In URL mode:

- `ozon_name` comes from the Ozon detail page
- `image_bytes` is the downloaded first main image

In legacy mode:

- values continue to come from Excel data / embedded image extraction

## Ozon Detail Resolution

### Fetch strategy

Use direct HTTP requests from Rust. Do not open a visible Ozon browser window.

### Parse strategy

Use:

1. structured data / embedded page state first
2. HTML metadata / fallback selectors second

Priority outputs:

- product title
- ordered product image list
- availability/off-shelf state

### Image rule

Only the first main product image is used in version 1.

No secondary images, variant images, or video covers are included.

### Availability rule

A row must stop before 1688 if any of the following is true:

- product page is unavailable / removed / off-shelf
- URL resolves to a non-product or error page
- product title cannot be extracted
- no usable main image can be extracted

## Row Status Semantics

Add a new monitor stage:

- `resolving_ozon_product`

Suggested live status:

- `正在解析 Ozon 商品页`

Suggested terminal statuses for source-resolution failures:

- `Ozon商品已下架或不可访问`
- `未解析到Ozon商品主图`
- `未解析到Ozon商品标题`
- `Ozon链接无效`

These statuses are source-resolution outcomes, not AI outcomes.

## Export Rules

For rows that fail during Ozon source resolution:

- do not enter 1688 search
- do not populate `AI分析结论`
- keep 1688 price/link empty
- keep matched image empty
- keep original image empty if no usable image was resolved

`AI分析结论` should only be written for rows that actually passed into the 1688 + VLM stage.

## Performance and Risk Controls

### Processing strategy

Version 1 should resolve Ozon rows serially, matching the current single-task serial model.

### Cache

Add an in-memory cache keyed by `product_url`:

- parsed title
- availability result
- first image bytes

This avoids refetching repeated URLs in the same run.

### Risk controls

- conservative request timeout
- deterministic parsing order
- explicit error classification for diagnostics

Do not introduce Ozon request concurrency in the first version.

## Diagnostics

Diagnostics should distinguish:

- source-resolution failure
- search-image generation failure
- 1688 search / candidate recall failure
- VLM screening / final review failure

If the Ozon detail page cannot be resolved, diagnostics should preserve at least:

- requested URL
- classified failure reason
- whether title was found
- whether image candidates were found

## Testing Plan

### Parser tests

- valid Ozon detail HTML returns title + first main image
- unavailable/off-shelf HTML returns unavailable
- product page with no usable image returns `未解析到Ozon商品主图`
- malformed/invalid URL row returns `Ozon链接无效`

### Pipeline tests

- URL mode success row enters existing matching pipeline
- URL mode failure row stops before 1688 and leaves `AI分析结论` empty
- legacy embedded-image mode still works unchanged

### UI/export tests

- monitor emits `resolving_ozon_product`
- exported workbook shows correct terminal status for source failures
- exported workbook leaves AI conclusion empty for source failures

## Rollout Recommendation

Implement this in two layers:

1. introduce URL-mode row loading and Ozon detail resolution
2. preserve legacy embedded-image mode as fallback

This keeps the migration low-risk while allowing the new `input.xlsx` format to work immediately.
