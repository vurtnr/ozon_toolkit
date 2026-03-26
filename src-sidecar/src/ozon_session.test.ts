import { describe, expect, test } from "bun:test";
import * as ozonSession from "./ozon_session";
import {
  buildCanonicalOzonProductUrl,
  classifyOzonSkuSearchSnapshot,
  classifyOzonLandingSnapshot,
  classifyOzonSnapshot,
  shouldHopFromIncompleteSnapshot,
  shouldHopFromResolvedSnapshot,
  isTransientPageNavigationError,
  isReusableBootstrapPageUrl,
  isOzonHomeUrl,
  scoreOzonImageCaptureCandidate,
  selectFirstRecommendedProductHref,
  selectPreferredOzonSessionPage,
  selectReusableOzonBootstrapPage,
  type OzonImageCaptureCandidateMetrics,
  type OzonRecommendedProductCandidate,
  type OzonSnapshot,
} from "./ozon_session";

describe("buildCanonicalOzonProductUrl", () => {
  test("normalizes product deep links into a canonical detail url", () => {
    expect(
      buildCanonicalOzonProductUrl(
        "https://www.ozon.ru/product/3552213000/?at=abc&utm_source=test#section",
      ),
    ).toBe("https://www.ozon.ru/product/3552213000/");
  });

  test("extracts the numeric product id from slugged product urls", () => {
    expect(
      buildCanonicalOzonProductUrl(
        "https://www.ozon.ru/product/morskaya-verevochnaya-lestnitsa-3552213000/?from_sku=3552213000",
      ),
    ).toBe("https://www.ozon.ru/product/3552213000/");
  });

  test("rejects non-product ozon urls", () => {
    expect(
      buildCanonicalOzonProductUrl("https://www.ozon.ru/category/sport-12345/"),
    ).toBeNull();
  });
});

describe("selectReusableOzonBootstrapPage", () => {
  test("reuses the browser bootstrap blank page before opening a second tab", () => {
    const blankPage = {
      url: () => "about:blank",
      isClosed: () => false,
    };
    const ozonPage = {
      url: () => "https://www.ozon.ru/",
      isClosed: () => false,
    };

    expect(selectReusableOzonBootstrapPage([ozonPage, blankPage])).toBe(blankPage);
  });

  test("ignores closed or already-navigated tabs when picking a reusable bootstrap page", () => {
    const closedBlank = {
      url: () => "about:blank",
      isClosed: () => true,
    };
    const activeOzon = {
      url: () => "https://www.ozon.ru/product/3552213000/",
      isClosed: () => false,
    };

    expect(selectReusableOzonBootstrapPage([closedBlank, activeOzon])).toBeNull();
  });
});

describe("selectPreferredOzonSessionPage", () => {
  test("prefers an already-open ozon landing page before creating a second tab", () => {
    const ozonLandingPage = {
      url: () => "https://www.ozon.ru/",
      isClosed: () => false,
    };
    const blankPage = {
      url: () => "about:blank",
      isClosed: () => false,
    };

    expect(selectPreferredOzonSessionPage([blankPage, ozonLandingPage])).toBe(
      ozonLandingPage,
    );
  });

  test("falls back to the reusable bootstrap blank tab when no ozon page exists yet", () => {
    const blankPage = {
      url: () => "about:blank",
      isClosed: () => false,
    };

    expect(selectPreferredOzonSessionPage([blankPage])).toBe(blankPage);
  });
});

describe("isReusableBootstrapPageUrl", () => {
  test("treats blank and browser new-tab pages as reusable bootstrap tabs", () => {
    expect(isReusableBootstrapPageUrl("about:blank")).toBe(true);
    expect(isReusableBootstrapPageUrl("chrome://newtab/")).toBe(true);
    expect(isReusableBootstrapPageUrl("edge://newtab/")).toBe(true);
  });

  test("does not treat real Ozon pages as reusable bootstrap tabs", () => {
    expect(isReusableBootstrapPageUrl("https://www.ozon.ru/")).toBe(false);
    expect(isReusableBootstrapPageUrl("https://www.ozon.ru/product/3552213000/")).toBe(false);
  });
});

