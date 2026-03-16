import { describe, expect, test } from "bun:test";
import { findChromePath, ChromeNotFoundError } from "./chrome-path";

describe("findChromePath", () => {
  test("uses CHROME_EXECUTABLE_PATH first when provided", async () => {
    const result = await findChromePath({
      env: { CHROME_EXECUTABLE_PATH: "/custom/chrome" },
      exists: (candidate) => candidate === "/custom/chrome",
      platform: "darwin",
    });

    expect(result).toBe("/custom/chrome");
  });

  test("falls back to platform candidates when env is empty", async () => {
    const result = await findChromePath({
      env: { CHROME_EXECUTABLE_PATH: "" },
      exists: (candidate) =>
        candidate ===
        "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
      platform: "win32",
    });

    expect(result).toBe(
      "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    );
  });

  test("throws ChromeNotFoundError when no path can be found", async () => {
    await expect(
      findChromePath({
        env: {},
        exists: () => false,
        platform: "darwin",
      }),
    ).rejects.toBeInstanceOf(ChromeNotFoundError);

    await expect(
      findChromePath({
        env: {},
        exists: () => false,
        platform: "darwin",
      }),
    ).rejects.toMatchObject({ code: "CHROME_NOT_FOUND" });
  });
});
