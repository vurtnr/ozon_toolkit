# Ozon Browser Stability Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix duplicate Ozon Chrome tabs on launch and Chrome crash when searching consecutive SKUs.

**Architecture:** Minimal changes to the sidecar's `buildOzonChromeArgs` (add stability flags), `ensureOzonBrowserAliveInner` (close extra tabs), `warmOzonSession` (skip redundant navigation), and `resolveOzonSkuViaSession` (crash recovery retry).

**Tech Stack:** TypeScript (Bun sidecar), Puppeteer, bun:test

**Spec:** `docs/superpowers/specs/2026-03-24-ozon-browser-stability-design.md`

---

### Task 1: Add stability flags to `buildOzonChromeArgs`

**Files:**
- Modify: `src-sidecar/src/server.ts:194-207`
- Test: `src-sidecar/src/server.test.ts`

- [ ] **Step 1: Update the existing test to expect new flags**

In `src-sidecar/src/server.test.ts`, update the `buildOzonChromeArgs` test to verify the new stability flags:

```typescript
describe("buildOzonChromeArgs", () => {
  test("uses a clean manual-like chrome startup for ozon without automation flags", () => {
    const args = buildOzonChromeArgs("/tmp/ozon_profile", 9222);

    expect(args).toContain("--user-data-dir=/tmp/ozon_profile");
    expect(args).toContain("--remote-debugging-port=9222");
    expect(args).toContain("about:blank");
    expect(args).toContain("--disable-dev-shm-usage");
    expect(args).toContain("--disable-gpu");
    expect(args).toContain("--disable-session-crashed-bubble");
    expect(args).toContain("--noerrdialogs");
    expect(args).not.toContain("--new-window");
    expect(args).not.toContain("--disable-blink-features=AutomationControlled");
    expect(args.some((value) => value.startsWith("--user-agent="))).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-sidecar && bun test src/server.test.ts`
Expected: FAIL — `buildOzonChromeArgs` does not yet include `--disable-dev-shm-usage`, `--disable-gpu`, `--disable-session-crashed-bubble`, or `--noerrdialogs`.

- [ ] **Step 3: Add the flags to `buildOzonChromeArgs`**

In `src-sidecar/src/server.ts`, update `buildOzonChromeArgs`:

```typescript
export function buildOzonChromeArgs(
  profileDir: string,
  remoteDebuggingPort: number,
  startUrl: string = "about:blank",
): string[] {
  return [
    `--user-data-dir=${profileDir}`,
    `--remote-debugging-port=${remoteDebuggingPort}`,
    "--start-maximized",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-dev-shm-usage",
    "--disable-gpu",
    "--disable-session-crashed-bubble",
    "--noerrdialogs",
    startUrl,
  ];
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-sidecar && bun test src/server.test.ts`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-sidecar/src/server.ts src-sidecar/src/server.test.ts
git commit -m "fix: add stability flags to ozon chrome launch args"
```

---

### Task 2: Close extra tabs after Ozon browser connect

**Files:**
- Modify: `src-sidecar/src/server.ts:731-746` (`ensureOzonBrowserAliveInner`)

- [ ] **Step 1: Add tab cleanup to `ensureOzonBrowserAliveInner`**

In `src-sidecar/src/server.ts`, after line 742 (`globalOzonPage = null;`), add tab cleanup logic before the closing brace:

```typescript
async function ensureOzonBrowserAliveInner(): Promise<Browser> {
  if (!globalOzonBrowser || !globalOzonBrowser.isConnected()) {
    if (globalOzonBrowser) {
      await globalOzonBrowser.close().catch(() => undefined);
    }

    const executablePath = await resolveChromePath();
    const profileDir = resolveOzonProfileDir();
    globalOzonBrowser =
      (await connectToExistingProfileBrowser(profileDir)) ??
      (await launchOzonBrowserProcess(executablePath, profileDir));
    globalOzonPage = null;

    // Close extra tabs — keep only one preferred page
    const allPages = await globalOzonBrowser.pages().catch(() => []);
    if (allPages.length > 1) {
      const preferred =
        selectPreferredOzonSessionPage(allPages) ?? allPages[0];
      for (const p of allPages) {
        if (p !== preferred && !p.isClosed()) {
          await p.close().catch(() => undefined);
        }
      }
    }
  }

  return globalOzonBrowser;
}
```

Note: `selectPreferredOzonSessionPage` is imported from `./ozon_session` — verify the import already exists at the top of `server.ts`.

- [ ] **Step 2: Verify the import exists**

Check `src-sidecar/src/server.ts` top-level imports. It should already import from `./ozon_session`. If `selectPreferredOzonSessionPage` is not imported, add it.

- [ ] **Step 3: Run all sidecar tests**

Run: `cd src-sidecar && bun test`
Expected: All tests PASS (this is a runtime-only change — no unit test can verify real browser tab behavior, but existing tests must not break)

- [ ] **Step 4: Commit**

```bash
git add src-sidecar/src/server.ts
git commit -m "fix: close extra ozon chrome tabs after browser connect"
```

---

### Task 3: Add `isOzonHomeUrl` helper and skip redundant navigation in `warmOzonSession`

**Files:**
- Modify: `src-sidecar/src/ozon_session.ts:91-98` (add helper), `src-sidecar/src/ozon_session.ts:516-555` (modify `warmOzonSession`)
- Test: `src-sidecar/src/ozon_session.test.ts`

- [ ] **Step 1: Write tests for `isOzonHomeUrl`**

Add to `src-sidecar/src/ozon_session.test.ts`:

```typescript
import {
  // ... existing imports ...
  isOzonHomeUrl,
} from "./ozon_session";