describe("classifyOzonLandingSnapshot", () => {
  test("treats access-restricted ozon home pages as anti_bot_challenge", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/",
      documentTitle: "Доступ ограничен",
      title: null,
      imageUrl: null,
      bodyText: "Инцидент: fab_chlg_20260323001058 Служба поддержки",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(classifyOzonLandingSnapshot(snapshot)).toBe("anti_bot_challenge");
  });

  test("treats normal ozon home pages as ready", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/",
      documentTitle: "Ozon",
      title: null,
      imageUrl: null,
      bodyText: "Маркетплейс Ozon",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(classifyOzonLandingSnapshot(snapshot)).toBe("ready");
  });

  test("treats redirected ozon landing pages on the same host as ready", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/highlight/global?miniapp=something",
      documentTitle: "Ozon",
      title: null,
      imageUrl: null,
      bodyText: "Маркетплейс Ozon",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(classifyOzonLandingSnapshot(snapshot)).toBe("ready");
  });
});

describe("classifyOzonSnapshot", () => {
  test("treats access-restricted pages as anti_bot_challenge", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3552213000",
      documentTitle: "Доступ ограничен",
      title: "Доступ ограничен",
      imageUrl: "https://cdn.ozon.ru/assets/warning.png",
      bodyText:
        "Инцидент: fab_chlg_20260323001058 Чтобы решить проблему, попробуйте сделать это: Обновить Служба поддержки",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(classifyOzonSnapshot(snapshot)).toBe("anti_bot_challenge");
  });

  test("treats unavailable product pages as unavailable", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3552213000",
      documentTitle: "Товар закончился",
      title: "Товар закончился",
      imageUrl: null,
      bodyText: "Такого товара нет",
      hasAntiBotChallenge: false,
      isUnavailable: true,
    };

    expect(classifyOzonSnapshot(snapshot)).toBe("unavailable");
  });

  test("treats resolved product pages as resolved", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3552213000",
      documentTitle: "Морская верёвочная лестница",
      title: "Морская верёвочная лестница",
      imageUrl: "https://cdn.ozon.ru/images/main.jpeg",
      bodyText: "Описание товара",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(classifyOzonSnapshot(snapshot)).toBe("resolved");
  });

  test("keeps incomplete pages as incomplete until title and image are both ready", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3552213000",
      documentTitle: "Морская верёвочная лестница",
      title: "Морская верёвочная лестница",
      imageUrl: null,
      bodyText: "Описание товара",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(classifyOzonSnapshot(snapshot)).toBe("incomplete");
  });

  test("treats detail not-found pages as unavailable even if recommendation cards expose images", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3570411009/",
      documentTitle: "Такой страницы не существует",
      title: "Комплект гастроемкостей",
      imageUrl: "https://ir.ozone.ru/s3/multimedia-1-7/wc800/8908721791.jpg",
      bodyText: "Такой страницы не существует Вернуться на главную Комплект гастроемкостей",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(classifyOzonSnapshot(snapshot)).toBe("unavailable");
  });
});

describe("shouldAttemptRecommendedProductHop", () => {
  test("disables recommended-product hop on explicit not-found detail pages", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3570411009/",
      documentTitle: "Такой страницы не существует",
      title: "Комплект гастроемкостей",
      imageUrl: "https://ir.ozone.ru/s3/multimedia-1-7/wc800/8908721791.jpg",
      bodyText: "Такой страницы не существует Вернуться на главную Комплект гастроемкостей",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    const hopGuard = (
      ozonSession as {
        shouldAttemptRecommendedProductHop?: (snapshot: OzonSnapshot) => boolean;
      }
    ).shouldAttemptRecommendedProductHop;

    expect(typeof hopGuard).toBe("function");
    expect(hopGuard?.(snapshot)).toBe(false);
  });

  test("keeps recommended-product hop for generic unavailable product pages", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3552213000/",
      documentTitle: "Товар закончился",
      title: "Товар закончился",
      imageUrl: null,
      bodyText: "Такого товара нет",
      hasAntiBotChallenge: false,
      isUnavailable: true,
    };

    const hopGuard = (
      ozonSession as {
        shouldAttemptRecommendedProductHop?: (snapshot: OzonSnapshot) => boolean;
      }
    ).shouldAttemptRecommendedProductHop;

    expect(typeof hopGuard).toBe("function");
    expect(hopGuard?.(snapshot)).toBe(true);
  });
});

