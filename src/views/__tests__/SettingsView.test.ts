import { describe, expect, test } from "bun:test";
import {
  buildChromeDialogFilters,
  coerceSettings,
  detectPlatformFromUserAgent,
  isValidProfitRatioInput,
  normalizeChromeExecutablePath,
  sanitizeProfitRatioInput,
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
    const settings = coerceSettings({
      chromeExecutablePath: "/Applications/Google Chrome.app",
    });
    expect(settings.chromeExecutablePath).toBe(
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    );
    expect(settings.dashscopeApiKey).toBe("");
    expect(settings.profitRatio).toBe("");
  });

  test("normalizes macOS app bundle paths into executable paths", () => {
    expect(normalizeChromeExecutablePath("/Applications/Google Chrome.app")).toBe(
      "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    );
  });

  test("sanitizes profit ratio input to digits and two decimals", () => {
    expect(sanitizeProfitRatioInput("12.3456")).toBe("12.34");
    expect(sanitizeProfitRatioInput("abc18.2x5")).toBe("18.25");
    expect(sanitizeProfitRatioInput("001.20")).toBe("001.20");
  });

  test("validates profit ratio input for task execution", () => {
    expect(isValidProfitRatioInput("12.34")).toBe(true);
    expect(isValidProfitRatioInput("0")).toBe(false);
    expect(isValidProfitRatioInput("100")).toBe(false);
    expect(isValidProfitRatioInput("")).toBe(false);
  });
});
