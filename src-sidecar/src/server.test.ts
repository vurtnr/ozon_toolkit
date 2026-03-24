import { describe, expect, test } from "bun:test";
import {
  buildOzonChromeArgs,
  extractChromeSingletonLockPid,
  parseChromeDevToolsPort,
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
      ozonPage: {
        isClosed: () => false,
        close: async () => {
          calls.push("ozonPage");
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

    expect(calls).toEqual(["page", "ozonPage", "browser", "server"]);
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
      ozonPage: {
        isClosed: () => true,
        close: async () => {
          calls.push("ozonPage");
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

describe("parseChromeDevToolsPort", () => {
  test("reads the first line as the devtools port", () => {
    expect(
      parseChromeDevToolsPort(
        "57511\n/devtools/browser/18de3a71-d85d-4095-9478-64ba49b0253b\n",
      ),
    ).toBe(57511);
  });

  test("returns null for invalid devtools port files", () => {
    expect(parseChromeDevToolsPort("not-a-port\n/devtools/browser/x\n")).toBeNull();
    expect(parseChromeDevToolsPort("")).toBeNull();
  });
});

describe("extractChromeSingletonLockPid", () => {
  test("extracts the chrome pid from macOS singleton lock targets", () => {
    expect(extractChromeSingletonLockPid("anonymous-21659")).toBe(21659);
  });

  test("returns null for unsupported lock targets", () => {
    expect(extractChromeSingletonLockPid("")).toBeNull();
    expect(extractChromeSingletonLockPid("/tmp/not-supported")).toBeNull();
  });
});

describe("buildOzonChromeArgs", () => {
  test("uses a clean manual-like chrome startup for ozon without automation flags", () => {
    const args = buildOzonChromeArgs("/tmp/ozon_profile", 9222);

    expect(args).toContain("--user-data-dir=/tmp/ozon_profile");
    expect(args).toContain("--remote-debugging-port=9222");
    expect(args).toContain("about:blank");
    expect(args).toContain("--disable-dev-shm-usage");
    expect(args).toContain("--disable-gpu");
    expect(args).toContain("--disable-session-crashed-bubble");
    expect(args).toContain("--noerrdialogs");
    expect(args).not.toContain("--new-window");
    expect(args).not.toContain("--disable-blink-features=AutomationControlled");
    expect(args.some((value) => value.startsWith("--user-agent="))).toBe(false);
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

  test("does not treat generic my-1688 header copy as authenticated state", () => {
    const snapshot: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "我的1688 我的阿里 采购车 消息",
      links: ["https://member.1688.com/member/default.htm"],
      hasAntiBotChallenge: false,
      hasLoginEntry: true,
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

  test("keeps login_required when a page still exposes login entry alongside stale workbench markers", () => {
    const snapshot: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "请登录 买家工作台",
      links: [
        "https://work.1688.com/home/page/index.htm",
        "https://login.1688.com/member/signin.htm",
      ],
      hasAntiBotChallenge: false,
      hasLoginEntry: true,
      hasLoggedInEntry: true,
    };

    expect(classifySessionState(snapshot)).toBe("login_required");
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

  test("does not treat generic member-center links as authenticated state without strong workbench markers", () => {
    const snapshot: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "欢迎来到1688",
      links: [
        "https://member.1688.com/member/default.htm",
        "https://member.1688.com/member/buyer_orders.htm",
      ],
      hasAntiBotChallenge: false,
      hasLoginEntry: true,
      hasLoggedInEntry: false,
    };

    expect(classifySessionState(snapshot)).toBe("login_required");
  });

  test("does not treat generic workbench links as authenticated state without visible logged-in markers", () => {
    const snapshot: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "欢迎来到1688",
      links: ["https://work.1688.com/home/page/index.htm"],
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

  test("does not let restored generic result pages bypass login gating", () => {
    const home: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "欢迎来到1688",
      links: [],
      hasAntiBotChallenge: false,
      hasLoginEntry: false,
      hasLoggedInEntry: false,
    };
    const restoredResultPage: SessionSnapshot = {
      url: "https://s.1688.com/selloffer/offer_search.htm?keywords=test",
      visibleText: "商品列表 排序 筛选",
      links: [],
      hasAntiBotChallenge: false,
      hasLoginEntry: false,
      hasLoggedInEntry: false,
    };

    expect(classifySessionStates([home, restoredResultPage])).toBe("login_required");
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

  test("does not let a stale authenticated tab override a primary home page that is still logged out", () => {
    const home: SessionSnapshot = {
      url: "https://www.1688.com/",
      visibleText: "欢迎来到1688 请登录",
      links: ["https://login.1688.com/member/signin.htm"],
      hasAntiBotChallenge: false,
      hasLoginEntry: true,
      hasLoggedInEntry: false,
    };
    const staleWorkbenchTab: SessionSnapshot = {
      url: "https://work.1688.com/home/page/index.htm",
      visibleText: "买家工作台",
      links: [],
      hasAntiBotChallenge: false,
      hasLoginEntry: false,
      hasLoggedInEntry: true,
    };

    expect(classifySessionStates([home, staleWorkbenchTab])).toBe("login_required");
  });
});
