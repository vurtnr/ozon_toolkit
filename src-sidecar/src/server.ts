import path from "node:path";
import os from "node:os";
import type { Server } from "node:http";
import { spawn, type ChildProcess } from "node:child_process";
import { readFile, readlink, rm } from "node:fs/promises";
import { createServer } from "node:net";
import express, { type Request, type Response } from "express";
import puppeteer, { type Browser, type Page } from "puppeteer";
import {
  extract1688DetailFreight,
  search1688ByImage,
  shouldEnsureHomePageBeforeSessionCheck,
} from "./1688_engine";
import { ChromeNotFoundError, findChromePath } from "./chrome-path";
import { ERROR_CODES, type SidecarErrorCode } from "./error-codes";
import {
  resolveOzonProductViaSession,
  resolveOzonSkuViaSession,
  selectPreferredOzonSessionPage,
  type OzonResolvePayload,
} from "./ozon_session";

interface SearchRequestBody {
  imagePath?: string;
  forceFullCrop?: boolean;
}

interface OzonResolveRequestBody {
  productUrl?: string;
}

interface OzonSkuResolveRequestBody {
  sku?: string;
}

interface Resolve1688DetailPricingRequestBody {
  itemUrl?: string;
  cardPrice?: string;
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
  hasLoginEntry: boolean;
  hasLoggedInEntry: boolean;
}

export type SessionState = "ready" | "login_required" | "anti_bot_challenge";

type SessionStateHandlerDependencies = {
  ensureBrowserAndPageAlive: () => Promise<Page>;
  collectSessionSnapshot: (page: Page) => Promise<SessionSnapshot>;
  classifySessionState: (snapshot: SessionSnapshot) => SessionState;
  classifySessionStates: (snapshots: SessionSnapshot[]) => SessionState;
  listCandidatePages: (primaryPage: Page) => Promise<Page[]>;
  buildErrorPayload: (error: unknown) => SidecarErrorPayload;
};

type ClosablePage = Pick<Page, "close" | "isClosed">;
type ClosableBrowser = Pick<Browser, "close" | "isConnected">;
type ClosableServer = Pick<Server, "close">;

type RuntimeResources = {
  page: ClosablePage | null;
  ozonPage: ClosablePage | null;
  browser: ClosableBrowser | null;
  ozonBrowser: ClosableBrowser | null;
  server: ClosableServer | null;
};

const app = express();
app.use(express.json());

const SIDECAR_PROFILE_DIR_ENV = "SIDECAR_PROFILE_DIR";
const HOME_URL_HINTS = ["https://www.1688.com/", "https://m.1688.com/"];
const LOGIN_URL_HINTS = [
  "login.1688.com",
  "member.1688.com/member/signin",
  "passport.alibaba.com",
];
const LOGIN_TEXT_HINTS = ["请登录", "免费注册", "会员登录", "登录后可", "登录后"];
const LOGGED_IN_TEXT_HINTS = ["买家工作台"];
const ANTI_BOT_URL_HINTS = ["sec.", "punish", "captcha", "verify"];
const ANTI_BOT_TEXT_HINTS = [
  "请完成验证",
  "请先完成验证",
  "滑动验证",
  "访问验证",
  "网络环境存在异常",
  "请先通过验证",
];
const PROFILE_RUNTIME_ENTRY_NAMES = [
  "DevToolsActivePort",
  "SingletonLock",
  "SingletonCookie",
  "SingletonSocket",
];

export function browserUserAgentForPlatform(
  platform: NodeJS.Platform = process.platform,
): string {
  if (platform === "win32") {
    return "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
  }

  if (platform === "darwin") {
    return "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
  }

  return "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36";
}

export function browserNavigatorPlatformForPlatform(
  platform: NodeJS.Platform = process.platform,
): string {
  if (platform === "win32") {
    return "Win32";
  }
  if (platform === "darwin") {
    return "MacIntel";
  }
  return "Linux x86_64";
}

function buildChromeArgs(
  platform: NodeJS.Platform = process.platform,
): string[] {
  return [
    "--start-maximized",
    "--disable-blink-features=AutomationControlled",
    "--disable-infobars",
    "--no-default-browser-check",
    "--disable-dev-shm-usage",
    `--user-agent=${browserUserAgentForPlatform(platform)}`,
  ];
}

