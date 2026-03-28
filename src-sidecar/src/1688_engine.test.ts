import { describe, expect, test } from "bun:test";
import {
  executeResultPageRecall,
  extract1688DetailFreight,
  extractSalesText,
  isLikelySearchResultsUrl,
  shouldKeepWaitingForSearchConfirm,
  shouldEnsureHomePageBeforeSessionCheck,
  selectClosableTabs,
  shouldStopResultScroll,
  shouldNavigateTo1688Home,
  waitForSearchResults,
  openCropDialogAndWaitForCanvas,
  type SearchResult,
} from "./1688_engine";

type CropDialogPageLike = {
  waitForFunction: (fn: unknown, options: { timeout: number }) => Promise<void>;
  evaluate: (fn: unknown) => Promise<void>;
  waitForSelector: (
    selector: string,
    options: { visible: boolean; timeout: number },
  ) => Promise<void>;
};

type ResultReadyPageLike = {
  waitForSelector: (
    selector: string,
    options: { timeout: number },
  ) => Promise<unknown>;
  waitForNetworkIdle: (options: { timeout: number }) => Promise<void>;
};

const sampleResults: SearchResult[] = [
  {
    title: "sample",
    price: "¥12.34",
    sales: "",
    moq: "",
    shopName: "shop",
    itemUrl: "https://detail.1688.com/offer/1.html",
    imageUrl: "https://img.1688.com/1.jpg",
    isAd: false,
    cosScore: 0.88,
  },
];

describe("extractSalesText", () => {
  test("extracts common sales formats from 1688 card text", () => {
    expect(extractSalesText("月销 123 笔 成交")).toBe("123");
    expect(extractSalesText("已售 2.5万+ 件")).toBe("2.5万+");
    expect(extractSalesText("成交 980 笔")).toBe("980");
    expect(extractSalesText("3000人付款")).toBe("3000");
  });

  test("returns empty string when no sales signal exists", () => {
    expect(extractSalesText("暂无销量 店铺上新")).toBe("");
    expect(extractSalesText("")).toBe("");
  });
});

describe("extract1688DetailFreight", () => {
  test("treats 包邮 as zero freight", () => {
    expect(
      extract1688DetailFreight([
        "48小时发货 包邮 7天包换",
        "service-item split-border 包邮",
      ]),
    ).toEqual({
      freightText: "¥0",
      freightValue: 0,
      isFreeShipping: true,
    });
  });

  test("extracts freight amount from detail service signals", () => {
    expect(
      extract1688DetailFreight([
        "48小时发货 运费 ¥6 起 7天包换",
        "service-item split-border ¥6起",
      ]),
    ).toEqual({
      freightText: "¥6",
      freightValue: 6,
      isFreeShipping: false,
    });
  });

  test("ignores unrelated signals when freight is absent", () => {
    expect(
      extract1688DetailFreight([
        "48小时发货 7天包换 破损包赔",
        "service-item split-border 极速发货",
      ]),
    ).toBeNull();
  });
});

describe("executeResultPageRecall", () => {
  test("keeps default search path when forceFullCrop is false", async () => {
    let cropCalls = 0;
    let scrapeCalls = 0;

    const results = await executeResultPageRecall({
      forceFullCrop: false,
      scrapeCurrentPage: async () => {
        scrapeCalls += 1;
        return sampleResults;
      },
      applyFullCanvasCrop: async () => {
        cropCalls += 1;
      },
    });

    expect(results).toEqual(sampleResults);
    expect(scrapeCalls).toBe(1);
    expect(cropCalls).toBe(0);
  });

  test("enters crop expansion path when forceFullCrop is true", async () => {
    const calls: string[] = [];

    const results = await executeResultPageRecall({
      forceFullCrop: true,
      scrapeCurrentPage: async () => {
        calls.push("scrape");
        return sampleResults;
      },
      applyFullCanvasCrop: async () => {
        calls.push("crop");
      },
    });

    expect(results).toEqual(sampleResults);
    expect(calls).toEqual(["crop", "scrape"]);
  });

  test("preserves FULL_CROP_NOT_APPLIED errors from crop expansion path", async () => {
    await expect(
      executeResultPageRecall({
        forceFullCrop: true,
        scrapeCurrentPage: async () => sampleResults,
        applyFullCanvasCrop: async () => {
          throw new Error("[FULL_CROP_NOT_APPLIED] crop failed");
        },
      }),
    ).rejects.toThrow("[FULL_CROP_NOT_APPLIED] crop failed");
  });
});

