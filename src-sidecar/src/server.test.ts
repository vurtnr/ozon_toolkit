import { describe, expect, test } from "bun:test";
import {
  classifySessionStates,
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

  test("does not treat generic header words like cart or messages as authenticated state", () => {
    const snapshot: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "采购车 进货单 消息",
      links: [],
      hasAntiBotChallenge: false,
      hasLoginEntry: false,
      hasLoggedInEntry: false,
    };

    expect(classifySessionState(snapshot)).toBe("login_required");
  });
});

describe("classifySessionStates", () => {
  test("keeps login_required while a login page is still open and no authenticated page is present", () => {
    const home: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "欢迎来到1688",
      links: [],
      hasAntiBotChallenge: false,
      hasLoginEntry: false,
      hasLoggedInEntry: false,
    };
    const loginPopup: SessionSnapshot = {
      url: "https://login.1688.com/member/signin.htm",
      visibleText: "请登录 免费注册",
      links: [],
      hasAntiBotChallenge: false,
      hasLoginEntry: true,
      hasLoggedInEntry: false,
    };

    expect(classifySessionStates([home, loginPopup])).toBe("login_required");
  });

  test("returns ready once any open page exposes a strong authenticated marker", () => {
    const home: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "欢迎来到1688",
      links: [],
      hasAntiBotChallenge: false,
      hasLoginEntry: false,
      hasLoggedInEntry: false,
    };
    const workbench: SessionSnapshot = {
      url: "https://work.1688.com/home/page/index.htm",
      visibleText: "买家工作台",
      links: [],
      hasAntiBotChallenge: false,
      hasLoginEntry: false,
      hasLoggedInEntry: true,
    };

    expect(classifySessionStates([home, workbench])).toBe("ready");
  });
});