let globalBrowser: Browser | null = null;
let globalHomePage: Page | null = null;
let globalOzonBrowser: Browser | null = null;
let globalOzonPage: Page | null = null;
let chromeExecutablePath: string | null = null;
let httpServer: Server | null = null;
let shutdownInFlight: Promise<void> | null = null;
let ozonBrowserChild: ChildProcess | null = null;

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

function resolveOzonProfileDir(): string {
  const baseProfileDir = resolveProfileDir();
  const baseName = path.basename(baseProfileDir);
  if (baseName === "1688_profile") {
    return path.join(path.dirname(baseProfileDir), "ozon_profile");
  }
  return `${baseProfileDir}-ozon`;
}

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

export function parseChromeDevToolsPort(content: string): number | null {
  const firstLine = content.split(/\r?\n/, 1)[0]?.trim() || "";
  if (!/^\d+$/.test(firstLine)) {
    return null;
  }

  const port = Number.parseInt(firstLine, 10);
  return Number.isInteger(port) && port > 0 ? port : null;
}

export function extractChromeSingletonLockPid(target: string): number | null {
  const match = target.trim().match(/anonymous-(\d+)$/);
  if (!match) {
    return null;
  }

  const pid = Number.parseInt(match[1], 10);
  return Number.isInteger(pid) && pid > 0 ? pid : null;
}

async function readChromeDevToolsPort(profileDir: string): Promise<number | null> {
  try {
    const content = await readFile(
      path.join(profileDir, "DevToolsActivePort"),
      "utf8",
    );
    return parseChromeDevToolsPort(content);
  } catch {
    return null;
  }
}

function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function terminateProcess(pid: number): Promise<void> {
  try {
    process.kill(pid, "SIGTERM");
  } catch {
    return;
  }

  const deadline = Date.now() + 3_000;
  while (Date.now() < deadline) {
    if (!isPidAlive(pid)) {
      return;
    }
    await delay(150);
  }

  try {
    process.kill(pid, "SIGKILL");
  } catch {}
}

async function connectToExistingProfileBrowser(
  profileDir: string,
): Promise<Browser | null> {
  const devtoolsPort = await readChromeDevToolsPort(profileDir);
  if (!devtoolsPort) {
    return null;
  }

  try {
    return await puppeteer.connect({
      browserURL: `http://127.0.0.1:${devtoolsPort}`,
      defaultViewport: null,
    });
  } catch {
    return null;
  }
}

async function findAvailableTcpPort(): Promise<number> {
  return await new Promise<number>((resolve, reject) => {
    const server = createServer();
    server.unref();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port =
        typeof address === "object" && address ? address.port : 0;
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve(port);
      });
    });
  });
}

async function connectToBrowserByPort(port: number): Promise<Browser | null> {
  try {
    return await puppeteer.connect({
      browserURL: `http://127.0.0.1:${port}`,
      defaultViewport: null,
    });
  } catch {
    return null;
  }
}

async function waitForBrowserConnectionByPort(
  port: number,
  timeoutMs: number = 20_000,
): Promise<Browser> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const browser = await connectToBrowserByPort(port);
    if (browser) {
      return browser;
    }
    await delay(200);
  }

  throw new Error("[OZON_BROWSER_CONNECT_FAILED] 无法连接到 Ozon 浏览器调试端口");
}

async function cleanupStaleProfileRuntime(profileDir: string): Promise<void> {
  try {
    const singletonTarget = await readlink(path.join(profileDir, "SingletonLock"));
    const pid = extractChromeSingletonLockPid(singletonTarget);
    if (pid) {
      await terminateProcess(pid);
    }
  } catch {}

  await Promise.all(
    PROFILE_RUNTIME_ENTRY_NAMES.map((entry) =>
      rm(path.join(profileDir, entry), {
        force: true,
        recursive: true,
      }).catch(() => undefined),
    ),
  );
}

async function launchOzonBrowserProcess(
  executablePath: string,
  profileDir: string,
): Promise<Browser> {
  await cleanupStaleProfileRuntime(profileDir);
  const remoteDebuggingPort = await findAvailableTcpPort();
  const child = spawn(
    executablePath,
    buildOzonChromeArgs(profileDir, remoteDebuggingPort),
    {
      stdio: "ignore",
    },
  );
  ozonBrowserChild = child;

  try {
    const browser = await waitForBrowserConnectionByPort(remoteDebuggingPort);
    child.once("exit", () => {
      if (ozonBrowserChild?.pid === child.pid) {
        ozonBrowserChild = null;
      }
    });
    return browser;
  } catch (error) {
    if (child.pid) {
      await terminateProcess(child.pid).catch(() => undefined);
    }
    if (ozonBrowserChild?.pid === child.pid) {
      ozonBrowserChild = null;
    }
    throw error;
  }
}

