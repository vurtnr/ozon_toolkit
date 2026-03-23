import { describe, expect, test } from "bun:test";
import {
  buildCanonicalOzonProductUrl,
  classifyOzonSkuSearchSnapshot,
  classifyOzonLandingSnapshot,
  classifyOzonSnapshot,
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
