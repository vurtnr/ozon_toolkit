import path from "node:path";
import os from "node:os";
import type { Server } from "node:http";
import express, { type Request, type Response } from "express";
import puppeteer, { type Browser, type Page } from "puppeteer";
import { search1688ByImage } from "./1688_engine";
import { ChromeNotFoundError, findChromePath } from "./chrome-path";
import { ERROR_CODES, type SidecarErrorCode } from "./error-codes";

interface SearchRequestBody {
  imagePath?: string;
  forceFullCrop?: boolean;
}

interface SidecarErrorPayload {
  success: false;
  code: SidecarErrorCode;
  error: string;
}

export interface SessionSnapshot {
  url: string;
  visibleText: string;
  links: string[];
  hasAntiBotChallenge: boolean;
}

export type SessionState = "ready" | "login_required" | "anti_bot_challenge";

type SessionStateHandlerDependencies = {
  ensureBrowserAndPageAlive: () => Promise<Page>;
  collectSessionSnapshot: (page: Page) => Promise<SessionSnapshot>;
  classifySessionState: (snapshot: SessionSnapshot) => SessionState;
  buildErrorPayload: (error: unknown) => SidecarErrorPayload;
};

type ClosablePage = Pick<Page, "close" | "isClosed">;
type ClosableBrowser = Pick<Browser, "close" | "isConnected">;
type ClosableServer = Pick<Server, "close">;

type RuntimeResources = {
  page: ClosablePage | null;
  browser: ClosableBrowser | null;
  server: ClosableServer | null;
};

const app = express();
app.use(express.json());

const CHROME_ARGS = [
  "--start-maximized",
  "--disable-blink-features=AutomationControlled",
  "--disable-infobars",
  "--no-default-browser-check",
  "--disable-dev-shm-usage",
  "--user-agent=Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36",
];
const SIDECAR_PROFILE_DIR_ENV = "SIDECAR_PROFILE_DIR";
const LOGIN_URL_HINTS = [
  "login.1688.com",
  "member.1688.com/member/signin",
  "passport.alibaba.com",
];
const LOGIN_TEXT_HINTS = ["请登录", "免费注册", "会员登录", "登录后可", "登录后"];
const LOGGED_IN_TEXT_HINTS = ["我的阿里", "我的1688", "买家工作台", "采购车", "进货单", "消息"];
const ANTI_BOT_URL_HINTS = ["sec.", "punish", "captcha", "verify"];
const ANTI_BOT_TEXT_HINTS = [
  "请完成验证",
  "请先完成验证",
  "滑动验证",
  "访问验证",
  "网络环境存在异常",
  "请先通过验证",
];

let globalBrowser: Browser | null = null;
let globalHomePage: Page | null = null;
let chromeExecutablePath: string | null = null;
let httpServer: Server | null = null;
let shutdownInFlight: Promise<void> | null = null;

export function createSharedAsyncRunner<T>(
  factory: () => Promise<T>,
): () => Promise<T> {
  let inFlight: Promise<T> | null = null;

  return async () => {
    if (inFlight) {
      return inFlight;
    }

    inFlight = (async () => factory())();
    try {
      return await inFlight;
    } finally {
      inFlight = null;
    }
  };
}

async function resolveChromePath(): Promise<string> {
  if (chromeExecutablePath) {
    return chromeExecutablePath;
  }

  chromeExecutablePath = await findChromePath();
  return chromeExecutablePath;
}

function resolveProfileDir(): string {
  const configured = process.env[SIDECAR_PROFILE_DIR_ENV]?.trim();
  if (configured) {
    return configured;
  }

  return path.join(os.tmpdir(), "desktop-app-sidecar", "1688_profile");
}

function normalizeVisibleText(value: string): string {
  return (value || "").replace(/\s+/g, "");
}

export function classifyLoginRequirement(
  snapshot: SessionSnapshot,
): SidecarErrorCode | null {
  const state = classifySessionState(snapshot);
  return state === "login_required" ? ERROR_CODES.LOGIN_REQUIRED : null;
}

export function classifySessionState(snapshot: SessionSnapshot): SessionState {
  const url = (snapshot.url || "").toLowerCase();
  const visibleText = normalizeVisibleText(snapshot.visibleText);
  const links = snapshot.links.join(" ").toLowerCase();

  if (
    snapshot.hasAntiBotChallenge ||
    ANTI_BOT_URL_HINTS.some((hint) => url.includes(hint)) ||
    ANTI_BOT_TEXT_HINTS.some((hint) => visibleText.includes(hint))
  ) {
    return "anti_bot_challenge";
  }

  if (LOGGED_IN_TEXT_HINTS.some((hint) => visibleText.includes(hint))) {
    return "ready";
  }

  if (
    LOGIN_URL_HINTS.some((hint) => url.includes(hint)) ||
    LOGIN_TEXT_HINTS.some((hint) => visibleText.includes(hint)) ||
    LOGIN_URL_HINTS.some((hint) => links.includes(hint))
  ) {
    return "login_required";
  }

  return "ready";
}

