import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export type RuntimePlatform = "windows" | "macos" | "linux" | "unknown";

export interface AppSettings {
  dashscopeApiKey: string;
  chromeExecutablePath: string;
  profitRatio: string;
}

interface AppSettingsWire {
  dashscope_api_key: string;
  chrome_executable_path: string;
  profit_ratio?: string;
}

export interface ChromeDialogFilter {
  name: string;
  extensions: string[];
}

export function createDefaultSettings(): AppSettings {
  return {
    dashscopeApiKey: "",
    chromeExecutablePath: "",
    profitRatio: "",
  };
}

export function sanitizeProfitRatioInput(value: string): string {
  const filtered = value.replace(/[^\d.]/g, "");
  const firstDot = filtered.indexOf(".");
  if (firstDot < 0) {
    return filtered;
  }

  const integerPart = filtered.slice(0, firstDot) || "0";
  const decimalPart = filtered
    .slice(firstDot + 1)
    .replace(/\./g, "")
    .slice(0, 2);
  return `${integerPart}.${decimalPart}`;
}

export function isValidProfitRatioInput(value: string): boolean {
  const normalized = value.trim();
  if (!/^\d+(?:\.\d{1,2})?$/.test(normalized)) {
    return false;
  }

  const parsed = Number.parseFloat(normalized);
  return Number.isFinite(parsed) && parsed > 0 && parsed < 100;
}

export function normalizeChromeExecutablePath(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) {
    return "";
  }

  const normalized = trimmed.replace(/\/+$/, "");
  if (/\.app\/Contents\/MacOS\/[^/]+$/i.test(normalized)) {
    return normalized;
  }

  const bundleMatch = normalized.match(/^(.*\/([^/]+)\.app)$/i);
  if (!bundleMatch) {
    return normalized;
  }

  const appBundlePath = bundleMatch[1];
  const executableName = bundleMatch[2];
  return `${appBundlePath}/Contents/MacOS/${executableName}`;
}

export function coerceSettings(input: Partial<AppSettings> | undefined): AppSettings {
  return {
    dashscopeApiKey: input?.dashscopeApiKey ?? "",
    chromeExecutablePath: normalizeChromeExecutablePath(
      input?.chromeExecutablePath ?? "",
    ),
    profitRatio: sanitizeProfitRatioInput(input?.profitRatio ?? ""),
  };
}

function fromWire(wire: AppSettingsWire | null | undefined): AppSettings {
  if (!wire) return createDefaultSettings();
  return coerceSettings({
    dashscopeApiKey: wire.dashscope_api_key,
    chromeExecutablePath: wire.chrome_executable_path,
    profitRatio: wire.profit_ratio ?? "",
  });
}

function toWire(settings: AppSettings): AppSettingsWire {
  return {
    dashscope_api_key: settings.dashscopeApiKey,
    chrome_executable_path: normalizeChromeExecutablePath(
      settings.chromeExecutablePath,
    ),
    profit_ratio: sanitizeProfitRatioInput(settings.profitRatio),
  };
}

export function detectPlatformFromUserAgent(userAgent: string): RuntimePlatform {
  const ua = userAgent.toLowerCase();
  if (ua.includes("windows")) return "windows";
  if (ua.includes("mac os") || ua.includes("macintosh")) return "macos";
  if (ua.includes("linux")) return "linux";
  return "unknown";
}

export function normalizeRuntimePlatform(value: string): RuntimePlatform {
  const lowered = value.toLowerCase();
  if (lowered.includes("windows")) return "windows";
  if (lowered.includes("macos") || lowered.includes("darwin")) return "macos";
  if (lowered.includes("linux")) return "linux";
  return "unknown";
}

export async function getRuntimePlatform(): Promise<RuntimePlatform> {
  try {
    const raw = await invoke<string>("get_runtime_platform");
    return normalizeRuntimePlatform(raw);
  } catch {
    return detectPlatformFromUserAgent(globalThis.navigator?.userAgent || "");
  }
}

export function buildChromeDialogFilters(platform: RuntimePlatform): ChromeDialogFilter[] {
  if (platform === "windows") {
    return [{ name: "Chrome Executable", extensions: ["exe"] }];
  }
  if (platform === "macos") {
    return [{ name: "Chrome App", extensions: ["app"] }];
  }
  return [];
}

export async function browseChromeExecutable(
  platform: RuntimePlatform,
): Promise<string | null> {
  const filters = buildChromeDialogFilters(platform);
  const selected = await open({
    multiple: false,
    directory: false,
    filters,
  } as any);

  return typeof selected === "string"
    ? normalizeChromeExecutablePath(selected)
    : null;
}

export async function loadSettings(): Promise<AppSettings> {
  const wire = await invoke<AppSettingsWire>("load_settings");
  return fromWire(wire);
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  await invoke("save_settings", {
    settings: toWire(settings),
  });
}