describe("shouldHopFromResolvedSnapshot", () => {
  test("requires hopping into the first product when a low-confidence generic title resolves on a multi-product page", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3560192694/",
      documentTitle: "Чехол для планшета - купить на OZON",
      title: "Чехол для планшета - купить на OZON",
      imageUrl: "https://ir.ozone.ru/s3/multimedia-1-z/wc800/9119999447.jpg",
      bodyText: "Чехол для планшета купить на ozon похожие товары",
      hasAntiBotChallenge: false,
      isUnavailable: false,
      titleSource: "document_title",
    };

    expect(shouldHopFromResolvedSnapshot(snapshot)).toBe(true);
  });

  test("requires hopping when the page only exposes generic Ozon og metadata and the site logo image", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3560192694/",
      documentTitle: "Чехол для планшета - купить на OZON",
      title: "Чехол для планшета - купить на OZON",
      titleSource: "meta_og",
      imageUrl: "https://ir.ozone.ru/s3/cms/logo/og_ozon_ru.png",
      imageSource: "meta_og",
      bodyText: "Похожие предложения Рекомендуем также",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(shouldHopFromResolvedSnapshot(snapshot)).toBe(true);
  });

  test("keeps real detail pages on the current product when the title comes from structured product data", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3552213000/",
      documentTitle: "Морская верёвочная лестница",
      title: "Морская верёвочная лестница",
      imageUrl: "https://cdn.ozon.ru/images/main.jpeg",
      bodyText: "Описание товара",
      hasAntiBotChallenge: false,
      isUnavailable: false,
      titleSource: "json_ld",
    };

    expect(shouldHopFromResolvedSnapshot(snapshot)).toBe(false);
  });
});

describe("shouldHopFromIncompleteSnapshot", () => {
  test("requires hopping when an intermediate page exposes only generic marketplace text before the real detail page", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3569938663/",
      documentTitle: "Чехол для планшета - купить на OZON",
      title: "Чехол для планшета - купить на OZON",
      titleSource: "document_title",
      imageUrl: null,
      bodyText: "Похожие предложения Рекомендуем также",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(shouldHopFromIncompleteSnapshot(snapshot)).toBe(true);
  });

  test("does not hop from explicit not-found pages even if recommendation text is present", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3570411009/",
      documentTitle: "Такой страницы не существует",
      title: null,
      imageUrl: null,
      bodyText: "Такой страницы не существует Похожие предложения",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(shouldHopFromIncompleteSnapshot(snapshot)).toBe(false);
  });
});

describe("classifyOzonSkuSearchSnapshot", () => {
  test("classifies an Ozon not-found error page as not_found for SKU search", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/search/?text=SKU-404",
      documentTitle: "Такой страницы не существует",
      title: null,
      imageUrl: null,
      bodyText: "Такой страницы не существует Вернуться на главную",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(classifyOzonSkuSearchSnapshot(snapshot)).toBe("not_found");
  });

  test("treats a resolved product detail page as resolved during SKU search", () => {
    const snapshot: OzonSnapshot = {
      url: "https://www.ozon.ru/product/3552213000/",
      documentTitle: "SKU Product",
      title: "SKU Product",
      imageUrl: "https://cdn.ozon.ru/main.jpeg",
      bodyText: "Описание товара",
      hasAntiBotChallenge: false,
      isUnavailable: false,
    };

    expect(classifyOzonSkuSearchSnapshot(snapshot)).toBe("resolved");
  });
});

describe("isTransientPageNavigationError", () => {
  test("treats execution-context-destroyed errors as transient navigation noise", () => {
    expect(
      isTransientPageNavigationError(
        new Error("Execution context was destroyed, most likely because of a navigation."),
      ),
    ).toBe(true);
  });

  test("does not hide unrelated runtime errors", () => {
    expect(isTransientPageNavigationError(new Error("delay is not defined"))).toBe(false);
  });

  test("treats session-closed and target-closed errors as transient", () => {
    expect(
      isTransientPageNavigationError(new Error("Session closed. Most likely the page has been closed.")),
    ).toBe(true);
    expect(
      isTransientPageNavigationError(new Error("Target closed.")),
    ).toBe(true);
    expect(
      isTransientPageNavigationError(new Error("Protocol error (Runtime.callFunctionOn): Session closed.")),
    ).toBe(true);
  });
});

describe("isOzonHomeUrl", () => {
  test("treats the ozon.ru root as a home URL", () => {
    expect(isOzonHomeUrl("https://www.ozon.ru/")).toBe(true);
    expect(isOzonHomeUrl("https://ozon.ru/")).toBe(true);
    expect(isOzonHomeUrl("https://www.ozon.ru")).toBe(true);
  });

  test("treats ozon highlight/landing redirects as home URLs", () => {
    expect(isOzonHomeUrl("https://www.ozon.ru/highlight/global?miniapp=x")).toBe(true);
  });

  test("does not treat product or search pages as home URLs", () => {
    expect(isOzonHomeUrl("https://www.ozon.ru/product/3552213000/")).toBe(false);
    expect(isOzonHomeUrl("https://www.ozon.ru/search/?text=test")).toBe(false);
  });

  test("does not treat non-ozon URLs as home URLs", () => {
    expect(isOzonHomeUrl("https://www.google.com/")).toBe(false);
    expect(isOzonHomeUrl("about:blank")).toBe(false);
  });
});