describe("isOzonHomeUrl", () => {
  test("treats the ozon.ru root as a home URL", () => {
    expect(isOzonHomeUrl("https://www.ozon.ru/")).toBe(true);
    expect(isOzonHomeUrl("https://ozon.ru/")).toBe(true);
    expect(isOzonHomeUrl("https://www.ozon.ru")).toBe(true);
  });

  test("treats ozon highlight/landing redirects as home URLs", () => {
    expect(isOzonHomeUrl("https://www.ozon.ru/highlight/global?miniapp=x")).toBe(true);
  });

  test("does not treat product or search pages as home URLs", () => {
    expect(isOzonHomeUrl("https://www.ozon.ru/product/3552213000/")).toBe(false);
    expect(isOzonHomeUrl("https://www.ozon.ru/search/?text=test")).toBe(false);
  });

  test("does not treat non-ozon URLs as home URLs", () => {
    expect(isOzonHomeUrl("https://www.google.com/")).toBe(false);
    expect(isOzonHomeUrl("about:blank")).toBe(false);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-sidecar && bun test src/ozon_session.test.ts`
Expected: FAIL — `isOzonHomeUrl` is not exported.

- [ ] **Step 3: Add `isOzonHomeUrl` to `ozon_session.ts`**

Add this exported function after the existing `isAllowedOzonHost` (around line 94):

```typescript
export function isOzonHomeUrl(url: string): boolean {
  try {
    const parsed = new URL(url);
    if (!isAllowedOzonHost(parsed.hostname)) {
      return false;
    }
    const path = parsed.pathname.replace(/\/+$/, "");
    return path === "" || path.startsWith("/highlight");
  } catch {
    return false;
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-sidecar && bun test src/ozon_session.test.ts`
Expected: PASS

- [ ] **Step 5: Modify `warmOzonSession` to skip navigation when already on homepage**

In `src-sidecar/src/ozon_session.ts`, update `warmOzonSession`:

```typescript
async function warmOzonSession(
  dependencies: ResolveOzonProductDependencies,
  page: Page,
): Promise<void> {
  try {
    await page.bringToFront();
  } catch {}

  // Skip navigation if already on the Ozon homepage
  const currentUrl = page.url();
  if (isOzonHomeUrl(currentUrl)) {
    return;
  }

  await page.goto(dependencies.landingUrl ?? DEFAULT_OZON_HOME_URL, {
    waitUntil: "domcontentloaded",
    timeout: 60_000,
  });

  const deadline = Date.now() + DEFAULT_LANDING_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const snapshot = await collectOzonSnapshotWithRetry(page);
    if (!snapshot) {
      await dependencies.delay(
        dependencies.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
      );
      continue;
    }
    const landingState = classifyOzonLandingSnapshot(snapshot);

    if (landingState === "anti_bot_challenge") {
      throw new Error("[ANTI_BOT_CHALLENGE] Ozon 首页触发验证且在进入商品页前未解除");
    }

    if (landingState === "ready") {
      await dependencies.delay(750);
      return;
    }

    await dependencies.delay(
      dependencies.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
    );
  }

  throw new Error("[OZON_RESOLVE_FAILED] Ozon 首页未完成加载，无法进入商品详情页");
}
```

- [ ] **Step 6: Run all sidecar tests**

Run: `cd src-sidecar && bun test`
Expected: All PASS

- [ ] **Step 7: Commit**

```bash
git add src-sidecar/src/ozon_session.ts src-sidecar/src/ozon_session.test.ts
git commit -m "fix: skip redundant ozon homepage navigation in warmOzonSession"
```

---

### Task 4: Add crash recovery retry to `resolveOzonSkuViaSession`

**Files:**
- Modify: `src-sidecar/src/ozon_session.ts:757-823` (`resolveOzonSkuViaSession`)
- Modify: `src-sidecar/src/ozon_session.ts:280-288` (widen `isTransientPageNavigationError`)

- [ ] **Step 1: Widen `isTransientPageNavigationError` to cover crash patterns**

In `src-sidecar/src/ozon_session.ts`, update the function:

```typescript
export function isTransientPageNavigationError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  const normalized = message.toLowerCase();

  return (
    normalized.includes("execution context was destroyed") ||
    normalized.includes("cannot find context with specified id") ||
    normalized.includes("session closed") ||
    normalized.includes("target closed") ||
    normalized.includes("protocol error")
  );
}
```

- [ ] **Step 2: Add tests for new crash patterns**

In `src-sidecar/src/ozon_session.test.ts`, add to the `isTransientPageNavigationError` describe block:

```typescript
  test("treats session-closed and target-closed errors as transient", () => {
    expect(
      isTransientPageNavigationError(new Error("Session closed. Most likely the page has been closed.")),
    ).toBe(true);
    expect(
      isTransientPageNavigationError(new Error("Target closed.")),
    ).toBe(true);
    expect(
      isTransientPageNavigationError(new Error("Protocol error (Runtime.callFunctionOn): Session closed.")),
    ).toBe(true);
  });
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cd src-sidecar && bun test src/ozon_session.test.ts`
Expected: PASS

- [ ] **Step 4: Add crash recovery to `resolveOzonSkuViaSession`**

Wrap the warm + search + poll logic in a try/catch with one retry on transient errors. In `src-sidecar/src/ozon_session.ts`, replace `resolveOzonSkuViaSession`:

```typescript
export async function resolveOzonSkuViaSession(
  dependencies: ResolveOzonProductDependencies,
  sku: string,
): Promise<OzonResolvePayload> {
  const normalizedSku = sku.trim();
  if (!normalizedSku) {
    throw new Error("[OZON_RESOLVE_FAILED] Ozon SKU 为空");
  }

  const attemptResolve = async (): Promise<OzonResolvePayload> => {
    const page = await ensureOzonSessionPage(dependencies);
    await warmOzonSession(dependencies, page);
    await fillAndSubmitOzonSkuSearch(page, normalizedSku);

    const deadline =
      Date.now() + (dependencies.resolveTimeoutMs ?? DEFAULT_RESOLVE_TIMEOUT_MS);
    let antiBotSeen = false;

    while (Date.now() < deadline) {
      const snapshot = await collectOzonSnapshotWithRetry(page);
      if (!snapshot) {
        await dependencies.delay(dependencies.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS);
        continue;
      }
      const snapshotState = classifyOzonSkuSearchSnapshot(snapshot);

      if (snapshotState === "anti_bot_challenge") {
        antiBotSeen = true;
        try {
          await page.bringToFront();
        } catch {}
        await dependencies.delay(
          dependencies.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
        );
        continue;
      }

      if (snapshotState === "not_found") {
        throw new Error("[OZON_SKU_NOT_FOUND] Ozon SKU 未找到对应商品");
      }

      if (snapshotState === "resolved") {
        const title = normalizeOzonTitle(snapshot.title);
        const imageUrl = normalizeOzonImageUrl(snapshot);
        if (!title || !imageUrl) {
          await dependencies.delay(
            dependencies.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
          );
          continue;
        }

        const imageBase64 = await captureOzonImageBase64(
          page,
          imageUrl,
          dependencies.delay,
        );
        return imageBase64 ? { title, imageUrl, imageBase64 } : { title, imageUrl };
      }

      await dependencies.delay(dependencies.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS);
    }

    if (antiBotSeen) {
      throw new Error("[ANTI_BOT_CHALLENGE] Ozon SKU 搜索触发验证且在超时前未解除");
    }

    throw new Error("[OZON_RESOLVE_FAILED] 未从 Ozon SKU 搜索中解析到商品标题或主图");
  };

  try {
    return await attemptResolve();
  } catch (error) {
    if (!isTransientPageNavigationError(error)) {
      throw error;
    }
    // Page/session crashed — clear session page and retry once
    dependencies.setSessionPage(null);
    return await attemptResolve();
  }
}
```

- [ ] **Step 5: Run all sidecar tests**

Run: `cd src-sidecar && bun test`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add src-sidecar/src/ozon_session.ts src-sidecar/src/ozon_session.test.ts
git commit -m "fix: add crash recovery retry for ozon sku search"
```

---

### Task 5: Verify Rust SKU-not-found handling and run full test suite

**Files:**
- Read-only: `src-tauri/src/commands/run_task.rs`
- Read-only: `src-tauri/tests/run_task_command_test.rs`

- [ ] **Step 1: Verify Rust handling (read-only)**

The existing code at `src-tauri/src/commands/run_task.rs:1560-1569` already handles `OzonResolutionFailure` by calling `continue`, which skips the failed row and processes the next one. The test `run_task_skips_rows_with_unresolved_ozon_sku` at `src-tauri/tests/run_task_command_test.rs:1148-1184` confirms this works. No Rust changes are needed.

- [ ] **Step 2: Run Rust tests to confirm no regressions**

Run: `cd src-tauri && cargo test --test run_task_command_test`
Expected: All tests PASS

- [ ] **Step 3: Run all sidecar tests one final time**

Run: `cd src-sidecar && bun test`
Expected: All tests PASS

- [ ] **Step 4: Final commit (if any remaining changes)**

No additional changes expected. All commits should already be made in Tasks 1–4.