function normalizeVisibleText(value: string): string {
  return (value || "").replace(/\s+/g, "");
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function is1688HomeUrl(url: string): boolean {
  const normalized = (url || "").trim().toLowerCase();
  return HOME_URL_HINTS.some((hint) => normalized.startsWith(hint));
}

export function classifyLoginRequirement(
  snapshot: SessionSnapshot,
): SidecarErrorCode | null {
  const state = classifySessionState(snapshot);
  return state === "login_required" ? ERROR_CODES.LOGIN_REQUIRED : null;
}

function snapshotHasStrongLoggedInSignal(snapshot: SessionSnapshot): boolean {
  const visibleText = normalizeVisibleText(snapshot.visibleText);

  return (
    snapshot.hasLoggedInEntry ||
    LOGGED_IN_TEXT_HINTS.some((hint) => visibleText.includes(hint))
  );
}

function snapshotHasLoginSignal(snapshot: SessionSnapshot): boolean {
  const url = (snapshot.url || "").toLowerCase();
  const visibleText = normalizeVisibleText(snapshot.visibleText);
  const links = snapshot.links.join(" ").toLowerCase();

  return (
    snapshot.hasLoginEntry ||
    LOGIN_URL_HINTS.some((hint) => url.includes(hint)) ||
    LOGIN_TEXT_HINTS.some((hint) => visibleText.includes(hint)) ||
    LOGIN_URL_HINTS.some((hint) => links.includes(hint))
  );
}

export function classifySessionState(snapshot: SessionSnapshot): SessionState {
  const hasStrongLoggedInSignal = snapshotHasStrongLoggedInSignal(snapshot);

  if (
    snapshot.hasAntiBotChallenge ||
    ANTI_BOT_URL_HINTS.some((hint) => (snapshot.url || "").toLowerCase().includes(hint)) ||
    ANTI_BOT_TEXT_HINTS.some((hint) =>
      normalizeVisibleText(snapshot.visibleText).includes(hint),
    )
  ) {
    return "anti_bot_challenge";
  }

  if (snapshotHasLoginSignal(snapshot)) {
    return "login_required";
  }

  if (hasStrongLoggedInSignal) {
    return "ready";
  }

  if (is1688HomeUrl((snapshot.url || "").toLowerCase())) {
    return "login_required";
  }

  return "login_required";
}

export function classifySessionStates(snapshots: SessionSnapshot[]): SessionState {
  if (snapshots.length === 0) {
    return "login_required";
  }

  const states = snapshots.map((snapshot) => classifySessionState(snapshot));
  if (states.includes("anti_bot_challenge")) {
    return "anti_bot_challenge";
  }
  const primarySnapshot = snapshots[0];
  const primaryState = states[0];
  if (
    primarySnapshot &&
    primaryState === "login_required" &&
    snapshotHasLoginSignal(primarySnapshot)
  ) {
    return "login_required";
  }
  if (states.includes("ready")) {
    return "ready";
  }
  return "login_required";
}

async function collectSessionSnapshot(page: Page): Promise<SessionSnapshot> {
  return page.evaluate(() => {
    const isVisible = (element: Element | null) => {
      if (!(element instanceof HTMLElement)) {
        return false;
      }
      const style = window.getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return (
        style.display !== "none" &&
        style.visibility !== "hidden" &&
        rect.width > 0 &&
        rect.height > 0
      );
    };

    const visibleAnchors = Array.from(
      document.querySelectorAll<HTMLAnchorElement>("a[href]"),
    ).filter((element) => isVisible(element));
    const visibleElements = Array.from(
      document.querySelectorAll<HTMLElement>("button, a, div, span"),
    ).filter((element) => isVisible(element));

    return {
      url: window.location.href,
      visibleText: (document.body?.innerText || "").slice(0, 12000),
      links: visibleAnchors
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
      hasLoginEntry:
        visibleAnchors.some((element) => {
          const href = element.href || element.getAttribute("href") || "";
          return (
            href.includes("login.1688.com") ||
            href.includes("member/signin") ||
            href.includes("passport.alibaba.com")
          );
        }) ||
        visibleElements.some((element) => {
          const text = (element.innerText || element.textContent || "").replace(/\s+/g, "");
          return text === "登录" || text === "请登录" || text === "免费注册";
        }),
      hasLoggedInEntry:
        visibleAnchors.some((element) => {
          const href = element.href || element.getAttribute("href") || "";
          return href.includes("work.1688.com/home/page");
        }) ||
        visibleElements.some((element) => {
          const text = (element.innerText || element.textContent || "").replace(/\s+/g, "");
          return text.includes("买家工作台");
        }),
    };
  });
}

async function listCandidatePages(primaryPage: Page): Promise<Page[]> {
  const pages = new Set<Page>([primaryPage]);
  if (globalBrowser && globalBrowser.isConnected()) {
    for (const page of await globalBrowser.pages().catch(() => [])) {
      pages.add(page);
    }
  }
  return [...pages].filter((page) => !page.isClosed());
}

async function ensurePageReadyForSessionCheck(page: Page): Promise<Page> {
  try {
    await page.bringToFront();
  } catch {}

  if (shouldEnsureHomePageBeforeSessionCheck(page.url())) {
    await page.goto("https://www.1688.com/", {
      waitUntil: "domcontentloaded",
      timeout: 60_000,
    });
  }

  return page;
}

async function ensure1688SessionReady(page: Page): Promise<void> {
  const state = classifySessionState(
    await collectSessionSnapshot(await ensurePageReadyForSessionCheck(page)),
  );
  if (state === "login_required") {
    throw new Error("[LOGIN_REQUIRED] 当前 1688 未登录，请先在浏览器完成登录");
  }
  if (state === "anti_bot_challenge") {
    throw new Error("[ANTI_BOT_CHALLENGE] 触发 1688 底层拦截，请先在浏览器完成验证");
  }
}

async function ensure1688DetailPageAccessible(page: Page): Promise<void> {
  const snapshot = await collectSessionSnapshot(page);
  if (snapshot.hasAntiBotChallenge) {
    throw new Error("[ANTI_BOT_CHALLENGE] 触发 1688 底层拦截，请先在浏览器完成验证");
  }
  if (snapshotHasLoginSignal(snapshot)) {
    throw new Error("[LOGIN_REQUIRED] 当前 1688 未登录，请先在浏览器完成登录");
  }
}

async function collect1688DetailFreightSignals(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const isVisible = (element: Element | null): element is HTMLElement => {
      if (!(element instanceof HTMLElement)) {
        return false;
      }
      const style = window.getComputedStyle(element);
      const rect = element.getBoundingClientRect();
      return (
        style.display !== "none" &&
        style.visibility !== "hidden" &&
        rect.width > 0 &&
        rect.height > 0
      );
    };

    const values = new Set<string>();
    const pushText = (value: string | null | undefined) => {
      const normalized = (value || "").replace(/\s+/g, " ").trim();
      if (!normalized) {
        return;
      }
      values.add(normalized);
    };

    const selectors = [
      ".service-item",
      '[class*="service-item"]',
      '[class*="serviceItem"]',
      '[class*="freight"]',
      '[class*="logistics"]',
    ];

    const serviceNodes = Array.from(
      document.querySelectorAll<HTMLElement>(selectors.join(",")),
    );
    for (const element of serviceNodes.slice(0, 120)) {
      if (!isVisible(element)) {
        continue;
      }
      pushText(element.innerText || element.textContent);
      pushText(element.parentElement?.innerText || element.parentElement?.textContent);
      pushText(
        `${element.previousElementSibling?.textContent || ""} ${element.innerText || element.textContent || ""}`,
      );
      pushText(
        `${element.innerText || element.textContent || ""} ${element.nextElementSibling?.textContent || ""}`,
      );
    }

    if (values.size === 0) {
      const fallbackNodes = Array.from(
        document.querySelectorAll<HTMLElement>("div, span, em, b"),
      );
      for (const element of fallbackNodes.slice(0, 300)) {
        if (!isVisible(element)) {
          continue;
        }
        const text = (element.innerText || element.textContent || "").replace(/\s+/g, " ").trim();
        if (!text) {
          continue;
        }
        if (
          text.includes("包邮") ||
          text.includes("运费") ||
          text.includes("物流") ||
          text.includes("配送")
        ) {
          pushText(text);
        }
      }
    }

    return [...values];
  });
}