describe("openCropDialogAndWaitForCanvas", () => {
  test("requires croper canvas before continuing full crop flow", async () => {
    const selectors: string[] = [];
    const page: CropDialogPageLike = {
      waitForFunction: async () => {},
      evaluate: async () => {},
      waitForSelector: async (selector) => {
        selectors.push(selector);
      },
    };

    await openCropDialogAndWaitForCanvas(page as never);

    expect(selectors).toEqual(["#croper-canvas"]);
  });
});

describe("shouldNavigateTo1688Home", () => {
  test("reuses existing home page when already on 1688 root", () => {
    expect(shouldNavigateTo1688Home("https://www.1688.com/")).toBe(false);
    expect(shouldNavigateTo1688Home("https://www.1688.com/?spm=a260k")).toBe(false);
  });

  test("requires navigation when current page is not the home entry", () => {
    expect(shouldNavigateTo1688Home("https://s.1688.com/selloffer/offer_search.htm")).toBe(true);
    expect(shouldNavigateTo1688Home("https://login.1688.com/member/signin.htm")).toBe(true);
    expect(shouldNavigateTo1688Home("about:blank")).toBe(true);
  });
});

describe("shouldEnsureHomePageBeforeSessionCheck", () => {
  test("requires resetting generic result pages back to home before session checks", () => {
    expect(
      shouldEnsureHomePageBeforeSessionCheck(
        "https://s.1688.com/selloffer/offer_search.htm?keywords=test",
      ),
    ).toBe(true);
    expect(
      shouldEnsureHomePageBeforeSessionCheck(
        "https://detail.1688.com/offer/123.html",
      ),
    ).toBe(true);
  });

  test("keeps current page when already at home or login flow", () => {
    expect(shouldEnsureHomePageBeforeSessionCheck("https://www.1688.com/")).toBe(false);
    expect(
      shouldEnsureHomePageBeforeSessionCheck(
        "https://login.1688.com/member/signin.htm",
      ),
    ).toBe(false);
  });
});

describe("waitForSearchResults", () => {
  test("returns immediately when product cards appear without waiting for network idle", async () => {
    const calls: string[] = [];
    const page: ResultReadyPageLike = {
      waitForSelector: async () => {
        calls.push("selector");
        return {};
      },
      waitForNetworkIdle: async () => {
        calls.push("idle");
      },
    };

    const ready = await waitForSearchResults(page as never);

    expect(ready).toBe(true);
    expect(calls).toEqual(["selector"]);
  });

  test("falls back to a shorter network idle path when selector is late", async () => {
    const calls: string[] = [];
    let selectorCalls = 0;
    const page: ResultReadyPageLike = {
      waitForSelector: async () => {
        selectorCalls += 1;
        calls.push(`selector-${selectorCalls}`);
        if (selectorCalls === 1) {
          throw new Error("late cards");
        }
        return {};
      },
      waitForNetworkIdle: async () => {
        calls.push("idle");
      },
    };

    const ready = await waitForSearchResults(page as never);

    expect(ready).toBe(true);
    expect(calls).toEqual(["selector-1", "idle", "selector-2"]);
  });

  test("returns false when neither current page nor follow-up check yields result cards", async () => {
    const calls: string[] = [];
    const page: ResultReadyPageLike = {
      waitForSelector: async () => {
        calls.push("selector");
        throw new Error("no cards");
      },
      waitForNetworkIdle: async () => {
        calls.push("idle");
      },
    };

    const ready = await waitForSearchResults(page as never);

    expect(ready).toBe(false);
    expect(calls).toEqual(["selector", "idle", "selector"]);
  });
});

