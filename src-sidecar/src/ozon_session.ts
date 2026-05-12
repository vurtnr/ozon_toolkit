import type { Browser, Page } from "puppeteer";

export interface OzonResolvePayload {
  title: string;
  imageUrl: string;
  imageBase64?: string;
  specProfile?: OzonSpecProfile;
}

export interface OzonAttributeEntry {
  key: string;
  value: string;
}

export interface OzonSpecProfile {
  color?: string;
  sizeTokens: string[];
  countTokens: string[];
  material?: string;
  modelTokens: string[];
  featureTokens: string[];
  rawAttributes: OzonAttributeEntry[];
}

export type OzonTitleSource = "json_ld" | "meta_og" | "h1" | "document_title";
export type OzonImageSource = "json_ld" | "meta_og" | "dom_img";

export interface OzonSnapshot {
  url: string;
  documentTitle: string;
  title: string | null;
  titleSource?: OzonTitleSource;
  imageUrl: string | null;
  imageSource?: OzonImageSource;
  bodyText: string;
  rawAttributes?: OzonAttributeEntry[];
  specProfile?: OzonSpecProfile;
  hasAntiBotChallenge: boolean;
  isUnavailable: boolean;
}

export interface OzonRecommendedProductCandidate {
  href: string;
  top: number;
  left: number;
  containerKey: string;
  containerTop: number;
  containerLeft: number;
  containerArea: number;
  containerProductCount: number;
}

export interface OzonImageCaptureCandidateMetrics {
  currentSrc: string;
  naturalWidth: number;
  naturalHeight: number;
  rectWidth: number;
  rectHeight: number;
  rectTop: number;
  rectBottom: number;
  viewportHeight: number;
}

export type OzonPageState =
  | "resolved"
  | "anti_bot_challenge"
  | "unavailable"
  | "incomplete";

export type OzonSkuSearchState =
  | "resolved"
  | "not_found"
  | "anti_bot_challenge"
  | "incomplete";

export type OzonLandingState = "ready" | "anti_bot_challenge" | "loading";

export interface ResolveOzonProductDependencies {
  browser: Browser;
  getSessionPage: () => Page | null;
  setSessionPage: (page: Page | null) => void;
  applyBrowserEvasions: (page: Page) => Promise<void>;
  delay: (ms: number) => Promise<void>;
  resolveTimeoutMs?: number;
  pollIntervalMs?: number;
  landingUrl?: string;
}

const DEFAULT_OZON_HOME_URL = "https://www.ozon.ru/";
const DEFAULT_RESOLVE_TIMEOUT_MS = 180_000;
const DEFAULT_POLL_INTERVAL_MS = 1_000;
const DEFAULT_LANDING_TIMEOUT_MS = 60_000;
const DEFAULT_RECOMMENDED_HOP_TIMEOUT_MS = 8_000;

const OZON_ANTI_BOT_URL_HINTS = ["__rr=", "abt_att=1", "captcha"];
const OZON_ANTI_BOT_TEXT_HINTS = [
  "antibot captcha",
  "antibot challenge",
  "please, enable javascript to continue",
  "нам нужно убедиться, что вы не робот",
];
const OZON_RESTRICTED_TEXT_HINTS = [
  "доступ ограничен",
  "инцидент:",
  "чтобы решить проблему, попробуйте сделать это",
  "служба поддержки",
  "обновить версию браузера или мобильного приложения",
  "подключиться к другому wi-fi или мобильной сети",
];
const OZON_BLOCKED_TEXT_HINTS = [
  ...OZON_ANTI_BOT_TEXT_HINTS,
  ...OZON_RESTRICTED_TEXT_HINTS,
];
const OZON_UNAVAILABLE_TEXT_HINTS = [
  "такого товара нет",
  "такой страницы не существует",
  "страница не найдена",
  "извините, такой страницы нет",
  "товар закончился",
  "нет в наличии",
];
const OZON_SKU_NOT_FOUND_TEXT_HINTS = ["такой страницы не существует"];
const OZON_GENERIC_RESOLVED_TITLE_HINTS = [
  "купить на ozon",
  "цена на ozon",
  "доставка на ozon",
];
const OZON_GENERIC_IMAGE_URL_HINTS = [
  "og_ozon_ru.png",
  "/s3/cms/logo/",
  "/cms/logo/",
];
const OZON_MULTI_PRODUCT_BODY_HINTS = [
  "похожие товары",
  "похожие предложения",
  "с этим товаром ищут",
  "рекомендуем также",
];
const OZON_SEARCH_INPUT_SELECTORS = [
  'input[type="search"]',
  'input[name="text"]',
  'input[placeholder*="искать" i]',
  'input[placeholder*="поиск" i]',
  'input[placeholder*="найти" i]',
  'input[aria-label*="поиск" i]',
  'input[aria-label*="искать" i]',
];

function dedupeTokens(values: string[]): string[] {
  const seen = new Set<string>();
  return values
    .map((value) => value.trim())
    .filter((value) => value.length > 0)
    .filter((value) => {
      const normalized = value.toLowerCase();
      if (seen.has(normalized)) return false;
      seen.add(normalized);
      return true;
    });
}

export function extractOzonNumericTokens(value: string): string[] {
  return dedupeTokens(
    (value.match(/\d+(?:\.\d+)?(?:\s?(?:cm|см|mm|мм|ml|мл|l|л|pcs|шт|件|pack|уп|г|kg|кг))?/gi) ||
      [])
      .map((token) => token.replace(/\s+/g, "")),
  );
}