async function resolve1688DetailPricingViaBrowser(itemUrl: string): Promise<{
  freightText: string | null;
  freightValue: number | null;
  isFreeShipping: boolean;
}> {
  const browser = await ensureBrowserAlive();
  const detailPage = await browser.newPage();

  try {
    await applyBrowserEvasions(detailPage);
    await detailPage.goto(itemUrl, {
      waitUntil: "domcontentloaded",
      timeout: 60_000,
    });
    await delay(1_200);
    await ensure1688DetailPageAccessible(detailPage);
    await detailPage.waitForNetworkIdle({ timeout: 3_000 }).catch(() => undefined);

    const signals = await collect1688DetailFreightSignals(detailPage);
    const freight = extract1688DetailFreight(signals);

    return freight ?? {
      freightText: null,
      freightValue: null,
      isFreeShipping: false,
    };
  } finally {
    if (!detailPage.isClosed()) {
      await detailPage.close().catch(() => undefined);
    }
  }
}

async function resolveOzonProductViaBrowser(productUrl: string): Promise<OzonResolvePayload> {
  await ensureOzonBrowserAlive();
  try {
    return await resolveOzonProductViaSession(
      {
        browser: globalOzonBrowser as Browser,
        getSessionPage: () => globalOzonPage,
        setSessionPage: (page) => {
          globalOzonPage = page;
        },
        applyBrowserEvasions: async () => undefined,
        delay,
      },
      productUrl,
    );
  } catch (error) {
    throw error;
  }
}

