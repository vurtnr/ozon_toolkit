import path from "path";
import { Browser, Page } from "puppeteer";

export interface SearchResult {
  title: string;
  price: string;
  sales: string;
  moq: string;
  shopName: string;
  itemUrl: string;
  imageUrl: string;
  isAd: boolean;
  cosScore: number;
}

interface RectBox {
  left: number;
  top: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
}

interface CropPoint {
  x: number;
  y: number;
}

interface CursorProbePoint extends CropPoint {
  cursor: string;
}

type CursorProbeMode = "move" | "resize";

interface FullCanvasCropPlan {
  moveStart: CropPoint;
  moveEnd: CropPoint;
  resizeStart: CropPoint;
  resizeEnd: CropPoint;
}

type SearchResultsPageLike = Pick<Page, "waitForSelector" | "waitForNetworkIdle">;
type ClosableTabLike = {
  url: () => string;
  isClosed?: () => boolean;
};

interface SearchResultRecord {
  title: string;
  priceMajor: string;
  priceMinor: string;
  legacyPriceText: string;
  sales: string;
  moq: string;
  shopName: string;
  itemUrl: string;
  imageUrl: string;
  isAd: boolean;
  cosScore: number;
}

interface SearchResultSnapshot {
  records: SearchResultRecord[];
  reachedBottom: boolean;
}

export interface ResultScrollState {
  visibleResultCount: number;
  targetResultCount: number;
  reachedBottom: boolean;
  totalScrolled: number;
  maxScrollDistance: number;
}

const CROP_EDGE_PADDING = 6;
const DEFAULT_RESULT_LIMIT = 36;
const CAMERA_READY_SETTLE_MS = 500;
const SEARCH_CONFIRM_POLL_INTERVAL_MS = 250;
const SEARCH_CONFIRM_MAX_ATTEMPTS = 14;
const IMMEDIATE_RESULT_ENTRY_TIMEOUT_MS = 2_500;
const RESULT_ENTRY_TIMEOUT_MS = 15_000;
const RESULT_ENTRY_POLL_INTERVAL_MS = 250;
const RESULT_SELECTOR_TIMEOUT_MS = 8_000;
const RESULT_IDLE_TIMEOUT_MS = 5_000;
const RESULT_SECONDARY_SELECTOR_TIMEOUT_MS = 4_000;
const RESULT_SCROLL_INTERVAL_MS = 160;
const RESULT_SCROLL_DISTANCE_MIN = 120;
const RESULT_SCROLL_DISTANCE_MAX = 200;
const RESULT_SCROLL_MAX_DISTANCE = 4_000;
const RESULT_POST_SCROLL_SETTLE_MS = 450;
const RESULT_BOTTOM_GAP_PX = 120;
const SEARCH_CONFIRM_LABELS = ["搜索图片", "开始搜索", "立即搜索"];
const RESULT_URL_HINTS = [
  "youyuan",
  "offer_search",
  "imagesearch",
  "tab=imagesearch",
  "tab=imageSearch",
];

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function normalizeCursor(cursor: string): string {
  return (cursor || "").trim().toLowerCase();
}

function isMoveCursor(cursor: string): boolean {
  const value = normalizeCursor(cursor);
  return value === "move" || value.includes("grab") || value.includes("grabbing");
}

function isResizeCursor(cursor: string): boolean {
  const value = normalizeCursor(cursor);
  return value.includes("resize") || value.includes("nwse") || value.includes("nesw") || value.includes("ew-resize") || value.includes("ns-resize");
}

function matchesCursorMode(cursor: string, mode: CursorProbeMode): boolean {
  return mode === "move" ? isMoveCursor(cursor) : isResizeCursor(cursor);
}

export function pickBestCursorProbePoint(
  probes: CursorProbePoint[],
  mode: CursorProbeMode,
  canvasRect: RectBox,
): CropPoint | null {
  const candidates = probes.filter((probe) => mode === "move" ? isMoveCursor(probe.cursor) : isResizeCursor(probe.cursor));
  if (candidates.length === 0) return null;

  if (mode === "move") {
    const center = {
      x: canvasRect.left + canvasRect.width / 2,
      y: canvasRect.top + canvasRect.height / 2,
    };
    const best = candidates.reduce((best, point) => {
      const currentDistance = Math.hypot(point.x - center.x, point.y - center.y);
      const bestDistance = Math.hypot(best.x - center.x, best.y - center.y);
      return currentDistance < bestDistance ? point : best;
    });
    return { x: best.x, y: best.y };
  }

  const best = candidates.reduce((best, point) => {
    const currentScore = point.x + point.y;
    const bestScore = best.x + best.y;
    return currentScore > bestScore ? point : best;
  });
  return { x: best.x, y: best.y };
}

export function deriveCursorBounds(probes: CursorProbePoint[], mode: CursorProbeMode): RectBox | null {
  const candidates = probes.filter((probe) => matchesCursorMode(probe.cursor, mode));
  if (candidates.length === 0) return null;

  let left = candidates[0].x;
  let top = candidates[0].y;
  let right = candidates[0].x;
  let bottom = candidates[0].y;

  for (const point of candidates) {
    if (point.x < left) left = point.x;
    if (point.y < top) top = point.y;
    if (point.x > right) right = point.x;
    if (point.y > bottom) bottom = point.y;
  }

  return {
    left,
    top,
    right,
    bottom,
    width: right - left,
    height: bottom - top,
  };
}

