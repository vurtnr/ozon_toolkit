import { describe, expect, test } from "bun:test";
import {
  classifySessionState,
  shutdownRuntimeResources,
  type SessionSnapshot,
} from "./server";

describe("shutdownRuntimeResources", () => {
  test("closes page, browser and http server in order", async () => {
    const calls: string[] = [];

    await shutdownRuntimeResources({
      page: {
        isClosed: () => false,
        close: async () => {
          calls.push("page");
        },
      },
      browser: {
        isConnected: () => true,
        close: async () => {
          calls.push("browser");
        },
      },
      server: {
        close: (callback) => {
          calls.push("server");
          callback();
          return {} as never;
        },
      },
    });

    expect(calls).toEqual(["page", "browser", "server"]);
  });

  test("skips already-closed resources", async () => {
    const calls: string[] = [];

    await shutdownRuntimeResources({
      page: {
        isClosed: () => true,
        close: async () => {
          calls.push("page");
        },
      },
      browser: {
        isConnected: () => false,
        close: async () => {
          calls.push("browser");
        },
      },
      server: null,
    });

    expect(calls).toEqual([]);
  });
});

describe("classifySessionState", () => {
  test("treats ambiguous 1688 home snapshots as login_required until positive login markers appear", () => {
    const snapshot: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "欢迎来到1688",
      links: [],
      hasAntiBotChallenge: false,
      hasLoginEntry: false,
      hasLoggedInEntry: false,
    };

    expect(classifySessionState(snapshot)).toBe("login_required");
  });

  test("treats logged-in workbench markers as ready even when generic login copy is absent", () => {
    const snapshot: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "欢迎回来",
      links: ["https://work.1688.com/home/page/index.htm"],
      hasAntiBotChallenge: false,
      hasLoginEntry: false,
      hasLoggedInEntry: true,
    };

    expect(classifySessionState(snapshot)).toBe("ready");
  });
});