describe("scoreOzonImageCaptureCandidate", () => {
  const baseCandidate: OzonImageCaptureCandidateMetrics = {
    currentSrc: "https://ir.ozone.ru/s3/multimedia-1-z/wc800/9119999447.jpg",
    naturalWidth: 900,
    naturalHeight: 900,
    rectWidth: 360,
    rectHeight: 360,
    rectTop: 120,
    rectBottom: 480,
    viewportHeight: 900,
  };

  test("rejects visible qr-like images when they do not match the expected product image url", () => {
    expect(
      scoreOzonImageCaptureCandidate(
        {
          ...baseCandidate,
          currentSrc: "https://cdn.example.com/share/qr-code.png",
        },
        "https://ir.ozone.ru/s3/multimedia-1-z/wc800/9119999447.jpg",
      ),
    ).toBe(Number.NEGATIVE_INFINITY);
  });

  test("accepts the product image when the filename matches even if the size segment differs", () => {
    expect(
      scoreOzonImageCaptureCandidate(
        {
          ...baseCandidate,
          currentSrc: "https://ir.ozone.ru/s3/multimedia-1-z/wc100/9119999447.jpg",
        },
        "https://ir.ozone.ru/s3/multimedia-1-z/wc800/9119999447.jpg",
      ),
    ).toBeGreaterThan(0);
  });
});

describe("selectFirstRecommendedProductHref", () => {
  test("chooses the first product from the earliest valid multi-product container", () => {
    const currentUrl = "https://www.ozon.ru/product/1111111111/";
    const candidates: OzonRecommendedProductCandidate[] = [
      {
        href: "https://www.ozon.ru/product/2222222222/",
        top: 180,
        left: 40,
        containerKey: "sidebar",
        containerTop: 160,
        containerLeft: 20,
        containerArea: 30_000,
        containerProductCount: 1,
      },
      {
        href: "https://www.ozon.ru/product/3333333333/",
        top: 220,
        left: 120,
        containerKey: "main-grid",
        containerTop: 200,
        containerLeft: 100,
        containerArea: 280_000,
        containerProductCount: 4,
      },
      {
        href: "https://www.ozon.ru/product/4444444444/",
        top: 220,
        left: 300,
        containerKey: "main-grid",
        containerTop: 200,
        containerLeft: 100,
        containerArea: 280_000,
        containerProductCount: 4,
      },
    ];

    expect(selectFirstRecommendedProductHref(candidates, currentUrl)).toBe(
      "https://www.ozon.ru/product/3333333333/",
    );
  });

  test("excludes the current product url and still picks the first remaining product", () => {
    const currentUrl = "https://www.ozon.ru/product/3333333333/";
    const candidates: OzonRecommendedProductCandidate[] = [
      {
        href: "https://www.ozon.ru/product/3333333333/?from_sku=3333333333",
        top: 220,
        left: 120,
        containerKey: "main-grid",
        containerTop: 200,
        containerLeft: 100,
        containerArea: 280_000,
        containerProductCount: 3,
      },
      {
        href: "https://www.ozon.ru/product/4444444444/",
        top: 220,
        left: 300,
        containerKey: "main-grid",
        containerTop: 200,
        containerLeft: 100,
        containerArea: 280_000,
        containerProductCount: 3,
      },
    ];

    expect(selectFirstRecommendedProductHref(candidates, currentUrl)).toBe(
      "https://www.ozon.ru/product/4444444444/",
    );
  });

  test("returns null when only single-item containers exist", () => {
    const currentUrl = "https://www.ozon.ru/product/1111111111/";
    const candidates: OzonRecommendedProductCandidate[] = [
      {
        href: "https://www.ozon.ru/product/2222222222/",
        top: 180,
        left: 40,
        containerKey: "sidebar",
        containerTop: 160,
        containerLeft: 20,
        containerArea: 30_000,
        containerProductCount: 1,
      },
      {
        href: "https://www.ozon.ru/product/3333333333/",
        top: 420,
        left: 60,
        containerKey: "footer",
        containerTop: 400,
        containerLeft: 20,
        containerArea: 40_000,
        containerProductCount: 1,
      },
    ];

    expect(selectFirstRecommendedProductHref(candidates, currentUrl)).toBeNull();
  });
});