export function buildOzonSpecProfile(rawAttributes: OzonAttributeEntry[]): OzonSpecProfile {
  let color: string | undefined;
  let material: string | undefined;
  const sizeTokens: string[] = [];
  const countTokens: string[] = [];
  const modelTokens: string[] = [];
  const featureTokens: string[] = [];

  for (const attribute of rawAttributes) {
    const key = attribute.key.trim().toLowerCase();
    const value = attribute.value.trim();
    const normalizedValue = value.toLowerCase();
    if (!key || !value) continue;

    if (!color && /цвет|расцветка|оттенок/.test(key)) {
      color = value;
      continue;
    }

    if (!material && /материал/.test(key)) {
      material = value;
      continue;
    }

    if (/длина|ширина|высота|размер|габарит|объем|объём|диаметр|вес/.test(key)) {
      sizeTokens.push(...extractOzonNumericTokens(value));
      featureTokens.push(value);
      continue;
    }

    if (/количество|комплект|набор|единиц|штук|шт|втоваре/.test(key.replace(/\s+/g, ""))) {
      countTokens.push(...extractOzonNumericTokens(value));
      featureTokens.push(value);
      continue;
    }

    if (/модель|артикул|тип|вид|серия|стиль|форма/.test(key)) {
      modelTokens.push(value);
      continue;
    }

    featureTokens.push(value);
  }

  return {
    color,
    sizeTokens: dedupeTokens(sizeTokens),
    countTokens: dedupeTokens(countTokens),
    material,
    modelTokens: dedupeTokens(modelTokens),
    featureTokens: dedupeTokens(featureTokens),
    rawAttributes: rawAttributes.filter((entry) => entry.key.trim() && entry.value.trim()),
  };
}

type ReusableBootstrapPageLike = {
  url: () => string;
  isClosed?: () => boolean;
};

function isAllowedOzonHost(hostname: string): boolean {
  const normalized = (hostname || "").trim().toLowerCase();
  return normalized === "ozon.ru" || normalized.endsWith(".ozon.ru");
}

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