async function collectSessionSnapshot(page: Page): Promise<SessionSnapshot> {
  return page.evaluate(() => ({
    url: window.location.href,
    visibleText: (document.body?.innerText || "").slice(0, 4000),
    links: Array.from(document.querySelectorAll<HTMLAnchorElement>("a[href]"))
      .slice(0, 200)
      .map((element) => element.href || element.getAttribute("href") || "")
      .filter((value) => value.length > 0),
    hasAntiBotChallenge:
      [
        ".nc-container",
        "#baxia-dialog-content",
        "#nc_1_n1z",
        'iframe[src*="punish"]',
        'iframe[src*="captcha"]',
      ].some((selector) => document.querySelector(selector) !== null) ||
      /请完成验证|请先完成验证|滑动验证|访问验证|网络环境存在异常/.test(
        document.body?.innerText || "",
      ),
  }));
}

async function ensure1688SessionReady(page: Page): Promise<void> {
  const state = classifySessionState(await collectSessionSnapshot(page));
  if (state === "login_required") {
    throw new Error("[LOGIN_REQUIRED] 当前 1688 未登录，请先在浏览器完成登录");
  }
  if (state === "anti_bot_challenge") {
    throw new Error("[ANTI_BOT_CHALLENGE] 触发 1688 底层拦截，请先在浏览器完成验证");
  }
}

async function applyBrowserEvasions(page: Page): Promise<void> {
  await page.evaluateOnNewDocument(() => {
    Object.defineProperty(navigator, "webdriver", {
      get: () => undefined,
      configurable: true,
    });

    Object.defineProperty(navigator, "languages", {
      get: () => ["zh-CN", "zh", "en-US", "en"],
      configurable: true,
    });

    Object.defineProperty(navigator, "plugins", {
      get: () => [1, 2, 3, 4],
      configurable: true,
    });

    Object.defineProperty(navigator, "platform", {
      get: () => "MacIntel",
      configurable: true,
    });

    const originalQuery = window.navigator.permissions?.query;
    if (originalQuery) {
      window.navigator.permissions.query = ((parameters: PermissionDescriptor) => {
        if (parameters.name === "notifications") {
          return Promise.resolve({
            name: "notifications",
            onchange: null,
            state: Notification.permission,
            addEventListener() {},
            removeEventListener() {},
            dispatchEvent() {
              return false;
            },
          } as PermissionStatus);
        }
        return originalQuery.call(window.navigator.permissions, parameters);
      }) as typeof window.navigator.permissions.query;
    }
  });
}

async function ensureBrowserAndPageAliveInner(): Promise<Page> {
  if (!globalBrowser || !globalBrowser.isConnected()) {
    if (globalBrowser) {
      await globalBrowser.close().catch(() => undefined);
    }

    const executablePath = await resolveChromePath();
    globalBrowser = await puppeteer.launch({
      headless: false,
      executablePath,
      defaultViewport: null,
      userDataDir: resolveProfileDir(),
      ignoreDefaultArgs: ["--enable-automation"],
      args: CHROME_ARGS,
    });
    globalHomePage = null;
  }

  let needNewPage = false;
  if (!globalHomePage || globalHomePage.isClosed()) {
    needNewPage = true;
  } else {
    try {
      await globalHomePage.bringToFront();
      await globalHomePage.evaluate(() => document.title);
    } catch {
      needNewPage = true;
    }
  }

  if (needNewPage) {
    if (globalHomePage && !globalHomePage.isClosed()) {
      await globalHomePage.close().catch(() => undefined);
    }

    globalHomePage = await globalBrowser.newPage();
    await applyBrowserEvasions(globalHomePage);
    await globalHomePage.goto("https://www.1688.com/", {
      waitUntil: "networkidle2",
      timeout: 60_000,
    });
  }

  return globalHomePage;
}

const ensureBrowserAndPageAlive = createSharedAsyncRunner(
  ensureBrowserAndPageAliveInner,
);

export async function shutdownRuntimeResources(
  resources: RuntimeResources,
): Promise<void> {
  if (resources.page && !resources.page.isClosed()) {
    await resources.page.close().catch(() => undefined);
  }

  if (resources.browser && resources.browser.isConnected()) {
    await resources.browser.close().catch(() => undefined);
  }

  if (resources.server) {
    await new Promise<void>((resolve) => {
      resources.server?.close(() => resolve());
    });
  }
}

