# Ozon Unavailable Page First-Card Hop Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** When Ozon direct product URLs land on an unavailable page with a visible product-card container, click only the first product card and continue into the final detail page.

**Architecture:** Keep the existing direct URL visit flow unchanged for valid detail pages. Add a single-hop fallback inside `resolveOzonProductViaSession()` that extracts visible product-card candidates from the unavailable page, selects the first product from the first usable multi-product container, clicks it, and then resumes the current snapshot loop.

**Tech Stack:** TypeScript, Bun, Puppeteer, Tauri sidecar

---

### Task 1: Add failing unit tests for first-card selection

**Files:**
- Modify: `src-sidecar/src/ozon_session.test.ts`

**Step 1: Write failing tests**

Add tests covering:
- chooses the first product from the first multi-product container
- excludes the current product URL
- ignores single-item side containers
- returns null when no valid recommendation container exists

**Step 2: Run the test file**

Run: `cd src-sidecar && bun test src/ozon_session.test.ts`
Expected: FAIL because the new helper does not exist yet

**Step 3: Commit**

```bash
git add src-sidecar/src/ozon_session.test.ts
git commit -m "test: cover unavailable-page first-card selection"
```

### Task 2: Implement candidate selection helper

**Files:**
- Modify: `src-sidecar/src/ozon_session.ts`

**Step 1: Add pure candidate type and selector**

Add a small exported helper that accepts normalized candidate metadata and returns the selected href or `null`.

**Step 2: Run the test file**

Run: `cd src-sidecar && bun test src/ozon_session.test.ts`
Expected: PASS for the new helper tests

**Step 3: Commit**

```bash
git add src-sidecar/src/ozon_session.ts src-sidecar/src/ozon_session.test.ts
git commit -m "feat: select first product from unavailable ozon recommendation page"
```

### Task 3: Wire the helper into the live browser flow

**Files:**
- Modify: `src-sidecar/src/ozon_session.ts`

**Step 1: Add DOM candidate extraction + click logic**

Implement a thin browser helper that:
- collects visible `/product/` links
- groups them by nearest usable multi-product container
- picks the href via the pure selector
- clicks the chosen first product
- falls back to `page.goto(href)` if click does not navigate

**Step 2: Add single-hop guard in `resolveOzonProductViaSession()`**

On the first `unavailable` snapshot:
- try the hop
- if successful, continue polling
- if not, preserve the current unavailable failure

**Step 3: Run sidecar tests**

Run: `cd src-sidecar && bun test src/ozon_session.test.ts src/server.test.ts`
Expected: PASS

**Step 4: Commit**

```bash
git add src-sidecar/src/ozon_session.ts src-sidecar/src/ozon_session.test.ts
git commit -m "fix: hop from unavailable ozon pages into first recommended product"
```

### Task 4: Run regression verification

**Files:**
- No code changes

**Step 1: Run sidecar regression**

Run: `cd src-sidecar && bun test`
Expected: PASS

**Step 2: Run Rust regression**

Run: `cd src-tauri && cargo test --test run_task_command_test -- --nocapture`
Expected: PASS

**Step 3: Manual validation**

Run the desktop app and verify:
- a direct Ozon URL can land on the unavailable/recommendation page
- only the first product card is entered
- title and main image are extracted from the final detail page
- if no valid card exists, the row still finalizes as unavailable