function extractComparableImageToken(value: string | null | undefined): string | null {
  const normalized = (value || "").trim();
  if (!normalized) {
    return null;
  }

  try {
    const parsed = new URL(normalized);
    const segments = parsed.pathname.split("/").filter(Boolean);
    const lastSegment = segments[segments.length - 1]?.trim().toLowerCase() || "";
    return lastSegment || null;
  } catch {
    const match = normalized.match(/([a-z0-9_-]+\.(?:jpg|jpeg|png|webp|gif|bmp))(?:[?#].*)?$/i);
    return match?.[1]?.toLowerCase() ?? null;
  }
}

export function scoreOzonImageCaptureCandidate(
  candidate: OzonImageCaptureCandidateMetrics,
  expectedImageUrl: string | null,
): number {
  const renderedArea =
    Math.max(candidate.rectWidth, 0) * Math.max(candidate.rectHeight, 0);
  const naturalArea =
    Math.max(candidate.naturalWidth, 0) * Math.max(candidate.naturalHeight, 0);
  const isVisible = candidate.rectWidth >= 40 && candidate.rectHeight >= 40;
  const isLikelyProductImage =
    candidate.naturalWidth >= 120 &&
    candidate.naturalHeight >= 120 &&
    candidate.rectTop < candidate.viewportHeight + 200 &&
    candidate.rectBottom > -200;

  if (!isVisible || !isLikelyProductImage) {
    return Number.NEGATIVE_INFINITY;
  }

  const expectedToken = extractComparableImageToken(expectedImageUrl);
  const currentToken = extractComparableImageToken(candidate.currentSrc);
  const matchesExpected = Boolean(
    expectedToken && currentToken && expectedToken === currentToken,
  );

  if (expectedToken && !matchesExpected) {
    return Number.NEGATIVE_INFINITY;
  }

  let nextScore = naturalArea + renderedArea;
  if (matchesExpected) {
    nextScore += 1_000_000_000;
  }
  if (candidate.rectTop >= 0 && candidate.rectTop <= candidate.viewportHeight) {
    nextScore += 100_000;
  }

  return nextScore;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function isReusableBootstrapPageUrl(url: string): boolean {
  const normalized = (url || "").trim().toLowerCase();
  return (
    normalized.length === 0 ||
    normalized === "about:blank" ||
    normalized.startsWith("chrome://newtab") ||
    normalized.startsWith("edge://newtab")
  );
}

export function selectReusableOzonBootstrapPage<T extends ReusableBootstrapPageLike>(
  pages: readonly T[],
): T | null {
  for (const page of pages) {
    if (page.isClosed?.()) {
      continue;
    }

    if (isReusableBootstrapPageUrl(page.url())) {
      return page;
    }
  }

  return null;
}

export function selectPreferredOzonSessionPage<T extends ReusableBootstrapPageLike>(
  pages: readonly T[],
): T | null {
  for (const page of pages) {
    if (page.isClosed?.()) {
      continue;
    }

    try {
      const parsed = new URL(page.url());
      if (isAllowedOzonHost(parsed.hostname)) {
        return page;
      }
    } catch {}
  }

  return selectReusableOzonBootstrapPage(pages);
}

export function buildCanonicalOzonProductUrl(value: string): string | null {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  try {
    const parsed = new URL(trimmed);
    if (!isAllowedOzonHost(parsed.hostname)) {
      return null;
    }

    const segments = parsed.pathname.split("/").filter(Boolean);
    if (segments[0] !== "product" || !segments[1]) {
      return null;
    }

    const rawSegment = segments[1].trim();
    const normalizedSegment = rawSegment ? decodeURIComponent(rawSegment) : "";
    const productIdMatch = normalizedSegment.match(/(\d{5,})$/);
    const productId = productIdMatch?.[1] ?? null;
    if (!productId) {
      return null;
    }

    const canonical = new URL(DEFAULT_OZON_HOME_URL);
    canonical.pathname = `/product/${productId}/`;
    canonical.search = "";
    canonical.hash = "";
    return canonical.toString();
  } catch {
    return null;
  }
}

export function selectFirstRecommendedProductHref(
  candidates: readonly OzonRecommendedProductCandidate[],
  currentUrl: string,
): string | null {
  const currentCanonicalUrl = buildCanonicalOzonProductUrl(currentUrl);
  const normalizedCandidates = candidates
    .map((candidate) => {
      const canonicalHref = buildCanonicalOzonProductUrl(candidate.href);
      if (!canonicalHref) {
        return null;
      }

      if (currentCanonicalUrl && canonicalHref === currentCanonicalUrl) {
        return null;
      }

      if (
        !Number.isFinite(candidate.top) ||
        !Number.isFinite(candidate.left) ||
        !Number.isFinite(candidate.containerTop) ||
        !Number.isFinite(candidate.containerLeft) ||
        !Number.isFinite(candidate.containerArea)
      ) {
        return null;
      }

      if (candidate.containerProductCount < 2) {
        return null;
      }

      return {
        ...candidate,
        canonicalHref,
      };
    })
    .filter(
      (
        candidate,
      ): candidate is OzonRecommendedProductCandidate & { canonicalHref: string } =>
        candidate !== null,
    );

  if (normalizedCandidates.length === 0) {
    return null;
  }

  const groupedCandidates = new Map<
    string,
    {
      containerTop: number;
      containerLeft: number;
      containerArea: number;
      containerProductCount: number;
      items: Array<OzonRecommendedProductCandidate & { canonicalHref: string }>;
    }
  >();

  for (const candidate of normalizedCandidates) {
    const key = candidate.containerKey.trim() || `${candidate.containerTop}:${candidate.containerLeft}`;
    const existing = groupedCandidates.get(key);
    if (existing) {
      existing.items.push(candidate);
      existing.containerProductCount = Math.max(
        existing.containerProductCount,
        candidate.containerProductCount,
      );
      existing.containerArea = Math.max(existing.containerArea, candidate.containerArea);
      existing.containerTop = Math.min(existing.containerTop, candidate.containerTop);
      existing.containerLeft = Math.min(existing.containerLeft, candidate.containerLeft);
      continue;
    }

    groupedCandidates.set(key, {
      containerTop: candidate.containerTop,
      containerLeft: candidate.containerLeft,
      containerArea: candidate.containerArea,
      containerProductCount: candidate.containerProductCount,
      items: [candidate],
    });
  }

  const selectedGroup = [...groupedCandidates.values()].sort((leftGroup, rightGroup) => {
    return (
      leftGroup.containerTop - rightGroup.containerTop ||
      leftGroup.containerLeft - rightGroup.containerLeft ||
      rightGroup.containerProductCount - leftGroup.containerProductCount ||
      rightGroup.containerArea - leftGroup.containerArea
    );
  })[0];

  if (!selectedGroup) {
    return null;
  }

  const selectedCandidate = [...selectedGroup.items].sort((leftCandidate, rightCandidate) => {
    return leftCandidate.top - rightCandidate.top || leftCandidate.left - rightCandidate.left;
  })[0];

  return selectedCandidate?.canonicalHref ?? null;
}

export async function collectOzonSnapshot(page: Page): Promise<OzonSnapshot> {
  const snapshot = await page.evaluate(
    (antiBotUrlHints: string[], antiBotTextHints: string[], unavailableTextHints: string[]) => {
      const readJsonLdProduct = (): {
        title: string | null;
        imageUrl: string | null;
        titleSource: OzonTitleSource | null;
      } => {
        const scripts = Array.from(
          document.querySelectorAll<HTMLScriptElement>('script[type="application/ld+json"]'),
        );

        for (const script of scripts) {
          const raw = script.textContent?.trim();
          if (!raw) continue;

          try {
            const parsed = JSON.parse(raw);
            const queue = Array.isArray(parsed) ? [...parsed] : [parsed];

            while (queue.length > 0) {
              const current = queue.shift();
              if (!current || typeof current !== "object") continue;

              if (Array.isArray((current as { "@graph"?: unknown[] })["@graph"])) {
                queue.push(
                  ...(((current as { "@graph"?: unknown[] })["@graph"] as unknown[]) || []),
                );
              }

              const type = (current as { "@type"?: unknown })["@type"];
              const looksLikeProduct =
                typeof type === "string"
                  ? type.toLowerCase() === "product"
                  : Array.isArray(type)
                    ? type.some(
                        (value) =>
                          typeof value === "string" && value.toLowerCase() === "product",
                      )
                    : false;
              if (!looksLikeProduct) continue;

              const title =
                typeof (current as { name?: unknown }).name === "string"
                  ? ((current as { name?: string }).name ?? null)
                  : null;
              const image = (current as { image?: unknown }).image;
              const imageUrl =
                typeof image === "string"
                  ? image
                  : Array.isArray(image)
                    ? image.find((value) => typeof value === "string") ?? null
                    : image &&
                        typeof image === "object" &&
                          typeof (image as { url?: unknown }).url === "string"
                      ? ((image as { url?: string }).url ?? null)
                      : null;

              return { title, imageUrl, titleSource: title ? "json_ld" : null };
            }
          } catch {}
        }

        return { title: null, imageUrl: null, titleSource: null };
      };

      const jsonLd = readJsonLdProduct();
      const ogTitle =
        document.querySelector('meta[property="og:title"]')?.getAttribute("content") || null;
      const h1Title = document.querySelector("h1")?.textContent || null;
      const documentTitle = document.title || "";
      const rawTitle =
        jsonLd.title ||
        ogTitle ||
        h1Title ||
        documentTitle ||
        null;
      const titleSource =
        jsonLd.titleSource ||
        (ogTitle ? "meta_og" : null) ||
        (h1Title ? "h1" : null) ||
        (documentTitle.trim() ? "document_title" : null);
      const ogImageUrl =
        document.querySelector('meta[property="og:image"]')?.getAttribute("content") || null;
      const domImageUrl = document.querySelector("img")?.getAttribute("src") || null;
      const rawImageUrl =
        jsonLd.imageUrl ||
        ogImageUrl ||
        domImageUrl ||
        null;
      const imageSource =
        (jsonLd.imageUrl ? "json_ld" : null) ||
        (ogImageUrl ? "meta_og" : null) ||
        (domImageUrl ? "dom_img" : null);
      const bodyText = (document.body?.innerText || "").slice(0, 20_000);
      const rawAttributes = Array.from(document.querySelectorAll("dt, th, div, span"))
        .map((node) => node as HTMLElement)
        .flatMap((node) => {
          const key = (node.innerText || node.textContent || "").trim();
          if (!key || key.length > 80) return [];
          if (!/(цвет|расцветка|оттенок|длина|ширина|высота|размер|габарит|объем|объём|диаметр|вес|количество|комплект|набор|единиц|штук|материал|модель|артикул|тип|вид|серия|стиль|форма)/i.test(key)) {
            return [];
          }

          const next = node.nextElementSibling as HTMLElement | null;
          const value = (next?.innerText || next?.textContent || "").trim();
          if (!value || value.length > 120) return [];
          return [{ key, value }];
        })
        .slice(0, 30);
      const normalizedBody = bodyText.toLowerCase();
      const normalizedTitle = (document.title || "").toLowerCase();
      const normalizedUrl = window.location.href.toLowerCase();

      return {
        url: window.location.href,
        documentTitle,
        title: rawTitle ? rawTitle.trim() : null,
        titleSource: titleSource ?? undefined,
        imageUrl: rawImageUrl ? rawImageUrl.trim() : null,
        imageSource: imageSource ?? undefined,
        bodyText,
        rawAttributes,
        hasAntiBotChallenge:
          document.querySelector("#captcha-input") !== null ||
          antiBotUrlHints.some((hint) => normalizedUrl.includes(hint)) ||
          antiBotTextHints.some(
            (hint) => normalizedTitle.includes(hint) || normalizedBody.includes(hint),
          ),
        isUnavailable: unavailableTextHints.some((hint) => normalizedBody.includes(hint)),
      };
    },
    OZON_ANTI_BOT_URL_HINTS,
    OZON_BLOCKED_TEXT_HINTS,
    OZON_UNAVAILABLE_TEXT_HINTS,
  );

  return {
    ...snapshot,
    specProfile: buildOzonSpecProfile(snapshot.rawAttributes || []),
  };
}

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

async function collectOzonSnapshotWithRetry(
  page: Page,
  timeoutMs: number = 2_000,
): Promise<OzonSnapshot | null> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    try {
      return await collectOzonSnapshot(page);
    } catch (error) {
      if (!isTransientPageNavigationError(error)) {
        throw error;
      }
    }

    await delay(100);
  }

  return null;
}

function normalizeOzonSnapshotText(snapshot: OzonSnapshot): string {
  return [snapshot.documentTitle, snapshot.title, snapshot.bodyText]
    .filter((value): value is string => typeof value === "string" && value.length > 0)
    .join(" ")
    .toLowerCase()
    .replace(/\s+/g, " ")
    .trim();
}

export function snapshotHasOzonAntiBotSignal(snapshot: OzonSnapshot): boolean {
  const normalizedUrl = (snapshot.url || "").toLowerCase();
  const normalizedText = normalizeOzonSnapshotText(snapshot);

  return (
    snapshot.hasAntiBotChallenge ||
    OZON_ANTI_BOT_URL_HINTS.some((hint) => normalizedUrl.includes(hint)) ||
    OZON_BLOCKED_TEXT_HINTS.some((hint) => normalizedText.includes(hint))
  );
}

export function normalizeOzonTitle(value: string | null): string | null {
  if (!value) return null;
  const normalized = value.replace(/\s+/g, " ").trim();
  if (!normalized) return null;
  const normalizedLower = normalized.toLowerCase();
  if (
    /^antibot/i.test(normalized) ||
    OZON_BLOCKED_TEXT_HINTS.some((hint) => normalizedLower.includes(hint))
  ) {
    return null;
  }
  return normalized;
}

export function normalizeOzonImageUrl(snapshot: OzonSnapshot): string | null {
  if (!snapshot.imageUrl) return null;
  try {
    return new URL(snapshot.imageUrl, snapshot.url).toString();
  } catch {
    return null;
  }
}

export function classifyOzonSnapshot(snapshot: OzonSnapshot): OzonPageState {
  if (snapshotHasOzonAntiBotSignal(snapshot)) {
    return "anti_bot_challenge";
  }

  const normalizedText = normalizeOzonSnapshotText(snapshot);
  if (
    snapshot.isUnavailable ||
    OZON_UNAVAILABLE_TEXT_HINTS.some((hint) => normalizedText.includes(hint))
  ) {
    return "unavailable";
  }

  const title = normalizeOzonTitle(snapshot.title);
  const imageUrl = normalizeOzonImageUrl(snapshot);
  if (title && imageUrl) {
    return "resolved";
  }

  return "incomplete";
}

function snapshotHasExplicitNotFoundSignal(snapshot: OzonSnapshot): boolean {
  const normalizedText = normalizeOzonSnapshotText(snapshot);
  return OZON_SKU_NOT_FOUND_TEXT_HINTS.some((hint) => normalizedText.includes(hint));
}

function snapshotHasGenericResolvedTitle(snapshot: OzonSnapshot): boolean {
  const title = normalizeOzonTitle(snapshot.title);
  if (!title) {
    return false;
  }
  const normalizedTitle = title.toLowerCase();
  return (
    snapshot.titleSource !== "json_ld" &&
    snapshot.titleSource !== "h1" &&
    OZON_GENERIC_RESOLVED_TITLE_HINTS.some((hint) => normalizedTitle.includes(hint))
  );
}

function snapshotHasGenericResolvedImage(snapshot: OzonSnapshot): boolean {
  const normalizedImageUrl = normalizeOzonImageUrl(snapshot)?.toLowerCase() || "";
  if (!normalizedImageUrl) {
    return false;
  }

  return OZON_GENERIC_IMAGE_URL_HINTS.some((hint) => normalizedImageUrl.includes(hint));
}

function snapshotHasIntermediateBodySignal(snapshot: OzonSnapshot): boolean {
  const normalizedBody = normalizeOzonSnapshotText(snapshot);
  return OZON_MULTI_PRODUCT_BODY_HINTS.some((hint) => normalizedBody.includes(hint));
}

function snapshotLooksLikeIntermediateListing(snapshot: OzonSnapshot): boolean {
  return snapshotHasGenericResolvedTitle(snapshot) || snapshotHasGenericResolvedImage(snapshot);
}

export function shouldHopFromResolvedSnapshot(snapshot: OzonSnapshot): boolean {
  const title = normalizeOzonTitle(snapshot.title);
  const imageUrl = normalizeOzonImageUrl(snapshot);
  if (!title || !imageUrl) {
    return false;
  }

  return (
    !snapshotHasExplicitNotFoundSignal(snapshot) &&
    snapshotLooksLikeIntermediateListing(snapshot) &&
    snapshotHasIntermediateBodySignal(snapshot)
  );
}

export function shouldHopFromIncompleteSnapshot(snapshot: OzonSnapshot): boolean {
  if (snapshotHasExplicitNotFoundSignal(snapshot)) {
    return false;
  }

  return (
    snapshotLooksLikeIntermediateListing(snapshot) ||
    (!normalizeOzonTitle(snapshot.title) && snapshotHasIntermediateBodySignal(snapshot))
  );
}

export function shouldAttemptRecommendedProductHop(snapshot: OzonSnapshot): boolean {
  return !snapshotHasExplicitNotFoundSignal(snapshot);
}

export function classifyOzonSkuSearchSnapshot(
  snapshot: OzonSnapshot,
): OzonSkuSearchState {
  if (snapshotHasOzonAntiBotSignal(snapshot)) {
    return "anti_bot_challenge";
  }

  const normalizedText = normalizeOzonSnapshotText(snapshot);
  if (
    OZON_SKU_NOT_FOUND_TEXT_HINTS.some((hint) => normalizedText.includes(hint))
  ) {
    return "not_found";
  }

  const title = normalizeOzonTitle(snapshot.title);
  const imageUrl = normalizeOzonImageUrl(snapshot);
  if (title && imageUrl) {
    return "resolved";
  }

  return "incomplete";
}

export function classifyOzonLandingSnapshot(
  snapshot: OzonSnapshot,
): OzonLandingState {
  if (snapshotHasOzonAntiBotSignal(snapshot)) {
    return "anti_bot_challenge";
  }

  try {
    const parsed = new URL(snapshot.url || "");
    if (isAllowedOzonHost(parsed.hostname)) {
      const hasVisibleContent =
        snapshot.documentTitle.trim().length > 0 || snapshot.bodyText.trim().length > 0;
      if (hasVisibleContent) {
        return "ready";
      }
    }
  } catch {}

  return "loading";
}

async function captureOzonImageBase64(
  page: Page,
  expectedImageUrl: string | null,
  delay: (ms: number) => Promise<void>,
): Promise<string | null> {
  const normalizedExpected = expectedImageUrl ? expectedImageUrl.trim() : "";
  const imageHandles = await page.$$("img");
  let bestHandle: (typeof imageHandles)[number] | null = null;
  let bestScore = Number.NEGATIVE_INFINITY;

  for (const handle of imageHandles) {
    try {
      const metrics = await handle.evaluate((img) => {
        const rect = img.getBoundingClientRect();
        return {
          currentSrc: (img.currentSrc || img.getAttribute("src") || "").trim(),
          naturalWidth: img.naturalWidth || 0,
          naturalHeight: img.naturalHeight || 0,
          rectWidth: rect.width,
          rectHeight: rect.height,
          rectTop: rect.top,
          rectBottom: rect.bottom,
          viewportHeight: window.innerHeight,
        };
      });
      const score = scoreOzonImageCaptureCandidate(metrics, normalizedExpected);

      if (score > bestScore) {
        if (bestHandle) {
          await bestHandle.dispose().catch(() => undefined);
        }
        bestHandle = handle;
        bestScore = score;
      } else {
        await handle.dispose().catch(() => undefined);
      }
    } catch {
      await handle.dispose().catch(() => undefined);
    }
  }

  if (!bestHandle || !Number.isFinite(bestScore)) {
    return null;
  }

  try {
    await bestHandle.evaluate((img) =>
      img.scrollIntoView({ block: "center", inline: "center", behavior: "instant" }),
    );
    await delay(250);
    const screenshot = await bestHandle.screenshot({ type: "png" });
    return Buffer.from(screenshot).toString("base64");
  } catch {
    return null;
  } finally {
    await bestHandle.dispose().catch(() => undefined);
  }
}

async function collectRecommendedProductCandidates(
  page: Page,
  currentUrl: string,
): Promise<OzonRecommendedProductCandidate[]> {
  const currentCanonicalUrl = buildCanonicalOzonProductUrl(currentUrl);

  return page.evaluate((currentCanonicalUrl: string | null) => {
    const isAllowedOzonHost = (hostname: string): boolean => {
      const normalized = (hostname || "").trim().toLowerCase();
      return normalized === "ozon.ru" || normalized.endsWith(".ozon.ru");
    };

    const canonicalizeProductUrl = (value: string): string | null => {
      const trimmed = (value || "").trim();
      if (!trimmed) {
        return null;
      }

      try {
        const parsed = new URL(trimmed, window.location.href);
        if (!isAllowedOzonHost(parsed.hostname)) {
          return null;
        }

        const segments = parsed.pathname.split("/").filter(Boolean);
        if (segments[0] !== "product" || !segments[1]) {
          return null;
        }

        const rawSegment = decodeURIComponent(segments[1].trim());
        const productIdMatch = rawSegment.match(/(\d{5,})$/);
        const productId = productIdMatch?.[1] ?? null;
        if (!productId) {
          return null;
        }

        const canonical = new URL("https://www.ozon.ru/");
        canonical.pathname = `/product/${productId}/`;
        canonical.search = "";
        canonical.hash = "";
        return canonical.toString();
      } catch {
        return null;
      }
    };

    const isVisible = (element: Element | null): element is HTMLElement => {
      if (!(element instanceof HTMLElement)) {
        return false;
      }

      const style = window.getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return (
        style.display !== "none" &&
        style.visibility !== "hidden" &&
        style.opacity !== "0" &&
        rect.width > 0 &&
        rect.height > 0
      );
    };

    const visibleProductAnchors = Array.from(
      document.querySelectorAll<HTMLAnchorElement>("a[href]"),
    ).filter((anchor) => {
      if (!isVisible(anchor)) {
        return false;
      }

      const canonicalHref = canonicalizeProductUrl(anchor.href || anchor.getAttribute("href") || "");
      if (!canonicalHref) {
        return false;
      }

      return canonicalHref !== currentCanonicalUrl;
    });

    const containerIds = new WeakMap<HTMLElement, string>();
    let nextContainerId = 1;

    const getContainerId = (container: HTMLElement): string => {
      const existing = containerIds.get(container);
      if (existing) {
        return existing;
      }

      const nextId = `recommended-container-${nextContainerId++}`;
      containerIds.set(container, nextId);
      return nextId;
    };

    return visibleProductAnchors
      .map((anchor) => {
        const canonicalHref = canonicalizeProductUrl(
          anchor.href || anchor.getAttribute("href") || "",
        );
        if (!canonicalHref) {
          return null;
        }

        const anchorRect = anchor.getBoundingClientRect();
        let containerMeta:
          | {
              containerKey: string;
              containerTop: number;
              containerLeft: number;
              containerArea: number;
              containerProductCount: number;
            }
          | null = null;

        for (let element = anchor.parentElement; element; element = element.parentElement) {
          if (!isVisible(element)) {
            continue;
          }

          const rect = element.getBoundingClientRect();
          if (rect.width < 320 || rect.height < 120) {
            continue;
          }

          const productLinks = Array.from(
            element.querySelectorAll<HTMLAnchorElement>("a[href]"),
          )
            .filter((link) => isVisible(link))
            .map((link) => canonicalizeProductUrl(link.href || link.getAttribute("href") || ""))
            .filter((href): href is string => href !== null && href !== currentCanonicalUrl);

          const uniqueProductLinks = [...new Set(productLinks)];
          if (uniqueProductLinks.length < 2) {
            continue;
          }

          containerMeta = {
            containerKey: getContainerId(element),
            containerTop: rect.top,
            containerLeft: rect.left,
            containerArea: rect.width * rect.height,
            containerProductCount: uniqueProductLinks.length,
          };
          break;
        }

        if (!containerMeta) {
          return null;
        }

        return {
          href: canonicalHref,
          top: anchorRect.top,
          left: anchorRect.left,
          ...containerMeta,
        };
      })
      .filter(
        (
          candidate,
        ): candidate is OzonRecommendedProductCandidate => candidate !== null,
      );
  }, currentCanonicalUrl);
}

async function waitForOzonUrlChange(
  page: Page,
  previousUrl: string,
  delayFn: (ms: number) => Promise<void>,
  timeoutMs: number = DEFAULT_RECOMMENDED_HOP_TIMEOUT_MS,
): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    const currentUrl = page.url();
    if (currentUrl && currentUrl !== previousUrl) {
      return true;
    }

    const snapshot = await collectOzonSnapshotWithRetry(page, 500);
    if (snapshot?.url && snapshot.url !== previousUrl) {
      return true;
    }

    await delayFn(200);
  }

  return false;
}