async function resolveOzonSkuViaBrowser(sku: string): Promise<OzonResolvePayload> {
  await ensureOzonBrowserAlive();
  try {
    return await resolveOzonSkuViaSession(
      {
        browser: globalOzonBrowser as Browser,
        getSessionPage: () => globalOzonPage,
        setSessionPage: (page) => {
          globalOzonPage = page;
        },
        applyBrowserEvasions: async () => undefined,
        delay,
      },
      sku,
    );
  } catch (error) {
    throw error;
  }
}

async function closeOzonSessionPage(): Promise<void> {
  if (globalOzonPage && !globalOzonPage.isClosed()) {
    await globalOzonPage.close().catch(() => undefined);
  }
  globalOzonPage = null;
  if (globalOzonBrowser && globalOzonBrowser.isConnected()) {
    await globalOzonBrowser.close().catch(() => undefined);
  }
  globalOzonBrowser = null;
  ozonBrowserChild = null;
}

async function applyBrowserEvasions(page: Page): Promise<void> {
  const navigatorPlatform = browserNavigatorPlatformForPlatform();
  await page.evaluateOnNewDocument((injectedPlatform: string) => {
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
      get: () => injectedPlatform,
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
  }, navigatorPlatform);
}

async function ensureBrowserAliveInner(): Promise<Browser> {
  if (!globalBrowser || !globalBrowser.isConnected()) {
    if (globalBrowser) {
      await globalBrowser.close().catch(() => undefined);
    }

    const executablePath = await resolveChromePath();
    const profileDir = resolveProfileDir();
    const launchBrowser = () =>
      puppeteer.launch({
        headless: false,
        executablePath,
        defaultViewport: null,
        userDataDir: profileDir,
        ignoreDefaultArgs: ["--enable-automation"],
        args: buildChromeArgs(),
      });

    globalBrowser =
      (await connectToExistingProfileBrowser(profileDir)) ??
      null;

    if (!globalBrowser) {
      try {
        globalBrowser = await launchBrowser();
      } catch (error) {
        const recovered = await connectToExistingProfileBrowser(profileDir);
        if (recovered) {
          globalBrowser = recovered;
        } else {
          const message = error instanceof Error ? error.message : String(error);
          const likelyProfileConflict =
            /existing browser session|Failed to launch the browser process/i.test(message);
          if (!likelyProfileConflict) {
            throw error;
          }

          await cleanupStaleProfileRuntime(profileDir);
          globalBrowser = await launchBrowser();
        }
      }
    }
    globalHomePage = null;
    globalOzonPage = null;
  }

  return globalBrowser;
}

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

async function ensureBrowserAndPageAliveInner(): Promise<Page> {
  await ensureBrowserAliveInner();

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

  return ensurePageReadyForSessionCheck(globalHomePage);
}

const ensureBrowserAlive = createSharedAsyncRunner(
  ensureBrowserAliveInner,
);