export function pickResizeStartFromBounds(
  moveBounds: RectBox,
  canvasRect: RectBox,
  offset: number = 4,
): CropPoint {
  return {
    x: clamp(moveBounds.right + offset, canvasRect.left + CROP_EDGE_PADDING, canvasRect.right - CROP_EDGE_PADDING),
    y: clamp(moveBounds.bottom + offset, canvasRect.top + CROP_EDGE_PADDING, canvasRect.bottom - CROP_EDGE_PADDING),
  };
}

export function isLikelyCropCanvasRect(canvasRect: RectBox): boolean {
  if (canvasRect.width < 220 || canvasRect.height < 220) return false;
  const ratio = canvasRect.width / canvasRect.height;
  return ratio > 0.75 && ratio < 1.35;
}

export function evaluateResizeCoverage(
  beforeResizeBounds: RectBox | null,
  afterResizeBounds: RectBox | null,
  canvasRect: RectBox,
): {
  ok: boolean;
  reason: string;
  metrics: {
    growthX: number;
    growthY: number;
    coverageX: number;
    coverageY: number;
    rightGap: number;
    bottomGap: number;
  };
} {
  if (!beforeResizeBounds || !afterResizeBounds) {
    return {
      ok: false,
      reason: "missing-bounds",
      metrics: { growthX: 0, growthY: 0, coverageX: 0, coverageY: 0, rightGap: -1, bottomGap: -1 },
    };
  }

  const growthX = afterResizeBounds.width / Math.max(beforeResizeBounds.width, 1);
  const growthY = afterResizeBounds.height / Math.max(beforeResizeBounds.height, 1);
  const coverageX = afterResizeBounds.width / Math.max(canvasRect.width, 1);
  const coverageY = afterResizeBounds.height / Math.max(canvasRect.height, 1);
  const rightGap = canvasRect.right - afterResizeBounds.right;
  const bottomGap = canvasRect.bottom - afterResizeBounds.bottom;

  const failed: string[] = [];
  if (coverageX < 0.78) failed.push("coverageX");
  if (coverageY < 0.78) failed.push("coverageY");
  if (rightGap > 28) failed.push("rightGap");
  if (bottomGap > 28) failed.push("bottomGap");

  return {
    ok: failed.length === 0,
    reason: failed.length === 0 ? "ok" : failed.join("+"),
    metrics: { growthX, growthY, coverageX, coverageY, rightGap, bottomGap },
  };
}

export function buildResizeHandleCandidates(
  moveBounds: RectBox,
  canvasRect: RectBox,
  span: number = 8,
  step: number = 4,
): CropPoint[] {
  const points: CropPoint[] = [];
  const seen = new Set<string>();
  const clampIntoCanvas = (point: CropPoint): CropPoint => ({
    x: clamp(point.x, canvasRect.left + CROP_EDGE_PADDING, canvasRect.right - CROP_EDGE_PADDING),
    y: clamp(point.y, canvasRect.top + CROP_EDGE_PADDING, canvasRect.bottom - CROP_EDGE_PADDING),
  });

  const push = (point: CropPoint): void => {
    const clamped = clampIntoCanvas(point);
    const key = `${clamped.x}:${clamped.y}`;
    if (seen.has(key)) return;
    seen.add(key);
    points.push(clamped);
  };

  // Always try the geometric bottom-right corner first.
  push({ x: Math.round(moveBounds.right), y: Math.round(moveBounds.bottom) });

  for (let dx = -span; dx <= span; dx += step) {
    for (let dy = -span; dy <= span; dy += step) {
      if (dx === 0 && dy === 0) continue;
      push({
        x: Math.round(moveBounds.right + dx),
        y: Math.round(moveBounds.bottom + dy),
      });
    }
  }

  return points;
}

function clampPointInCanvas(point: CropPoint, canvasRect: RectBox, edgePadding: number): CropPoint {
  return {
    x: clamp(point.x, canvasRect.left + edgePadding, canvasRect.right - edgePadding),
    y: clamp(point.y, canvasRect.top + edgePadding, canvasRect.bottom - edgePadding),
  };
}

export function buildFullCanvasCropPlan(
  selectionRect: RectBox,
  canvasRect: RectBox,
  edgePadding: number = CROP_EDGE_PADDING,
): FullCanvasCropPlan {
  const centerOffsetX = selectionRect.width / 2;
  const centerOffsetY = selectionRect.height / 2;
  const moveStart = clampPointInCanvas(
    {
      x: selectionRect.left + centerOffsetX,
      y: selectionRect.top + centerOffsetY,
    },
    canvasRect,
    edgePadding,
  );

  const moveEnd = {
    x: canvasRect.left + edgePadding + centerOffsetX,
    y: canvasRect.top + edgePadding + centerOffsetY,
  };

  const resizeStart = clampPointInCanvas(
    {
      x: selectionRect.right - 4,
      y: selectionRect.bottom - 4,
    },
    canvasRect,
    edgePadding,
  );

  const resizeEnd = {
    x: canvasRect.right - edgePadding,
    y: canvasRect.bottom - edgePadding,
  };

  return { moveStart, moveEnd, resizeStart, resizeEnd };
}

export function limitSearchResults(results: SearchResult[], limit: number = DEFAULT_RESULT_LIMIT): SearchResult[] {
  return results.slice(0, limit);
}