async function clickRecommendedProductByHref(
  page: Page,
  targetHref: string,
): Promise<boolean> {
  return page.evaluate((targetHref: string) => {
    const isAllowedOzonHost = (hostname: string): boolean => {
      const normalized = (hostname || "").trim().toLowerCase();
      return normalized === "ozon.ru" || normalized.endsWith(".ozon.ru");
    };

    const canonicalizeProductUrl = (value: string): string | null => {
      const trimmed = (value || "").trim();
      if (!trimmed) {
        return null;
      }

      try {
        const parsed = new URL(trimmed, window.location.href);
        if (!isAllowedOzonHost(parsed.hostname)) {
          return null;
        }

        const segments = parsed.pathname.split("/").filter(Boolean);
        if (segments[0] !== "product" || !segments[1]) {
          return null;
        }

        const rawSegment = decodeURIComponent(segments[1].trim());
        const productIdMatch = rawSegment.match(/(\d{5,})$/);
        const productId = productIdMatch?.[1] ?? null;
        if (!productId) {
          return null;
        }

        const canonical = new URL("https://www.ozon.ru/");
        canonical.pathname = `/product/${productId}/`;
        canonical.search = "";
        canonical.hash = "";
        return canonical.toString();
      } catch {
        return null;
      }
    };

    const isVisible = (element: Element | null): element is HTMLElement => {
      if (!(element instanceof HTMLElement)) {
        return false;
      }

      const style = window.getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return (
        style.display !== "none" &&
        style.visibility !== "hidden" &&
        style.opacity !== "0" &&
        rect.width > 0 &&
        rect.height > 0
      );
    };

    const anchors = Array.from(
      document.querySelectorAll<HTMLAnchorElement>("a[href]"),
    )
      .filter((anchor) => isVisible(anchor))
      .filter((anchor) => {
        const canonicalHref = canonicalizeProductUrl(anchor.href || anchor.getAttribute("href") || "");
        return canonicalHref === targetHref;
      })
      .sort((leftAnchor, rightAnchor) => {
        const leftRect = leftAnchor.getBoundingClientRect();
        const rightRect = rightAnchor.getBoundingClientRect();
        return leftRect.top - rightRect.top || leftRect.left - rightRect.left;
      });

    const targetAnchor = anchors[0];
    if (!targetAnchor) {
      return false;
    }

    targetAnchor.scrollIntoView({ block: "center", inline: "center", behavior: "instant" });
    targetAnchor.removeAttribute("target");
    targetAnchor.click();
    return true;
  }, targetHref);
}

