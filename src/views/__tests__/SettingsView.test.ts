import { describe, expect, test } from "bun:test";
import {
  buildChromeDialogFilters,
  coerceSettings,
  detectPlatformFromUserAgent,
} from "../../stores/settings";

describe("Settings helpers", () => {
  test("returns .exe file filter on Windows", () => {
    const filters = buildChromeDialogFilters("windows");
    expect(filters).toHaveLength(1);
    expect(filters[0]).toEqual({
      name: "Chrome Executable",
      extensions: ["exe"],
    });
  });

  test("returns .app file filter on macOS", () => {
    const filters = buildChromeDialogFilters("macos");
    expect(filters).toHaveLength(1);
    expect(filters[0]).toEqual({
      name: "Chrome App",
      extensions: ["app"],
    });
  });

  test("detects platform by user agent", () => {
    expect(detectPlatformFromUserAgent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe("macos");
    expect(detectPlatformFromUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("windows");
  });

  test("coerceSettings fills missing fields", () => {
    const settings = coerceSettings({ chromeExecutablePath: "/Applications/Google Chrome.app" });
    expect(settings.chromeExecutablePath).toBe("/Applications/Google Chrome.app");
    expect(settings.dashscopeApiKey).toBe("");
  });
});