export function shouldStopResultScroll(state: ResultScrollState): boolean {
  return (
    state.visibleResultCount >= state.targetResultCount ||
    state.reachedBottom ||
    state.totalScrolled >= state.maxScrollDistance
  );
}

export function shouldNavigateTo1688Home(currentUrl: string): boolean {
  const url = (currentUrl || "").trim().toLowerCase();
  if (!url.startsWith("https://www.1688.com/")) {
    return true;
  }

  try {
    const parsed = new URL(url);
    return parsed.pathname !== "/";
  } catch {
    return true;
  }
}

export function shouldEnsureHomePageBeforeSessionCheck(currentUrl: string): boolean {
  const url = (currentUrl || "").trim().toLowerCase();
  if (!url.startsWith("http")) {
    return true;
  }
  if (shouldNavigateTo1688Home(url)) {
    if (
      url.includes("login.1688.com") ||
      url.includes("member/signin") ||
      url.includes("passport.alibaba.com") ||
      url.includes("sec.") ||
      url.includes("punish") ||
      url.includes("captcha") ||
      url.includes("verify")
    ) {
      return false;
    }
    return true;
  }
  return false;
}

export function isLikelySearchResultsUrl(currentUrl: string): boolean {
  const url = (currentUrl || "").trim().toLowerCase();
  if (!url.startsWith("http")) {
    return false;
  }

  return RESULT_URL_HINTS.some((hint) => url.includes(hint));
}

export function shouldKeepWaitingForSearchConfirm(
  currentUrl: string,
  hasVisibleResults: boolean,
): boolean {
  if (hasVisibleResults) {
    return false;
  }

  return !isLikelySearchResultsUrl(currentUrl);
}

function shouldAutoCloseTab(currentUrl: string): boolean {
  const url = (currentUrl || "").trim().toLowerCase();
  return url === "about:blank" || url.includes("1688.com");
}

export function selectClosableTabs<T extends ClosableTabLike>(
  pages: T[],
  keepPages: T[],
): T[] {
  const keepSet = new Set(keepPages);
  return pages.filter((page) => {
    if (keepSet.has(page)) return false;
    if (page.isClosed?.()) return false;
    return shouldAutoCloseTab(page.url());
  });
}

async function cleanupAutomationTabs(browser: Pick<Browser, "pages">, keepPages: Page[]): Promise<void> {
  const pages = await browser.pages();
  const closableTabs = selectClosableTabs(pages, keepPages);
  for (const tab of closableTabs) {
    await tab.close().catch(() => undefined);
  }
}

export function assemblePriceFromFragments(
  majorFragment: string,
  minorFragment: string,
  fallbackText: string,
): string {
  const normalize = (value: string): string => (value || "").replace(/\s+/g, "");
  const extractNumeric = (value: string): string => {
    const cleaned = normalize(value).replace(/[^\d.]/g, "");
    const matched = cleaned.match(/\d+(?:\.\d+)?/);
    return matched ? matched[0] : "";
  };

  const merged = extractNumeric(`${normalize(majorFragment)}${normalize(minorFragment)}`);
  if (merged) return merged;
  return extractNumeric(fallbackText);
}

export type ResultPageRecallOptions = {
  forceFullCrop: boolean;
  scrapeCurrentPage: () => Promise<SearchResult[]>;
  applyFullCanvasCrop: () => Promise<void>;
};

export async function executeResultPageRecall(
  options: ResultPageRecallOptions,
): Promise<SearchResult[]> {
  if (!options.forceFullCrop) {
    console.log("👀 [第一重拦截] 采用 1688 默认 AI 框选极速提取...");
    return options.scrapeCurrentPage();
  }

  console.log("📐 [第二重爆破] 启动机械臂拖动 Canvas 拉满全图...");

  try {
    await options.applyFullCanvasCrop();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("❌ 强制重绘操作受阻，1688 页面可能未响应:", error);
    if (message.includes("[FULL_CROP_NOT_APPLIED]")) {
      throw error;
    }
    throw new Error(`[FULL_CROP_NOT_APPLIED] ${message}`);
  }

  return options.scrapeCurrentPage();
}

export async function openCropDialogAndWaitForCanvas(
  resultPage: Pick<Page, "waitForFunction" | "evaluate" | "waitForSelector">,
): Promise<void> {
  console.log("⏳ 等待裁剪面板出现...");
  await resultPage.waitForFunction(() => {
    const cut1 = document.querySelector(".cut-btn");
    const cut2 = document.querySelector('div[class*="cutBtn"]');
    return cut1 !== null || cut2 !== null;
  }, { timeout: 15_000 });

  await resultPage.evaluate(() => {
    const cutBtn =
      document.querySelector(".cut-btn") ||
      document.querySelector('div[class*="cutBtn"]');
    if (cutBtn) {
      cutBtn.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      cutBtn.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
      cutBtn.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
      cutBtn.click();
    }
  });

  await resultPage.waitForSelector("#croper-canvas", {
    visible: true,
    timeout: 20_000,
  });
  await new Promise((resolve) => setTimeout(resolve, 1_500));
}

