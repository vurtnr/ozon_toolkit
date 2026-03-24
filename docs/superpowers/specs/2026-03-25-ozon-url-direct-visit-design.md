# Design: Ozon URL Direct Visit — Replace SKU Search with Direct Product Page Navigation

## Problem

The current flow resolves Ozon products by typing SKUs into the Ozon search bar via `keyboard.type()`. This is fragile — Ozon's autocomplete/React hydration intercepts keystrokes, causing character drops (e.g. "3552213000" → "2213000"). Even the JS-value-setter fix is a workaround for an inherently brittle approach.

The user's Excel now provides direct Ozon product URLs in column 1 ("ozon链接"). We should navigate directly to these URLs instead of searching by SKU.

## Requirements

1. Read `ozon_url` from Excel column 1 (was `ozon_name`), keep SKU from column 2 for display only
2. Resolve Ozon product by navigating directly to the product URL (`page.goto`) instead of SKU search
3. Process rows sequentially top-to-bottom
4. On each task start, delete all historical data (Ozon cache + previous output Excel) and restart from row 1
5. Simulate human browsing behavior: random delays between requests, mouse movement, page scrolling
6. Maintain existing anti-bot challenge handling (pause-resume flow)

## Excel Format

| Column | Header | Content | Usage |
|--------|--------|---------|-------|
| 1 | ozon链接 | Ozon product URL (e.g. `https://www.ozon.ru/product/3570411009`) | Used for direct page navigation |
| 2 | sku | Product SKU number | Display/output only, not used for resolution |
| 3 | 产品重量 | Product weight | Passed through to output |

## Architecture Changes

### 1. Excel Parsing & Data Model (Rust: `src-tauri/src/commands/run_task.rs`)

**`TaskRow` struct** — add `ozon_url` field:

```rust
struct TaskRow {
    excel_row_index: u32,
    ozon_url: String,      // NEW — from column 0
    ozon_name: String,      // starts empty, populated from resolved page title
    sku: String,            // from column 1, display only
    original_cells: Vec<String>,
    image_bytes: Option<Vec<u8>>,
}
```

**`read_task_workbook`** — change column 0 parsing:

```rust
// Before:
let ozon_name = first_cell;
let sku = row.get(1)...;

// After:
let ozon_url = first_cell;  // column 0 is now the Ozon URL
let ozon_name = String::new(); // populated later from resolved title
let sku = row.get(1)...;      // same, but display-only
```

### 2. Ozon Resolution Path (Rust: `src-tauri/src/commands/run_task.rs`)

**Switch from SKU to URL resolution**:

- Change `DEFAULT_SIDECAR_OZON_RESOLVE_URL` from `/resolve-ozon-sku` to `/resolve-ozon-product`
- Change `SidecarOzonResolveRequest` to send `productUrl` instead of `sku`
- In `prepare_task_rows_for_execution`: check `!row.ozon_url.is_empty()` instead of `!row.sku.is_empty()`
- `resolve_ozon_product_via_sidecar` takes `product_url: &str` instead of `sku: &str`
- Cache key uses the product URL instead of SKU

**`hydrate_ozon_source_via_browser`** — takes `ozon_url` parameter:

```rust
fn hydrate_ozon_source_via_browser<F>(
    sink: &mut dyn EventSink,
    client: &Client,
    ozon_url: &str,           // was: sku
    ozon_disk_cache: &OzonSourceCache,
    ozon_session_warmed: &mut bool,
    ensure_browser_ready: &mut F,
) -> Result<OzonProductResolution, OzonResolutionFailure>
```

### 3. Cache & Output Clearing (Rust: `src-tauri/src/commands/run_task.rs`)

At the start of `run_task`, before processing any rows:

```rust
// Delete Ozon source cache directory
if let Some(cache_dir) = output_anchor_path.parent() {
    let cache_root = cache_dir.join(".desktop_app_cache");
    if cache_root.exists() {
        let _ = std::fs::remove_dir_all(&cache_root);
    }
}

// Delete previous output Excel file if it exists
if output_path.exists() {
    let _ = std::fs::remove(output_path);
}
```

### 4. Anti-Crawl Human Behavior Simulation

#### 4a. Random Inter-Row Delay (Rust: `src-tauri/src/commands/run_task.rs`)

Between each row's Ozon resolution call, add a random delay of 3-8 seconds:

