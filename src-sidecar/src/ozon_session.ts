import type { Browser, Page } from "puppeteer";

export interface OzonResolvePayload {
  title: string;
  imageUrl: string;
  imageBase64?: string;
}

export interface OzonSnapshot {
  url: string;
  documentTitle: string;
  title: string | null;
  imageUrl: string | null;
  bodyText: string;
  hasAntiBotChallenge: boolean;
  isUnavailable: boolean;
}

export type OzonPageState =
  | "resolved"
  | "anti_bot_challenge"
  | "unavailable"
  | "incomplete";

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
  "страница не найдена",
  "извините, такой страницы нет",
  "товар закончился",
  "нет в наличии",
];

export async function collectOzonSnapshot(page: Page): Promise<OzonSnapshot> {
  return page.evaluate(
    (antiBotUrlHints: string[], antiBotTextHints: string[], unavailableTextHints: string[]) => {
      const readJsonLdProduct = (): { title: string | null; imageUrl: string | null } => {
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

              return { title, imageUrl };
            }
          } catch {}
        }

        return { title: null, imageUrl: null };
      };

      const jsonLd = readJsonLdProduct();
      const rawTitle =
        jsonLd.title ||
        document.querySelector('meta[property="og:title"]')?.getAttribute("content") ||
        document.querySelector("h1")?.textContent ||
        document.title ||
        null;
      const rawImageUrl =
        jsonLd.imageUrl ||
        document.querySelector('meta[property="og:image"]')?.getAttribute("content") ||
        document.querySelector("img")?.getAttribute("src") ||
        null;
      const bodyText = (document.body?.innerText || "").slice(0, 20_000);
      const normalizedBody = bodyText.toLowerCase();
      const normalizedTitle = (document.title || "").toLowerCase();
      const normalizedUrl = window.location.href.toLowerCase();

      return {
        url: window.location.href,
        documentTitle: document.title || "",
        title: rawTitle ? rawTitle.trim() : null,
        imageUrl: rawImageUrl ? rawImageUrl.trim() : null,
        bodyText,
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

  if (snapshot.isUnavailable) {
    return "unavailable";
  }

  const title = normalizeOzonTitle(snapshot.title);
  const imageUrl = normalizeOzonImageUrl(snapshot);
  if (title && imageUrl) {
    return "resolved";
  }

  return "incomplete";
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
      const score = await handle.evaluate((img, expected) => {
        const rect = img.getBoundingClientRect();
        const currentSrc = (img.currentSrc || img.getAttribute("src") || "").trim();
        const naturalWidth = img.naturalWidth || 0;
        const naturalHeight = img.naturalHeight || 0;
        const renderedArea = Math.max(rect.width, 0) * Math.max(rect.height, 0);
        const naturalArea = naturalWidth * naturalHeight;
        const isVisible = rect.width >= 40 && rect.height >= 40;
        const isLikelyProductImage =
          naturalWidth >= 120 &&
          naturalHeight >= 120 &&
          rect.top < window.innerHeight + 200 &&
          rect.bottom > -200;

        if (!isVisible || !isLikelyProductImage) {
          return Number.NEGATIVE_INFINITY;
        }

        let nextScore = naturalArea + renderedArea;
        if (expected && currentSrc === expected) {
          nextScore += 1_000_000_000;
        }
        if (rect.top >= 0 && rect.top <= window.innerHeight) {
          nextScore += 100_000;
        }

        return nextScore;
      }, normalizedExpected);

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

  await page.goto(dependencies.landingUrl ?? DEFAULT_OZON_HOME_URL, {
    waitUntil: "domcontentloaded",
    timeout: 60_000,
  });
  await dependencies.delay(750);
}

export async function resolveOzonProductViaSession(
  dependencies: ResolveOzonProductDependencies,
  productUrl: string,
): Promise<OzonResolvePayload> {
  const page = await ensureOzonSessionPage(dependencies);
  await warmOzonSession(dependencies, page);

  await page.goto(productUrl, {
    waitUntil: "domcontentloaded",
    timeout: 60_000,
  });

  const deadline =
    Date.now() + (dependencies.resolveTimeoutMs ?? DEFAULT_RESOLVE_TIMEOUT_MS);
  let antiBotSeen = false;

  while (Date.now() < deadline) {
    const snapshot = await collectOzonSnapshot(page);
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
      throw new Error("[OZON_PRODUCT_UNAVAILABLE] Ozon 商品页显示为不可访问或已下架");
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
    throw new Error("[ANTI_BOT_CHALLENGE] Ozon 商品页触发验证且在超时前未解除");
  }

  throw new Error("[OZON_RESOLVE_FAILED] 未从浏览器页解析到 Ozon 商品标题或主图");
}