export async function waitForSearchResults(
  resultPage: SearchResultsPageLike,
): Promise<boolean> {
  const selector = 'div[class*="searchOfferWrapper"]';
  const cardsReady = await resultPage
    .waitForSelector(selector, { timeout: RESULT_SELECTOR_TIMEOUT_MS })
    .then(() => true)
    .catch(() => false);

  if (cardsReady) {
    return true;
  }

  await resultPage.waitForNetworkIdle({ timeout: RESULT_IDLE_TIMEOUT_MS }).catch(() => {});
  return resultPage
    .waitForSelector(selector, { timeout: RESULT_SECONDARY_SELECTOR_TIMEOUT_MS })
    .then(() => true)
    .catch(() => false);
}

async function clickSearchConfirmIfPresent(
  page: Pick<Page, "evaluate">,
): Promise<boolean> {
  return page.evaluate(
    async ({ labels, maxAttempts, pollIntervalMs, resultUrlHints, resultSelector }) => {
      const normalizedLabels = labels.map((label) => label.replace(/\s+/g, ""));
      const isVisible = (element: HTMLElement): boolean => {
        const style = window.getComputedStyle(element);
        const rect = element.getBoundingClientRect();
        return (
          style.display !== "none" &&
          style.visibility !== "hidden" &&
          style.pointerEvents !== "none" &&
          rect.width > 0 &&
          rect.height > 0
        );
      };

      const clickLikeUser = (element: HTMLElement) => {
        element.scrollIntoView({ block: "center", inline: "center" });
        const events = [
          "pointerover",
          "mouseover",
          "pointerenter",
          "mouseenter",
          "pointerdown",
          "mousedown",
          "pointerup",
          "mouseup",
          "click",
        ];
        for (const eventName of events) {
          element.dispatchEvent(new MouseEvent(eventName, { bubbles: true, cancelable: true, composed: true }));
        }
        element.click();
      };

      return new Promise<boolean>((resolve) => {
        let attempts = 0;
        const timer = setInterval(() => {
          attempts += 1;
          const currentUrl = (window.location.href || "").trim().toLowerCase();
          const hasVisibleResults = document.querySelector(resultSelector) !== null;
          const alreadyEnteredResults =
            hasVisibleResults ||
            resultUrlHints.some((hint) => currentUrl.includes(hint));

          if (alreadyEnteredResults) {
            clearInterval(timer);
            resolve(false);
            return;
          }

          const candidates = Array.from(
            document.querySelectorAll<HTMLElement>("button, a, div, span"),
          ).filter((element) => {
            const text = (element.innerText || element.textContent || "").replace(/\s+/g, "");
            return normalizedLabels.includes(text) && isVisible(element);
          });

          if (candidates.length > 0) {
            clearInterval(timer);
            clickLikeUser(candidates[0]);
            resolve(true);
            return;
          }

          if (attempts >= maxAttempts) {
            clearInterval(timer);
            resolve(false);
          }
        }, pollIntervalMs);
      });
    },
    {
      labels: SEARCH_CONFIRM_LABELS,
      maxAttempts: SEARCH_CONFIRM_MAX_ATTEMPTS,
      pollIntervalMs: SEARCH_CONFIRM_POLL_INTERVAL_MS,
      resultUrlHints: RESULT_URL_HINTS,
      resultSelector: 'div[class*="searchOfferWrapper"]',
    },
  );
}

async function resolveResultPageAfterUpload(
  browser: Browser,
  sourcePage: Page,
  newTargetPromise: Promise<import("puppeteer").Target | null>,
  timeoutMs: number = RESULT_ENTRY_TIMEOUT_MS,
): Promise<Page | null> {
  let promisedPage: Page | null = null;
  void newTargetPromise.then(async (target) => {
    promisedPage = target ? await target.page().catch(() => null) : null;
  });

  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    if (promisedPage && isLikelySearchResultsUrl(promisedPage.url())) {
      return promisedPage;
    }

    if (isLikelySearchResultsUrl(sourcePage.url())) {
      return sourcePage;
    }

    const currentPageHasCards =
      (await sourcePage.$('div[class*="searchOfferWrapper"]')) !== null;
    if (currentPageHasCards) {
      return sourcePage;
    }

    const allPages = await browser.pages();
    const latestResultPage = [...allPages]
      .reverse()
      .find((candidate) => isLikelySearchResultsUrl(candidate.url()));
    if (latestResultPage) {
      return latestResultPage;
    }

    await delay(RESULT_ENTRY_POLL_INTERVAL_MS);
  }

  return null;
}