```rust
use rand::Rng;

// Before resolving each row's Ozon product
let delay_ms = rand::thread_rng().gen_range(3_000..=8_000);
std::thread::sleep(Duration::from_millis(delay_ms));
```

#### 4b. Post-Navigation Human Simulation (Sidecar: `src-sidecar/src/ozon_session.ts`)

Add to `resolveOzonProductViaSession`, after `page.goto` and before the snapshot polling loop:

```typescript
async function simulateHumanBrowsing(page: Page, delay: (ms: number) => Promise<void>): Promise<void> {
  // Random mouse movements (2-4 points)
  const moveCount = 2 + Math.floor(Math.random() * 3);
  for (let i = 0; i < moveCount; i++) {
    const x = 200 + Math.floor(Math.random() * 800);
    const y = 150 + Math.floor(Math.random() * 500);
    await page.mouse.move(x, y, { steps: 5 + Math.floor(Math.random() * 10) });
    await delay(200 + Math.floor(Math.random() * 400));
  }

  // Scroll down 100-400px
  const scrollDown = 100 + Math.floor(Math.random() * 300);
  await page.evaluate((amount: number) => window.scrollBy(0, amount), scrollDown);
  await delay(500 + Math.floor(Math.random() * 1000));

  // Sometimes scroll back up a bit (50% chance)
  if (Math.random() > 0.5) {
    const scrollUp = 50 + Math.floor(Math.random() * 150);
    await page.evaluate((amount: number) => window.scrollBy(0, -amount), scrollUp);
    await delay(300 + Math.floor(Math.random() * 500));
  }
}
```

Call this after `page.goto` completes and DOM is loaded, before entering the polling loop.

#### 4c. Existing Evasions (No changes needed)

The Ozon browser already:
- Launches with `--disable-blink-features=AutomationControlled`
- Uses a real Chrome UA
- Does NOT apply Puppeteer's `evaluateOnNewDocument` evasions (it's a raw Chrome process, not Puppeteer-launched)

### 5. Sidecar Endpoint (No code changes)

The `/resolve-ozon-product` endpoint in `server.ts` already exists (lines 1027-1048). It accepts `{ productUrl: string }` and calls `resolveOzonProductViaBrowser(productUrl)`. No changes needed to the endpoint itself.

### 6. Output Excel

The output Excel maintains the same format but:
- `ozon_name` column shows the title resolved from the product page (not from Excel input)
- SKU column shows the original SKU from Excel (for reference)
- All other output columns (status, price, matched URL, images) remain the same

### 7. Error Handling

| Condition | Behavior |
|-----------|----------|
| Empty `ozon_url` in column 1 | Row status: "Ozon链接为空" |
| Invalid Ozon URL format | Row status: "Ozon链接格式无效" |
| Product page unavailable/404 | Row status: "Ozon商品不可访问" (existing handling) |
| Anti-bot challenge triggered | Pause-resume flow (existing handling) |
| Network timeout | Row status: error message (existing handling) |

## Files Modified

| File | Changes |
|------|---------|
| `src-tauri/src/commands/run_task.rs` | Excel parsing, resolution path, cache clearing, inter-row delay |
| `src-sidecar/src/ozon_session.ts` | Add `simulateHumanBrowsing` function, call after page.goto |
| `src-tauri/Cargo.toml` | Add `rand` dependency (if not present) |

## Files NOT Modified

- `src-sidecar/src/server.ts` — `/resolve-ozon-product` endpoint already exists
- `src-tauri/src/core/orchestrator.rs` — matching logic unchanged
- `src-tauri/src/core/ozon_cache.rs` — cache logic unchanged, just gets cleared on start
- `src-tauri/src/core/ozon_product.rs` — direct HTTP resolution unchanged (we use browser path)
- Frontend Vue components — UI unchanged

## Testing

1. **Sidecar tests**: `cd src-sidecar && bun test` — existing tests should pass; add test for `simulateHumanBrowsing`
2. **Rust tests**: `cd src-tauri && cargo test` — update Excel-parsing tests for new column mapping
3. **Manual E2E**: Upload the reference Excel (`origin_input.xlsx`), verify:
   - Each product URL is visited directly (no search bar interaction)
   - Random delays visible between rows (3-8s)
   - Mouse/scroll simulation visible in browser
   - Product title + image resolved correctly
   - Cache directory is empty at start
   - Anti-bot pause-resume works if triggered
