import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export type RuntimePlatform = "windows" | "macos" | "linux" | "unknown";

export interface AppSettings {
  dashscopeApiKey: string;
  chromeExecutablePath: string;
}

interface AppSettingsWire {
  dashscope_api_key: string;
  chrome_executable_path: string;
}

export interface ChromeDialogFilter {
  name: string;
  extensions: string[];
}

export function createDefaultSettings(): AppSettings {
  return {
    dashscopeApiKey: "",
    chromeExecutablePath: "",
  };
}

export function coerceSettings(input: Partial<AppSettings> | undefined): AppSettings {
  return {
    dashscopeApiKey: input?.dashscopeApiKey ?? "",
    chromeExecutablePath: input?.chromeExecutablePath ?? "",
  };
}

function fromWire(wire: AppSettingsWire | null | undefined): AppSettings {
  if (!wire) return createDefaultSettings();
  return coerceSettings({
    dashscopeApiKey: wire.dashscope_api_key,
    chromeExecutablePath: wire.chrome_executable_path,
  });
}

function toWire(settings: AppSettings): AppSettingsWire {
  return {
    dashscope_api_key: settings.dashscopeApiKey,
    chrome_executable_path: settings.chromeExecutablePath,
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

  return typeof selected === "string" ? selected : null;
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