describe("isLikelySearchResultsUrl", () => {
  test("recognizes common 1688 image-search result urls across platform variants", () => {
    expect(
      isLikelySearchResultsUrl(
        "https://s.1688.com/youyuan/index.htm?tab=imageSearch&imageType=offer",
      ),
    ).toBe(true);
    expect(
      isLikelySearchResultsUrl(
        "https://s.1688.com/selloffer/offer_search.htm?keywords=test",
      ),
    ).toBe(true);
  });

  test("does not treat the 1688 home page as a result page", () => {
    expect(isLikelySearchResultsUrl("https://www.1688.com/")).toBe(false);
    expect(isLikelySearchResultsUrl("about:blank")).toBe(false);
  });
});

describe("shouldKeepWaitingForSearchConfirm", () => {
  test("stops confirm polling once the image-search result page is already open", () => {
    expect(
      shouldKeepWaitingForSearchConfirm(
        "https://s.1688.com/youyuan/index.htm?tab=imageSearch&imageType=offer",
        false,
      ),
    ).toBe(false);
  });

  test("stops confirm polling once result cards are already visible on the current page", () => {
    expect(
      shouldKeepWaitingForSearchConfirm("https://www.1688.com/", true),
    ).toBe(false);
    expect(
      shouldKeepWaitingForSearchConfirm("https://www.1688.com/", false),
    ).toBe(true);
  });
});

describe("selectClosableTabs", () => {
  test("closes stale 1688 result tabs and blank tabs while keeping the home page", () => {
    const homePage = {
      url: () => "https://www.1688.com/",
      isClosed: () => false,
    };
    const resultPage = {
      url: () => "https://s.1688.com/youyuan/index.htm?tab=imageSearch",
      isClosed: () => false,
    };
    const blankPage = {
      url: () => "about:blank",
      isClosed: () => false,
    };
    const externalPage = {
      url: () => "https://example.com/",
      isClosed: () => false,
    };

    expect(
      selectClosableTabs([homePage, resultPage, blankPage, externalPage], [homePage]),
    ).toEqual([resultPage, blankPage]);
  });

  test("skips already closed pages and explicitly kept result tabs", () => {
    const homePage = {
      url: () => "https://www.1688.com/",
      isClosed: () => false,
    };
    const keptResultPage = {
      url: () => "https://s.1688.com/selloffer/offer_search.htm",
      isClosed: () => false,
    };
    const closedResultPage = {
      url: () => "https://s.1688.com/youyuan/index.htm?tab=imageSearch",
      isClosed: () => true,
    };

    expect(
      selectClosableTabs([homePage, keptResultPage, closedResultPage], [homePage, keptResultPage]),
    ).toEqual([]);
  });
});

describe("shouldStopResultScroll", () => {
  test("stops immediately once enough visible candidates are present", () => {
    expect(
      shouldStopResultScroll({
        visibleResultCount: 36,
        targetResultCount: 36,
        reachedBottom: false,
        totalScrolled: 600,
        maxScrollDistance: 4000,
      }),
    ).toBe(true);
  });

  test("stops when the page bottom has already been reached", () => {
    expect(
      shouldStopResultScroll({
        visibleResultCount: 18,
        targetResultCount: 36,
        reachedBottom: true,
        totalScrolled: 1200,
        maxScrollDistance: 4000,
      }),
    ).toBe(true);
  });

  test("keeps scrolling when candidate budget is still insufficient and more page remains", () => {
    expect(
      shouldStopResultScroll({
        visibleResultCount: 18,
        targetResultCount: 36,
        reachedBottom: false,
        totalScrolled: 1200,
        maxScrollDistance: 4000,
      }),
    ).toBe(false);
  });
});