async function hopToFirstRecommendedProductDetail(
  page: Page,
  currentUrl: string,
  delayFn: (ms: number) => Promise<void>,
): Promise<boolean> {
  const targetHref = selectFirstRecommendedProductHref(
    await collectRecommendedProductCandidates(page, currentUrl),
    currentUrl,
  );
  if (!targetHref) {
    return false;
  }

  const previousUrl = page.url();
  let clickTriggered = false;

  try {
    clickTriggered = await clickRecommendedProductByHref(page, targetHref);
  } catch (error) {
    if (!isTransientPageNavigationError(error)) {
      throw error;
    }
  }

  if (clickTriggered && await waitForOzonUrlChange(page, previousUrl, delayFn)) {
    return true;
  }

  await page.goto(targetHref, {
    waitUntil: "domcontentloaded",
    timeout: 60_000,
  });
  return true;
}

async function ensureOzonSessionPage(
  dependencies: ResolveOzonProductDependencies,
): Promise<Page> {
  const current = dependencies.getSessionPage();
  if (current && !current.isClosed()) {
    try {
      await current.bringToFront();
      await current.evaluate(() => document.title);
      return current;
    } catch {}
  }

  const reusablePage = selectPreferredOzonSessionPage(
    await dependencies.browser.pages().catch(() => []),
  );
  if (reusablePage) {
    await dependencies.applyBrowserEvasions(reusablePage);
    dependencies.setSessionPage(reusablePage);
    return reusablePage;
  }

  const page = await dependencies.browser.newPage();
  await dependencies.applyBrowserEvasions(page);
  dependencies.setSessionPage(page);
  return page;
}