async function shutdownSidecarProcess(exitCode?: number): Promise<void> {
  if (shutdownInFlight) {
    return shutdownInFlight;
  }

  shutdownInFlight = (async () => {
    const resources: RuntimeResources = {
      page: globalHomePage,
      browser: globalBrowser,
      server: httpServer,
    };
    globalHomePage = null;
    globalBrowser = null;
    httpServer = null;

    await shutdownRuntimeResources(resources);
  })();

  try {
    await shutdownInFlight;
  } finally {
    shutdownInFlight = null;
    if (typeof exitCode === "number") {
      process.exit(exitCode);
    }
  }
}

function registerShutdownSignals(): void {
  const handleSignal = (exitCode: number) => {
    void shutdownSidecarProcess(exitCode);
  };

  process.on("SIGINT", () => handleSignal(0));
  process.on("SIGTERM", () => handleSignal(0));
}

function buildErrorPayload(error: unknown): SidecarErrorPayload {
  if (error instanceof ChromeNotFoundError) {
    return {
      success: false,
      code: ERROR_CODES.CHROME_NOT_FOUND,
      error: error.message,
    };
  }

  const message = error instanceof Error ? error.message : String(error);

  if (message.includes("[LOGIN_REQUIRED]")) {
    return {
      success: false,
      code: ERROR_CODES.LOGIN_REQUIRED,
      error: message,
    };
  }

  if (message.includes("[ANTI_BOT_CHALLENGE]")) {
    return {
      success: false,
      code: ERROR_CODES.ANTI_BOT_CHALLENGE,
      error: message,
    };
  }

  if (message.includes("[FULL_CROP_NOT_APPLIED]")) {
    return {
      success: false,
      code: ERROR_CODES.FULL_CROP_NOT_APPLIED,
      error: message,
    };
  }

  if (
    message.includes("[IMAGE_SEARCH_NOT_ENTERED_RESULT_PAGE]") ||
    message.includes("未能成功进入搜索结果页")
  ) {
    return {
      success: false,
      code: ERROR_CODES.IMAGE_SEARCH_NOT_ENTERED_RESULT_PAGE,
      error: message,
    };
  }

  return {
    success: false,
    code: ERROR_CODES.UNKNOWN,
    error: message,
  };
}

export function createHealthHandler() {
  return async (_req: Request, res: Response) => {
    return res.json({ success: true });
  };
}

export function createSessionStateHandler(
  dependencies: SessionStateHandlerDependencies = {
    ensureBrowserAndPageAlive,
    collectSessionSnapshot,
    classifySessionState,
    buildErrorPayload,
  },
) {
  return async (_req: Request, res: Response) => {
    try {
      const activeHomePage = await dependencies.ensureBrowserAndPageAlive();
      const status = dependencies.classifySessionState(
        await dependencies.collectSessionSnapshot(activeHomePage),
      );
      return res.json({ success: true, status });
    } catch (error) {
      const payload = dependencies.buildErrorPayload(error);
      const statusCode = payload.code === ERROR_CODES.UNKNOWN ? 500 : 200;
      return res.status(statusCode).json(payload);
    }
  };
}

app.post("/search", async (req: Request<unknown, unknown, SearchRequestBody>, res: Response) => {
  const imagePath = req.body?.imagePath;
  const forceFullCrop = !!req.body?.forceFullCrop;

  if (!imagePath) {
    return res.status(400).json({
      success: false,
      code: ERROR_CODES.UNKNOWN,
      error: "missing imagePath",
    });
  }

  const runSearch = async () => {
    const activeHomePage = await ensureBrowserAndPageAlive();
    await ensure1688SessionReady(activeHomePage);
    return search1688ByImage(globalBrowser as Browser, activeHomePage, imagePath, forceFullCrop, []);
  };

  try {
    let candidates;
    try {
      candidates = await runSearch();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      const shouldRetry = /Session closed|Target closed|Protocol error/i.test(message);
      if (!shouldRetry) {
        throw error;
      }

      globalHomePage = null;
      candidates = await runSearch();
    }

    return res.json({ success: true, data: candidates });
  } catch (error) {
    const payload = buildErrorPayload(error);
    const statusCode = payload.code === ERROR_CODES.UNKNOWN ? 500 : 200;
    return res.status(statusCode).json(payload);
  }
});

app.get("/health", createHealthHandler());

app.get("/session-state", createSessionStateHandler());

app.post("/shutdown", async (_req: Request, res: Response) => {
  res.json({ success: true });
  setTimeout(() => {
    void shutdownSidecarProcess(0);
  }, 0);
});

const PORT = 8266;

async function bootstrapServer(): Promise<void> {
  registerShutdownSignals();
  httpServer = app.listen(PORT, "127.0.0.1", () => {
    console.log(`sidecar ready on 127.0.0.1:${PORT}`);
  });
}

if (import.meta.main) {
  await bootstrapServer();
}
