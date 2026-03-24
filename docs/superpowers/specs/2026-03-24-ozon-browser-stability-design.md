# Ozon Browser Stability: Duplicate Tabs & Chrome Crash Fix

## Problem

Two bugs in the Ozon browser session flow:

1. **Duplicate tabs**: Chrome launches with `about:blank` via `buildOzonChromeArgs`, but if the user data dir (`ozon_profile`) has a saved session, Chrome restores previous tabs in addition to the `about:blank` tab, resulting in two or more tabs visiting ozon.ru.

2. **Chrome crash on second SKU**: `resolveOzonSkuViaSession` calls `warmOzonSession` before each SKU search, which does `page.goto("https://www.ozon.ru/")` on the same tab. On the second call, the page is on a product detail page; navigating it back to the homepage under Ozon's heavy frontend JS triggers a crash. The Ozon browser also lacks `--disable-dev-shm-usage` (present on the 1688 browser), increasing crash risk under memory pressure.

## Expected Behavior

- Chrome launches with exactly one tab
- SKU search works reliably across consecutive lookups
- When a SKU has no matching product, the row is skipped (not the task aborted)
- When a SKU matches, the product main image is captured (existing logic, no changes needed)

## Approach: Minimal Fix (Option A)

### Fix 1: Eliminate duplicate tabs

**File**: `src-sidecar/src/server.ts`

**Changes to `buildOzonChromeArgs`**:
- Add `--disable-session-crashed-bubble` to suppress session restore prompts
- Add `--noerrdialogs` to suppress error dialogs

**Changes to `ensureOzonBrowserAliveInner`**:
After Puppeteer connects to the browser, close all tabs except one:
- Call `browser.pages()` to get all open pages
- Use `selectPreferredOzonSessionPage` to pick the best page (prefers an Ozon-domain page, falls back to `about:blank`)
- Close all other pages

### Fix 2: Prevent Chrome crash on consecutive SKU searches

**File**: `src-sidecar/src/server.ts`

**Changes to `buildOzonChromeArgs`**:
- Add `--disable-dev-shm-usage` to avoid shared memory exhaustion crashes
- Add `--disable-gpu` to reduce GPU-process crash risk

**File**: `src-sidecar/src/ozon_session.ts`

**Changes to `warmOzonSession`**:
Before calling `page.goto()`, check if the page is already on the Ozon homepage. If so, skip the navigation entirely:
- Parse the current URL; if hostname is `*.ozon.ru` and the path is `/` (or empty), the page is already on the homepage — return immediately
- Only navigate if the page is on a product detail page, search results, or non-Ozon URL

**Changes to `resolveOzonSkuViaSession`**:
Add crash recovery: if `warmOzonSession` or `fillAndSubmitOzonSkuSearch` throws a "Target closed" / "Session closed" / "Protocol error" exception:
- Clear `globalOzonPage` (set to null via `dependencies.setSessionPage(null)`)
- Re-acquire a session page via `ensureOzonSessionPage` + `warmOzonSession`
- Retry the search once

### Fix 3: SKU not found handling (Rust side)

**File**: `src-tauri/src/commands/run_task.rs`

Ensure that when the sidecar returns `OZON_SKU_NOT_FOUND` error code, the Rust orchestration:
- Marks the current row as `sku_not_found` status (emits a `row_result` event with appropriate stage/status)
- Skips to the next row instead of aborting the entire task
- This error code handling may already exist; verify and add if missing

## Files Changed

| File | Change |
|------|--------|
| `src-sidecar/src/server.ts` | `buildOzonChromeArgs`: add stability flags; `ensureOzonBrowserAliveInner`: close extra tabs after connect |
| `src-sidecar/src/ozon_session.ts` | `warmOzonSession`: skip navigation when already on homepage; `resolveOzonSkuViaSession`: add crash recovery retry |
| `src-tauri/src/commands/run_task.rs` | Verify `OZON_SKU_NOT_FOUND` handling skips row (not aborts task) |

## Out of Scope

- Changing the search mechanism (URL-based navigation, new tab per SKU)
- Modifying the image capture logic (already working correctly)
- Anti-bot challenge handling improvements