async function warmOzonSession(
  dependencies: ResolveOzonProductDependencies,
  page: Page,
): Promise<void> {
  try {
    await page.bringToFront();
  } catch {}

  const searchInputCssSelector = OZON_SEARCH_INPUT_SELECTORS
    .map(s => `${s}:not([disabled])`)
    .join(", ");

  const waitForSearchInputInteractive = async (): Promise<void> => {
    try {
      await page.waitForSelector(searchInputCssSelector, {
        visible: true,
        timeout: 10_000,
      });
    } catch {}
    // Allow JS hydration / autocomplete handlers to fully attach
    await dependencies.delay(1_000);
  };

  // If already on the Ozon homepage, still ensure the search input is interactive
  const currentUrl = page.url();
  if (isOzonHomeUrl(currentUrl)) {
    await waitForSearchInputInteractive();
    return;
  }

  await page.goto(dependencies.landingUrl ?? DEFAULT_OZON_HOME_URL, {
    waitUntil: "load",
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
      await waitForSearchInputInteractive();
      return;
    }

    await dependencies.delay(
      dependencies.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS,
    );
  }

  throw new Error("[OZON_RESOLVE_FAILED] Ozon 首页未完成加载，无法进入商品详情页");
}

async function fillAndSubmitOzonSkuSearch(
  page: Page,
  sku: string,
): Promise<void> {
  const selectedSelector = await page.evaluate((selectors: string[]) => {
    for (const selector of selectors) {
      const candidate = document.querySelector<HTMLInputElement>(selector);
      if (!candidate || candidate.disabled) {
        continue;
      }

      const rect = candidate.getBoundingClientRect();
      const visible = rect.width > 0 && rect.height > 0;
      if (visible) {
        return selector;
      }
    }

    return null;
  }, OZON_SEARCH_INPUT_SELECTORS);

  if (!selectedSelector) {
    throw new Error("[OZON_RESOLVE_FAILED] 未找到 Ozon 顶部搜索框");
  }

  const beforeUrl = page.url();
  await page.waitForSelector(selectedSelector, { visible: true, timeout: 5_000 });
  await delay(500);

  // Dismiss any existing autocomplete/suggestion overlay
  await page.keyboard.press("Escape");
  await delay(500);

  // Clear input reliably via JS and focus
  const clearAndFocusInput = async (): Promise<void> => {
    await page.evaluate((selector: string) => {
      const input = document.querySelector<HTMLInputElement>(selector);
      if (input) {
        input.focus();
        input.value = "";
        input.dispatchEvent(new Event("input", { bubbles: true }));
        input.dispatchEvent(new Event("change", { bubbles: true }));
      }
    }, selectedSelector);
    await delay(500);

    // Verify the input actually has focus; re-focus if not
    const hasFocus = await page.evaluate((selector: string) => {
      const input = document.querySelector<HTMLInputElement>(selector);
      if (!input) return false;
      if (document.activeElement !== input) {
        input.focus();
      }
      return document.activeElement === input;
    }, selectedSelector);

    if (!hasFocus) {
      await page.click(selectedSelector).catch(() => undefined);
      await delay(300);
    }
  };

  // Set input value directly via JS, bypassing React controlled component interception
  const setInputValueViaJS = async (value: string): Promise<void> => {
    await page.evaluate((selector: string, val: string) => {
      const input = document.querySelector<HTMLInputElement>(selector);
      if (!input) return;
      input.focus();
      const nativeSetter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype, "value",
      )?.set;
      if (nativeSetter) {
        nativeSetter.call(input, val);
      } else {
        input.value = val;
      }
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new Event("change", { bubbles: true }));
    }, selectedSelector, value);
  };

  // Read back the current input value for verification
  const readInputValue = async (): Promise<string> => {
    return page.evaluate((selector: string) => {
      const input = document.querySelector<HTMLInputElement>(selector);
      return input?.value ?? "";
    }, selectedSelector);
  };

  // Set value atomically via JS and verify with retries
  await clearAndFocusInput();
  await setInputValueViaJS(sku);
  await delay(500);

  for (let attempt = 0; attempt < 3; attempt++) {
    const currentValue = (await readInputValue()).trim();
    if (currentValue === sku) break;

    // Mismatch — dismiss autocomplete, clear, re-set
    await page.keyboard.press("Escape");
    await delay(300);
    await clearAndFocusInput();
    await setInputValueViaJS(sku);
    await delay(500);
  }

  // Hard gate: verify input value matches SKU before submitting
  const finalValue = (await readInputValue()).trim();
  if (finalValue !== sku) {
    throw new Error(
      `[OZON_RESOLVE_FAILED] SKU 输入验证失败: 期望 "${sku}", 实际 "${finalValue}"`,
    );
  }

  const waitForSearchTransition = async (timeoutMs: number): Promise<boolean> => {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const currentUrl = page.url();
      if (!isReusableBootstrapPageUrl(currentUrl) && currentUrl !== beforeUrl) {
        return true;
      }

      const snapshot = await collectOzonSnapshotWithRetry(page);
      if (snapshot) {
        const state = classifyOzonSkuSearchSnapshot(snapshot);
        if (
          state === "resolved" ||
          state === "not_found" ||
          state === "anti_bot_challenge"
        ) {
          return true;
        }

        if (!isReusableBootstrapPageUrl(snapshot.url) && snapshot.url !== beforeUrl) {
          return true;
        }
      }

      await delay(250);
    }

    return false;
  };

  await page.keyboard.press("Enter");
  if (await waitForSearchTransition(10_000)) {
    return;
  }

  const clicked = await page.evaluate((selector: string) => {
    const input = document.querySelector<HTMLInputElement>(selector);
    if (!input) {
      return false;
    }

    const buttonCandidates = new Set<Element>();
    const form = input.closest("form");
    if (form) {
      for (const candidate of form.querySelectorAll("button, [type='submit']")) {
        buttonCandidates.add(candidate);
      }
    }

    for (const candidate of document.querySelectorAll("button, [type='submit']")) {
      buttonCandidates.add(candidate);
    }

    for (const candidate of buttonCandidates) {
      if (!(candidate instanceof HTMLElement)) {
        continue;
      }

      const rect = candidate.getBoundingClientRect();
      if (rect.width <= 0 || rect.height <= 0) {
        continue;
      }

      const text = (candidate.innerText || candidate.textContent || "")
        .toLowerCase()
        .replace(/\s+/g, " ")
        .trim();
      if (
        text.includes("искать") ||
        text.includes("найти") ||
        text.includes("search")
      ) {
        candidate.click();
        return true;
      }
    }

    return false;
  }, selectedSelector);

  if (clicked && await waitForSearchTransition(10_000)) {
    return;
  }

  throw new Error("[OZON_RESOLVE_FAILED] Ozon SKU 搜索未触发结果页跳转");
}

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