const ensureOzonBrowserAlive = createSharedAsyncRunner(
  ensureOzonBrowserAliveInner,
);

const ensureBrowserAndPageAlive = createSharedAsyncRunner(
  ensureBrowserAndPageAliveInner,
);

export async function shutdownRuntimeResources(
  resources: RuntimeResources,
): Promise<void> {
  if (resources.page && !resources.page.isClosed()) {
    await resources.page.close().catch(() => undefined);
  }

  if (resources.ozonPage && !resources.ozonPage.isClosed()) {
    await resources.ozonPage.close().catch(() => undefined);
  }

  if (resources.ozonBrowser && resources.ozonBrowser.isConnected()) {
    await resources.ozonBrowser.close().catch(() => undefined);
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
      ozonPage: globalOzonPage,
      browser: globalBrowser,
      ozonBrowser: globalOzonBrowser,
      server: httpServer,
    };
    globalHomePage = null;
    globalOzonPage = null;
    globalBrowser = null;
    globalOzonBrowser = null;
    httpServer = null;
    ozonBrowserChild = null;

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

  if (message.includes("[OZON_SKU_NOT_FOUND]")) {
    return {
      success: false,
      code: ERROR_CODES.OZON_SKU_NOT_FOUND,
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
    classifySessionStates,
    listCandidatePages,
    buildErrorPayload,
  },
) {
  return async (_req: Request, res: Response) => {
    try {
      const activeHomePage = await ensurePageReadyForSessionCheck(
        await dependencies.ensureBrowserAndPageAlive(),
      );
      const pages = await dependencies.listCandidatePages(activeHomePage);
      const snapshots = await Promise.all(
        pages.map(async (page) => {
          try {
            return await dependencies.collectSessionSnapshot(page);
          } catch {
            return null;
          }
        }),
      );
      const availableSnapshots = snapshots.filter(
        (snapshot): snapshot is SessionSnapshot => snapshot !== null,
      );
      const status =
        availableSnapshots.length > 0
          ? dependencies.classifySessionStates(availableSnapshots)
          : dependencies.classifySessionState(
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

app.post(
  "/resolve-1688-detail-pricing",
  async (
    req: Request<unknown, unknown, Resolve1688DetailPricingRequestBody>,
    res: Response,
  ) => {
    const itemUrl = (req.body?.itemUrl || "").trim();
    if (!itemUrl) {
      return res.status(400).json({
        success: false,
        code: ERROR_CODES.UNKNOWN,
        error: "missing itemUrl",
      });
    }

    try {
      const data = await resolve1688DetailPricingViaBrowser(itemUrl);
      return res.json({ success: true, data });
    } catch (error) {
      const payload = buildErrorPayload(error);
      const statusCode = payload.code === ERROR_CODES.UNKNOWN ? 500 : 200;
      return res.status(statusCode).json(payload);
    }
  },
);

app.post(
  "/resolve-ozon-product",
  async (req: Request<unknown, unknown, OzonResolveRequestBody>, res: Response) => {
    const productUrl = (req.body?.productUrl || "").trim();
    if (!productUrl) {
      return res.status(400).json({
        success: false,
        code: ERROR_CODES.UNKNOWN,
        error: "missing productUrl",
      });
    }

    try {
      const data = await resolveOzonProductViaBrowser(productUrl);
      return res.json({ success: true, data });
    } catch (error) {
      const payload = buildErrorPayload(error);
      const statusCode = payload.code === ERROR_CODES.UNKNOWN ? 500 : 200;
      return res.status(statusCode).json(payload);
    }
  },
);

app.post(
  "/resolve-ozon-sku",
  async (req: Request<unknown, unknown, OzonSkuResolveRequestBody>, res: Response) => {
    const sku = (req.body?.sku || "").trim();
    if (!sku) {
      return res.status(400).json({
        success: false,
        code: ERROR_CODES.UNKNOWN,
        error: "missing sku",
      });
    }

    try {
      const data = await resolveOzonSkuViaBrowser(sku);
      return res.json({ success: true, data });
    } catch (error) {
      const payload = buildErrorPayload(error);
      const statusCode = payload.code === ERROR_CODES.UNKNOWN ? 500 : 200;
      return res.status(statusCode).json(payload);
    }
  },
);

app.post("/close-ozon-session", async (_req: Request, res: Response) => {
  try {
    await closeOzonSessionPage();
    return res.json({ success: true });
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