export async function search1688ByImage(
  browser: Browser,
  page: Page,
  imagePath: string,
  forceFullCrop: boolean = false, 
  targetKeywords: string[] = [],
): Promise<SearchResult[]> {
  const CAMERA_ICON_SELECTOR = ".image-file-reader-wrapper";
  const absoluteImgPath = path.resolve(imagePath);
  let resultPage: Page | null = null;

  const readSearchResultSnapshot = async (): Promise<SearchResultSnapshot> => {
    return resultPage!.evaluate(({ keywords, bottomGapPx }) => {
      const cards = Array.from(document.querySelectorAll('div[class*="searchOfferWrapper"]'));
      const parsedItems = cards.map((card) => {
        const titleEl = card.querySelector('div[class*="titleText"]');
        const title = titleEl ? titleEl.innerText.trim() : "";
        const priceContainer = card.querySelector('div[class*="priceItem"]');
        let priceMajor = "";
        let priceMinor = "";
        if (priceContainer instanceof HTMLElement) {
          const priceNodes = Array.from(priceContainer.querySelectorAll(":scope > div"));
          const yuanIndex = priceNodes.findIndex((node) => (node.textContent || "").replace(/\s+/g, "") === "¥");
          if (yuanIndex >= 0) {
            priceMajor = (priceNodes[yuanIndex + 1]?.textContent || "").trim();
            priceMinor = (priceNodes[yuanIndex + 2]?.textContent || "").trim();
          }
        }

        const legacyPriceEl = card.querySelector('div[class*="textMain"]');
        const legacyPriceText = legacyPriceEl ? legacyPriceEl.textContent?.trim() || "" : "";
        const shopEl = card.querySelector('div[class*="shopName"]');
        const shopName = shopEl ? shopEl.innerText.trim() : "";
        const imgEl = card.querySelector('img[class*="mainImg"]');
        const imageUrl = imgEl ? imgEl.src || imgEl.getAttribute("data-src") : "";
        const reportData = card.getAttribute("data-aplus-report") || card.getAttribute("data-tracker") || "";
        const isAd = reportData.includes("offerType:e_p4p") || reportData.includes("offerType:p4p");

        let cosScore = 0;
        const scoreMatch = reportData.match(/cosScore.*?([\d\.]+)/i);
        if (scoreMatch && scoreMatch[1]) cosScore = parseFloat(scoreMatch[1]);

        let itemUrl = "";
        const wwEl = card.querySelector(".J_WangWang");
        if (wwEl) {
          try {
            const extra = JSON.parse(wwEl.getAttribute("data-extra") || "{}");
            if (extra.offerId) itemUrl = `https://detail.1688.com/offer/${extra.offerId}.html`;
          } catch (e) {}
        }
        if (!itemUrl) {
          const match = reportData.match(/object_id@(\d+)/);
          if (match && match[1]) itemUrl = `https://detail.1688.com/offer/${match[1]}.html`;
        }
        return { title, priceMajor, priceMinor, legacyPriceText, sales: "", moq: "", shopName, itemUrl, imageUrl, isAd, cosScore };
      });

      const isScoreValid = parsedItems.filter((item) => item.cosScore > 0).length > 0;
      const records = parsedItems.filter((item) => {
        if (!item.title || !item.itemUrl || item.isAd) return false;
        if (isScoreValid && item.cosScore < 0.3) return false;
        if (keywords && keywords.length > 0) {
          const isMatchKeyword = keywords.some((kw) => item.title.includes(kw));
          if (!isMatchKeyword) return false;
        }
        return true;
      });
      const reachedBottom =
        Math.ceil(window.scrollY + window.innerHeight) >=
        Math.ceil(document.body.scrollHeight) - bottomGapPx;
      return { records, reachedBottom };
    }, { keywords: targetKeywords, bottomGapPx: RESULT_BOTTOM_GAP_PX });
  };

  const scrollResultPageStep = async (): Promise<number> => {
    return resultPage!.evaluate(({ minDistance, maxDistance }) => {
      const distance =
        Math.floor(Math.random() * (maxDistance - minDistance + 1)) + minDistance;
      window.scrollBy(0, distance);
      return distance;
    }, {
      minDistance: RESULT_SCROLL_DISTANCE_MIN,
      maxDistance: RESULT_SCROLL_DISTANCE_MAX,
    });
  };

  const scrapeCurrentPage = async (): Promise<SearchResult[]> => {
    let snapshot = await readSearchResultSnapshot();
    let totalScrolled = 0;

    while (
      !shouldStopResultScroll({
        visibleResultCount: snapshot.records.length,
        targetResultCount: DEFAULT_RESULT_LIMIT,
        reachedBottom: snapshot.reachedBottom,
        totalScrolled,
        maxScrollDistance: RESULT_SCROLL_MAX_DISTANCE,
      })
    ) {
      totalScrolled += await scrollResultPageStep();
      await new Promise((resolve) => setTimeout(resolve, RESULT_SCROLL_INTERVAL_MS));
      snapshot = await readSearchResultSnapshot();
    }

    await new Promise((resolve) => setTimeout(resolve, RESULT_POST_SCROLL_SETTLE_MS));
    snapshot = await readSearchResultSnapshot();

    const normalizedData: SearchResult[] = snapshot.records.map((item) => {
      const priceValue = assemblePriceFromFragments(item.priceMajor, item.priceMinor, item.legacyPriceText);
      return {
        title: item.title,
        price: priceValue ? `¥${priceValue}` : "暂无",
        sales: item.sales,
        moq: item.moq,
        shopName: item.shopName,
        itemUrl: item.itemUrl,
        imageUrl: item.imageUrl,
        isAd: item.isAd,
        cosScore: item.cosScore,
      };
    });

    // 保持页面原始排序并只取前 36 个结果回传 Rust
    return limitSearchResults(normalizedData);
  };

  try {
    await cleanupAutomationTabs(browser, [page]);

    // 阶段一：激活常驻主阵地，防止页面休眠
    await page.bringToFront();
    const needsHomeNavigation =
      shouldNavigateTo1688Home(page.url()) ||
      (await page.$(CAMERA_ICON_SELECTOR)) === null;
    if (needsHomeNavigation) {
      await page.goto("https://www.1688.com/", {
        waitUntil: "domcontentloaded",
        timeout: 60_000,
      });
    }

    const currentUrl = page.url();
    const hasSlider = (await page.$('.nc-container, #baxia-dialog-content, #nc_1_n1z, iframe[src*="punish"]')) !== null;
    if (currentUrl.includes("login") || currentUrl.includes("sec.") || hasSlider) {
      throw new Error("[ANTI_BOT_CHALLENGE] 触发 1688 底层拦截，请在浏览器中完成验证后重试");
    }

    const cameraHandle = await page.waitForSelector(CAMERA_ICON_SELECTOR, { visible: true, timeout: 30000 });
    await new Promise((r) => setTimeout(r, CAMERA_READY_SETTLE_MS));

    // 预埋标签页捕捉器
    const newTargetPromise = browser.waitForTarget((t) => t.type() === "page" && t.url().includes("1688.com") && t.url() !== page.url(), { timeout: 30000 }).catch(() => null);

    // 触发文件上传
    const [fileChooser] = await Promise.all([
      page.waitForFileChooser({ timeout: 15000 }),
      cameraHandle!.click().catch(async () => {
        await page.evaluate((sel) => document.querySelector(sel)?.click(), CAMERA_ICON_SELECTOR);
      }),
    ]);

    await fileChooser.accept([absoluteImgPath]);

    // 阶段二：先快速探测是否已自动进入结果页，只有未进入时才等待“搜索图片/开始搜索”确认按钮。
    resultPage = await resolveResultPageAfterUpload(
      browser,
      page,
      newTargetPromise,
      IMMEDIATE_RESULT_ENTRY_TIMEOUT_MS,
    );
    let searchConfirmed = false;

    if (!resultPage) {
      searchConfirmed = await clickSearchConfirmIfPresent(page);
      resultPage = await resolveResultPageAfterUpload(browser, page, newTargetPromise);
    }

    if (!resultPage && !searchConfirmed) {
      searchConfirmed = await clickSearchConfirmIfPresent(page);
      resultPage = await resolveResultPageAfterUpload(
        browser,
        page,
        newTargetPromise,
        IMMEDIATE_RESULT_ENTRY_TIMEOUT_MS,
      );
    }

    if (!resultPage) throw new Error("[IMAGE_SEARCH_NOT_ENTERED_RESULT_PAGE] 未能成功进入搜索结果页");
    await resultPage.bringToFront();
    const resultsReady = await waitForSearchResults(resultPage);
    if (!resultsReady && !isLikelySearchResultsUrl(resultPage.url())) {
      throw new Error("[IMAGE_SEARCH_NOT_ENTERED_RESULT_PAGE] 未能成功进入搜索结果页");
    }

    return await executeResultPageRecall({
      forceFullCrop,
      scrapeCurrentPage,
      applyFullCanvasCrop: async () => {
            const dragMouse = async (start: CropPoint, end: CropPoint, steps: number = 28): Promise<void> => {
              await resultPage!.mouse.move(start.x, start.y);
              await resultPage!.mouse.down();
              await resultPage!.mouse.move(end.x, end.y, { steps });
              await new Promise((r) => setTimeout(r, 200));
              await resultPage!.mouse.up();
            };

            const getCroperCanvasRect = async (): Promise<RectBox | null> => {
              const canvasHandle = await resultPage!.$("#croper-canvas");
              if (!canvasHandle) return null;
              const box = await canvasHandle.boundingBox();
              if (!box || box.width < 50 || box.height < 50) return null;
              const rect = {
                left: box.x,
                top: box.y,
                right: box.x + box.width,
                bottom: box.y + box.height,
                width: box.width,
                height: box.height,
              };
              return isLikelyCropCanvasRect(rect) ? rect : null;
            };

            const readCursorAtPoint = async (point: CropPoint): Promise<string> => {
              return await resultPage!.evaluate((payload) => {
                const { x, y } = payload;
                const canvas = document.querySelector("#croper-canvas");
                const hit = document.elementFromPoint(x, y);
                const picks: string[] = [];

                if (hit instanceof HTMLElement) {
                  picks.push(window.getComputedStyle(hit).cursor || hit.style.cursor || "");
                }
                if (canvas instanceof HTMLElement) {
                  picks.push(window.getComputedStyle(canvas).cursor || canvas.style.cursor || "");
                }
                if (document.body instanceof HTMLElement) {
                  picks.push(window.getComputedStyle(document.body).cursor || document.body.style.cursor || "");
                }

                const normalized = picks.map((cursor) => (cursor || "").toLowerCase().trim());
                const preferred = normalized.find((cursor) => cursor.length > 0 && cursor !== "default" && cursor !== "auto");
                return preferred || normalized[0] || "";
              }, point);
            };

            const scanCanvasCursorPoints = async (
              canvasRect: RectBox,
              rows: number,
              cols: number,
              edgePadding: number = 8,
            ): Promise<CursorProbePoint[]> => {
              const points: CursorProbePoint[] = [];
              const minX = canvasRect.left + edgePadding;
              const maxX = canvasRect.right - edgePadding;
              const minY = canvasRect.top + edgePadding;
              const maxY = canvasRect.bottom - edgePadding;

              for (let r = 0; r < rows; r++) {
                for (let c = 0; c < cols; c++) {
                  const x = cols === 1 ? (minX + maxX) / 2 : minX + (maxX - minX) * (c / (cols - 1));
                  const y = rows === 1 ? (minY + maxY) / 2 : minY + (maxY - minY) * (r / (rows - 1));
                  const point = { x: Math.round(x), y: Math.round(y) };
                  await resultPage!.mouse.move(point.x, point.y);
                  await new Promise((rsv) => setTimeout(rsv, 12));
                  const cursor = await readCursorAtPoint(point);
                  points.push({ ...point, cursor });
                }
              }
              return points;
            };

            const findCursorProbe = async (
              mode: CursorProbeMode,
              canvasRect: RectBox,
              passes: Array<{ rows: number; cols: number; edgePadding: number }> = [
                { rows: 7, cols: 7, edgePadding: 10 },
                { rows: 13, cols: 13, edgePadding: 6 },
              ],
            ): Promise<{ point: CropPoint | null; probes: CursorProbePoint[] }> => {
              const allProbes: CursorProbePoint[] = [];
              for (const pass of passes) {
                const probes = await scanCanvasCursorPoints(canvasRect, pass.rows, pass.cols, pass.edgePadding);
                allProbes.push(...probes);
                const picked = pickBestCursorProbePoint(allProbes, mode, canvasRect);
                if (picked) {
                  return { point: picked, probes: allProbes };
                }
              }
              return { point: null, probes: allProbes };
            };

            const moveBoundsProbePasses: Array<{ rows: number; cols: number; edgePadding: number }> = [
              { rows: 15, cols: 15, edgePadding: 4 },
              { rows: 21, cols: 21, edgePadding: 3 },
            ];

            const searchCursorAroundPoint = async (
              center: CropPoint,
              mode: CursorProbeMode,
              canvasRect: RectBox,
              radius: number = 30,
              step: number = 4,
            ): Promise<CropPoint | null> => {
              const points: CropPoint[] = [];
              for (let dx = -radius; dx <= radius; dx += step) {
                for (let dy = -radius; dy <= radius; dy += step) {
                  points.push({
                    x: clamp(Math.round(center.x + dx), canvasRect.left + CROP_EDGE_PADDING, canvasRect.right - CROP_EDGE_PADDING),
                    y: clamp(Math.round(center.y + dy), canvasRect.top + CROP_EDGE_PADDING, canvasRect.bottom - CROP_EDGE_PADDING),
                  });
                }
              }

              points.sort((a, b) => {
                const da = Math.hypot(a.x - center.x, a.y - center.y);
                const db = Math.hypot(b.x - center.x, b.y - center.y);
                return da - db;
              });

              for (const point of points) {
                await resultPage!.mouse.move(point.x, point.y);
                await new Promise((rsv) => setTimeout(rsv, 10));
                const cursor = await readCursorAtPoint(point);
                if (matchesCursorMode(cursor, mode)) return point;
              }

              return null;
            };

            const formatProbeSummary = (probes: CursorProbePoint[]): string => {
              return probes
                .filter((item) => item.cursor.length > 0)
                .slice(0, 24)
                .map((item) => `${item.cursor}@(${item.x},${item.y})`)
                .join(", ");
            };

            const ensureCropDialogReady = async (timeoutMs: number = 20000): Promise<RectBox> => {
              const startedAt = Date.now();
              while (Date.now() - startedAt < timeoutMs) {
                const rect = await getCroperCanvasRect();
                if (rect) {
                  const isModalReady = await resultPage!.evaluate((payload) => {
                    const canvas = document.querySelector("#croper-canvas");
                    if (!(canvas instanceof HTMLElement)) return false;
                    const centerX = Math.round(payload.left + payload.width / 2);
                    const centerY = Math.round(payload.top + payload.height / 2);
                    const hit = document.elementFromPoint(centerX, centerY);
                    const hitOnCanvas = hit === canvas || (hit instanceof Node && canvas.contains(hit));
                    const confirmBtn = Array.from(document.querySelectorAll("button, div, span"))
                      .find((el) => (el.textContent || "").trim() === "确认");
                    const cancelBtn = Array.from(document.querySelectorAll("button, div, span"))
                      .find((el) => (el.textContent || "").trim() === "取消");
                    return !!confirmBtn && !!cancelBtn && hitOnCanvas;
                  }, rect);
                  if (isModalReady) return rect;
                }
                await new Promise((r) => setTimeout(r, 250));
              }
              throw new Error("[FULL_CROP_NOT_APPLIED] 裁剪弹窗未进入可操作状态（可能误命中缩略图画布）");
            };

            await openCropDialogAndWaitForCanvas(resultPage!);
            const canvasRect = await ensureCropDialogReady(20000);

            const moveProbe = await findCursorProbe("move", canvasRect);
            if (!moveProbe.point) {
              const probeSummary = moveProbe.probes
                .filter((item) => item.cursor.length > 0)
                .slice(0, 20)
                .map((item) => `${item.cursor}@(${item.x},${item.y})`)
                .join(", ");
              throw new Error(`未探测到可拖拽 move 光标点，样本=${probeSummary || "none"}`);
            }

            const moveTarget = {
              x: canvasRect.left + 26,
              y: canvasRect.top + 26,
            };

            console.log("📐 步骤 1/2：探测 move 光标后，拖拽选框到左上角...");
            await dragMouse(moveProbe.point, moveTarget, 24);
            await new Promise((r) => setTimeout(r, 450));

            const beforeResizeProbe = await findCursorProbe("move", canvasRect, moveBoundsProbePasses);
            let moveBoundsBeforeResize = deriveCursorBounds(beforeResizeProbe.probes, "move");
            if (!moveBoundsBeforeResize) {
              throw new Error(`[FULL_CROP_NOT_APPLIED] 第一步移动后无法识别选框范围，样本=${formatProbeSummary(beforeResizeProbe.probes) || "none"}`);
            }
            console.log("🧭 move_bounds_before_resize:", moveBoundsBeforeResize);

            const resizeTarget = {
              x: canvasRect.right - CROP_EDGE_PADDING,
              y: canvasRect.bottom - CROP_EDGE_PADDING,
            };

            let coverage = evaluateResizeCoverage(moveBoundsBeforeResize, moveBoundsBeforeResize, canvasRect);
            let moveBoundsAfterResize: RectBox | null = null;

            for (let attempt = 1; attempt <= 3; attempt++) {
              const handleCandidates = buildResizeHandleCandidates(moveBoundsBeforeResize, canvasRect, 10, 3);
              let resizeStart: CropPoint | null = null;
              for (const candidate of handleCandidates) {
                const cursor = await readCursorAtPoint(candidate);
                if (isResizeCursor(cursor)) {
                  resizeStart = candidate;
                  break;
                }
              }

              if (!resizeStart) {
                const fallbackStart = pickResizeStartFromBounds(moveBoundsBeforeResize, canvasRect);
                resizeStart = await searchCursorAroundPoint(fallbackStart, "resize", canvasRect, 42, 3);
              }

              if (!resizeStart) {
                throw new Error(`[FULL_CROP_NOT_APPLIED] 第${attempt}次拉伸前未定位到 resize 控制点`);
              }

              console.log(`📐 步骤 2/2（第${attempt}次）：从`, resizeStart, "拉伸到", resizeTarget);
              await dragMouse(resizeStart, resizeTarget, attempt === 1 ? 30 : 26);
              await new Promise((r) => setTimeout(r, 380));

              const afterResizeMoveProbe = await findCursorProbe("move", canvasRect, moveBoundsProbePasses);
              moveBoundsAfterResize = deriveCursorBounds(afterResizeMoveProbe.probes, "move");
              console.log(`🧭 move_bounds_after_resize_attempt_${attempt}:`, moveBoundsAfterResize);

              coverage = evaluateResizeCoverage(moveBoundsBeforeResize, moveBoundsAfterResize, canvasRect);
              console.log(`🧭 coverage_ratio_attempt_${attempt}:`, coverage.metrics, "result:", coverage.reason);

              if (coverage.ok) break;

              // Coverage still fails: reset the box to top-left and retry a different handle start.
              if (attempt < 3) {
                const resetMoveProbe = await findCursorProbe("move", canvasRect, moveBoundsProbePasses);
                if (!resetMoveProbe.point) {
                  throw new Error(`[FULL_CROP_NOT_APPLIED] 第${attempt}次拉伸失败后无法重置选框，coverage=${coverage.reason}`);
                }
                await dragMouse(resetMoveProbe.point, moveTarget, 22);
                await new Promise((r) => setTimeout(r, 280));

                const resetBoundsProbe = await findCursorProbe("move", canvasRect, moveBoundsProbePasses);
                const resetBounds = deriveCursorBounds(resetBoundsProbe.probes, "move");
                if (!resetBounds) {
                  throw new Error(`[FULL_CROP_NOT_APPLIED] 第${attempt}次拉伸后重置失败，无法识别选框范围`);
                }
                moveBoundsBeforeResize = resetBounds;
                console.log(`🧭 move_bounds_before_resize_retry_${attempt}:`, moveBoundsBeforeResize);
              }
            }

            if (!coverage.ok) {
              throw new Error(`[FULL_CROP_NOT_APPLIED] 拉伸覆盖校验失败：${coverage.reason} metrics=${JSON.stringify(coverage.metrics)}`);
            }

            await resultPage.evaluate(() => {
              const visibleDialogs = Array.from(document.querySelectorAll('div[role="dialog"], div[class*="dialog"]'))
                .filter((node) => {
                  if (!(node instanceof HTMLElement)) return false;
                  const style = window.getComputedStyle(node);
                  const rect = node.getBoundingClientRect();
                  return style.display !== "none" && style.visibility !== "hidden" && rect.width > 50 && rect.height > 50;
                });
              const targetDialog = visibleDialogs.find((dialog) => dialog.querySelector("canvas")) || visibleDialogs[0];
              const scope = targetDialog || document;
              const confirmBtn = Array.from(scope.querySelectorAll("button, div, span")).find((el) => el.textContent?.trim() === "确认");
              if (confirmBtn instanceof HTMLElement) {
                confirmBtn.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
                confirmBtn.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
                confirmBtn.click();
              }
            });
            
            console.log("✅ 全图覆盖重绘完成！已提交，等待最新数据刷新...");
            await waitForSearchResults(resultPage);
      },
    });

  } catch (error) {
    console.error(`❌ 处理图片 ${imagePath} 发生异常:`, error);
    // 🌟 核心修改：绝对不吞没致命报错，将其透传回 server.ts 和 Rust！
    throw error; 
  } finally {
    // 阶段四：阅后即焚，关掉所有自动化遗留页，把干净的 1688 首页留给下一次搜索
    await cleanupAutomationTabs(browser, [page]);
  }
}