export async function resolveOzonProductViaSession(
  dependencies: ResolveOzonProductDependencies,
  productUrl: string,
): Promise<OzonResolvePayload> {
  const page = await ensureOzonSessionPage(dependencies);
  await warmOzonSession(dependencies, page);
  const canonicalProductUrl = buildCanonicalOzonProductUrl(productUrl);
  if (!canonicalProductUrl) {
    throw new Error("[OZON_RESOLVE_FAILED] Ozon 商品链接无效");
  }

  await page.goto(canonicalProductUrl, {
    waitUntil: "domcontentloaded",
    timeout: 60_000,
  });

  // Simulate human browsing behavior to avoid anti-bot detection
  await simulateHumanBrowsing(page, dependencies.delay);

  const deadline =
    Date.now() + (dependencies.resolveTimeoutMs ?? DEFAULT_RESOLVE_TIMEOUT_MS);
  let antiBotSeen = false;
  let recommendedProductHopAttempted = false;

  while (Date.now() < deadline) {
    const snapshot = await collectOzonSnapshotWithRetry(page);
    if (!snapshot) {
      await dependencies.delay(dependencies.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS);
      continue;
    }
    const snapshotState = classifyOzonSnapshot(snapshot);

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

    if (snapshotState === "unavailable") {
      if (
        !recommendedProductHopAttempted &&
        shouldAttemptRecommendedProductHop(snapshot)
      ) {
        recommendedProductHopAttempted = true;
        const jumpedIntoRecommendedProduct = await hopToFirstRecommendedProductDetail(
          page,
          snapshot.url || canonicalProductUrl,
          dependencies.delay,
        );
        if (jumpedIntoRecommendedProduct) {
          await dependencies.delay(500);
          continue;
        }
      }

      throw new Error("[OZON_PRODUCT_UNAVAILABLE] Ozon 商品页显示为不可访问或已下架");
    }

    if (snapshotState === "resolved") {
      if (
        !recommendedProductHopAttempted &&
        shouldHopFromResolvedSnapshot(snapshot)
      ) {
        recommendedProductHopAttempted = true;
        const jumpedIntoRecommendedProduct = await hopToFirstRecommendedProductDetail(
          page,
          snapshot.url || canonicalProductUrl,
          dependencies.delay,
        );
        if (jumpedIntoRecommendedProduct) {
          await dependencies.delay(500);
          continue;
        }
      }

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
      return imageBase64
        ? { title, imageUrl, imageBase64, specProfile: snapshot.specProfile }
        : { title, imageUrl, specProfile: snapshot.specProfile };
    }

    if (
      snapshotState === "incomplete" &&
      !recommendedProductHopAttempted &&
      shouldHopFromIncompleteSnapshot(snapshot)
    ) {
      recommendedProductHopAttempted = true;
      const jumpedIntoRecommendedProduct = await hopToFirstRecommendedProductDetail(
        page,
        snapshot.url || canonicalProductUrl,
        dependencies.delay,
      );
      if (jumpedIntoRecommendedProduct) {
        await dependencies.delay(500);
        continue;
      }
    }

    await dependencies.delay(dependencies.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS);
  }

  if (antiBotSeen) {
    throw new Error("[ANTI_BOT_CHALLENGE] Ozon 商品页触发验证且在超时前未解除");
  }

  throw new Error("[OZON_RESOLVE_FAILED] 未从浏览器页解析到 Ozon 商品标题或主图");
}

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
      return imageBase64
        ? { title, imageUrl, imageBase64, specProfile: snapshot.specProfile }
        : { title, imageUrl, specProfile: snapshot.specProfile };
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
