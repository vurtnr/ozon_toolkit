import { describe, expect, test } from "bun:test";
import {
  buildDetailSpecGroupsFromSections,
  didInlineRowQuantityIncrement,
  didQuantityIncrementApply,
  didPriceSourceRefresh,
  didSelectionPlanApplyOrRefreshPriceSource,
  deriveDetailPricingFailureCode,
  extractStructuredFreightPriceFromNetworkEvents,
  extractStructuredVariantRowsFromData,
  extractNumericTokens,
  isPriceInventoryOnlyLabel,
  isDetailSelectionRowActive,
  normalizeComparableSpecToken,
  parseInlineQuantityValue,
  isPlusLikeSymbol,
  isMinusLikeSymbol,
  executeResultPageRecall,
  didSelectionPlanApply,
  resolveDetailPricingSelectionPlan,
  pickVariantRowByImage,
  pickVariantRowByLabel,
  resolveDetailPricingDecision,
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
  type DetailSpecGroupSection,
  type DetailVariantRow,
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

const sampleVariantRows: DetailVariantRow[] = [
  {
    rowIndex: 0,
    label: "40cm 蓝色",
    imageUrls: ["https://cbu01.alicdn.com/img/ibank/O1CN01-target.jpg"],
  },
  {
    rowIndex: 1,
    label: "55cm 白色",
    imageUrls: ["https://cbu01.alicdn.com/img/ibank/O1CN01-other.jpg"],
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

describe("spec normalization", () => {
  test("normalizes russian units and chinese count units into comparable tokens", () => {
    expect(normalizeComparableSpecToken("89см")).toBe("89cm");
    expect(normalizeComparableSpecToken("1шт")).toBe("1件");
    expect(normalizeComparableSpecToken("2 pcs")).toBe("2件");
  });

  test("normalizes russian and english color tokens into chinese canonical colors", () => {
    expect(normalizeComparableSpecToken("Белый")).toBe("白色");
    expect(normalizeComparableSpecToken("black")).toBe("黑色");
  });
});

describe("detail pricing helpers", () => {
  test("extractStructuredVariantRowsFromData discovers nested variant options from page state", () => {
    expect(
      extractStructuredVariantRowsFromData({
        skuModel: {
          saleProp: [
            {
              name: "颜色",
              values: [
                { displayName: "白色 89cm 1件", imageUrl: "https://img.example.com/white.jpg" },
                { displayName: "黑色 60cm 2件", imageUrl: "https://img.example.com/black.jpg" },
              ],
            },
          ],
        },
      }),
    ).toEqual([
      {
        rowIndex: 0,
        label: "白色 89cm 1件",
        imageUrls: ["https://img.example.com/white.jpg"],
      },
      {
        rowIndex: 1,
        label: "黑色 60cm 2件",
        imageUrls: ["https://img.example.com/black.jpg"],
      },
    ]);
  });

  test("extractStructuredVariantRowsFromData ignores generic panel labels and action buttons", () => {
    expect(
      extractStructuredVariantRowsFromData({
        result: {
          data: {
            productInfo: {
              fields: {
                amountOnSale: "100条/桶",
                detailAttributes: [
                  { label: "酸碱度范围", value: "5.5-9" },
                  { label: "总体尺寸", value: "5*5*13cm" },
                  { label: "产品类型", value: "pH试纸" },
                ],
              },
            },
            orderPanel: {
              buttons: [
                { text: "立即下单" },
                { text: "加采购车" },
                { text: "跨境铺货" },
              ],
              moduleTitle: "下单面板",
            },
            skuModel: {
              saleProp: [],
            },
          },
        },
      }),
    ).toEqual([]);
  });

  test("extractStructuredVariantRowsFromData ignores generic attribute values outside sku paths", () => {
    expect(
      extractStructuredVariantRowsFromData({
        result: {
          data: {
            productPackInfo: {
              fields: {
                unitWeight: 0.065,
                attrs: {
                  offerUnit: "件",
                  detailAttributes: [
                    { value: "100条/桶" },
                    { value: "5.5-9" },
                    { value: "5*5*13cm" },
                    { value: "pH试纸" },
                    { value: "24个月" },
                  ],
                },
              },
            },
          },
        },
      }),
    ).toEqual([]);
  });

  test("extractStructuredFreightPriceFromNetworkEvents parses freight totalCost responses", () => {
    expect(
      extractStructuredFreightPriceFromNetworkEvents([
        {
          url: "https://h5api.m.1688.com/h5/mtop.1688.freightInfoService.getFreightInfoWithScene/1.0/",
          status: 200,
          method: "GET",
          resourceType: "xhr",
          contentType: "application/json",
          bodySample:
            '{"api":"mtop.1688.freightinfoservice.getfreightinfowithscene","data":{"totalCost":7}}',
        },
      ]),
    ).toBe("¥7.00");
  });

  test("buildDetailSpecGroupsFromSections preserves multiple explicit groups", () => {
    const sections: DetailSpecGroupSection[] = [
      {
        label: "颜色",
        rows: [
          { rowIndex: 0, label: "白色", imageUrls: [] },
          { rowIndex: 1, label: "黑色", imageUrls: [] },
        ],
      },
      {
        label: "尺寸",
        rows: [
          { rowIndex: 2, label: "60cm", imageUrls: [] },
          { rowIndex: 3, label: "89cm", imageUrls: [] },
        ],
      },
    ];

    expect(buildDetailSpecGroupsFromSections(sections)).toEqual([
      {
        groupIndex: 0,
        groupLabel: "颜色",
        options: [
          { groupIndex: 0, optionIndex: 0, rowIndex: 0, label: "白色", imageUrls: [] },
          { groupIndex: 0, optionIndex: 1, rowIndex: 1, label: "黑色", imageUrls: [] },
        ],
      },
      {
        groupIndex: 1,
        groupLabel: "尺寸",
        options: [
          { groupIndex: 1, optionIndex: 0, rowIndex: 2, label: "60cm", imageUrls: [] },
          { groupIndex: 1, optionIndex: 1, rowIndex: 3, label: "89cm", imageUrls: [] },
        ],
      },
    ]);
  });

  test("buildDetailSpecGroupsFromSections falls back to a single implicit group", () => {
    const sections: DetailSpecGroupSection[] = [
      {
        label: null,
        rows: [
          { rowIndex: 0, label: "40cm 蓝色", imageUrls: [] },
          { rowIndex: 1, label: "55cm 白色", imageUrls: [] },
        ],
      },
    ];

    expect(buildDetailSpecGroupsFromSections(sections)).toEqual([
      {
        groupIndex: 0,
        groupLabel: null,
        options: [
          { groupIndex: 0, optionIndex: 0, rowIndex: 0, label: "40cm 蓝色", imageUrls: [] },
          { groupIndex: 0, optionIndex: 1, rowIndex: 1, label: "55cm 白色", imageUrls: [] },
        ],
      },
    ]);
  });

  test("extractNumericTokens keeps stable multi-digit fragments", () => {
    expect(extractNumericTokens("刷子收纳盒 40cm 2件装")).toEqual(["40"]);
    expect(extractNumericTokens("无规格数字")).toEqual([]);
  });

  test("pickVariantRowByImage matches by normalized image filename", () => {
    expect(
      pickVariantRowByImage(
        sampleVariantRows,
        "https://img.alicdn.com/path/O1CN01-target.jpg?size=640x640",
      )?.rowIndex,
    ).toBe(0);
  });

  test("pickVariantRowByLabel matches by numeric fragments in ozon title", () => {
    expect(
      pickVariantRowByLabel(
        [
          { rowIndex: 0, label: "40cm 蓝色", imageUrls: [] },
          { rowIndex: 1, label: "55cm 白色", imageUrls: [] },
        ],
        "Brush holder 55cm premium",
      )?.rowIndex,
    ).toBe(1);
  });

  test("resolveDetailPricingDecision returns manual review when sku exists but no signal matches", () => {
    expect(
      resolveDetailPricingDecision({
        hasSkuSelection: true,
        rows: [{ rowIndex: 0, label: "大号 蓝色", imageUrls: [] }],
        matchedImageUrl: "https://img.example.com/unrelated.jpg",
        ozonTitle: "Storage box",
      }),
    ).toEqual({
      resolutionMode: "manual_review_required_unknown_spec",
      row: null,
    });
  });

  test("resolveDetailPricingDecision prefers ozon spec profile over weak title signal", () => {
    expect(
      resolveDetailPricingDecision({
        hasSkuSelection: true,
        rows: [
          { rowIndex: 0, label: "白色 89cm", imageUrls: [] },
          { rowIndex: 1, label: "黑色 60cm", imageUrls: [] },
        ],
        matchedImageUrl: "",
        ozonTitle: "Broom set",
        ozonSpecProfile: {
          color: "白色",
          sizeTokens: ["89cm"],
          countTokens: [],
          material: null,
          modelTokens: [],
          featureTokens: [],
          rawAttributes: [],
        },
      }).row?.rowIndex,
    ).toBe(0);
  });

  test("resolveDetailPricingDecision matches russian ozon profile against chinese 1688 label", () => {
    expect(
      resolveDetailPricingDecision({
        hasSkuSelection: true,
        rows: [
          { rowIndex: 0, label: "白色 89cm 1件", imageUrls: [] },
          { rowIndex: 1, label: "黑色 60cm 2件", imageUrls: [] },
        ],
        matchedImageUrl: "",
        ozonTitle: "Швабра набор",
        ozonSpecProfile: {
          color: "Белый",
          sizeTokens: ["89см"],
          countTokens: ["1шт"],
          material: null,
          modelTokens: [],
          featureTokens: [],
          rawAttributes: [],
        },
      }).row?.rowIndex,
    ).toBe(0);
  });

  test("resolveDetailPricingDecision uses combined color and count signals to beat title-only ambiguity", () => {
    expect(
      resolveDetailPricingDecision({
        hasSkuSelection: true,
        rows: [
          { rowIndex: 0, label: "白色 1件", imageUrls: [] },
          { rowIndex: 1, label: "白色 2件", imageUrls: [] },
        ],
        matchedImageUrl: "",
        ozonTitle: "Cleaning set",
        ozonSpecProfile: {
          color: "白色",
          sizeTokens: [],
          countTokens: ["2件"],
          material: null,
          modelTokens: [],
          featureTokens: [],
          rawAttributes: [],
        },
      }).row?.rowIndex,
    ).toBe(1);
  });

  test("resolveDetailPricingDecision returns manual review when top spec-profile scores are too close", () => {
    expect(
      resolveDetailPricingDecision({
        hasSkuSelection: true,
        rows: [
          { rowIndex: 0, label: "白色 60cm", imageUrls: [] },
          { rowIndex: 1, label: "黑色 89cm", imageUrls: [] },
        ],
        matchedImageUrl: "",
        ozonTitle: "Broom set",
        ozonSpecProfile: {
          color: "白色",
          sizeTokens: ["89cm"],
          countTokens: [],
          material: null,
          modelTokens: [],
          featureTokens: [],
          rawAttributes: [],
        },
      }),
    ).toEqual({
      resolutionMode: "manual_review_required_unknown_spec",
      row: null,
    });
  });

  test("resolveDetailPricingSelectionPlan selects one option per explicit group", () => {
    const plan = resolveDetailPricingSelectionPlan({
      hasSkuSelection: true,
      groups: [
        {
          groupIndex: 0,
          groupLabel: "颜色",
          options: [
            { groupIndex: 0, optionIndex: 0, rowIndex: 0, label: "白色", imageUrls: [] },
            { groupIndex: 0, optionIndex: 1, rowIndex: 1, label: "黑色", imageUrls: [] },
          ],
        },
        {
          groupIndex: 1,
          groupLabel: "尺寸",
          options: [
            { groupIndex: 1, optionIndex: 0, rowIndex: 2, label: "60cm", imageUrls: [] },
            { groupIndex: 1, optionIndex: 1, rowIndex: 3, label: "89cm", imageUrls: [] },
          ],
        },
      ],
      rows: [],
      matchedImageUrl: "",
      ozonTitle: "Broom set",
      ozonSpecProfile: {
        color: "白色",
        sizeTokens: ["89cm"],
        countTokens: [],
        material: null,
        modelTokens: [],
        featureTokens: [],
        rawAttributes: [],
      },
    });

    expect(plan.resolutionMode).toBe("variant_label_payable_total");
    expect(plan.options.map((option) => option.label)).toEqual(["白色", "89cm"]);
    expect(plan.matchedVariantLabel).toBe("白色 / 89cm");
  });

  test("resolveDetailPricingSelectionPlan selects russian ozon profile against chinese grouped options", () => {
    const plan = resolveDetailPricingSelectionPlan({
      hasSkuSelection: true,
      groups: [
        {
          groupIndex: 0,
          groupLabel: "颜色",
          options: [
            { groupIndex: 0, optionIndex: 0, rowIndex: 0, label: "白色", imageUrls: [] },
            { groupIndex: 0, optionIndex: 1, rowIndex: 1, label: "黑色", imageUrls: [] },
          ],
        },
        {
          groupIndex: 1,
          groupLabel: "尺寸",
          options: [
            { groupIndex: 1, optionIndex: 0, rowIndex: 2, label: "60cm", imageUrls: [] },
            { groupIndex: 1, optionIndex: 1, rowIndex: 3, label: "89cm", imageUrls: [] },
          ],
        },
        {
          groupIndex: 2,
          groupLabel: "数量",
          options: [
            { groupIndex: 2, optionIndex: 0, rowIndex: 4, label: "1件", imageUrls: [] },
            { groupIndex: 2, optionIndex: 1, rowIndex: 5, label: "2件", imageUrls: [] },
          ],
        },
      ],
      rows: [],
      matchedImageUrl: "",
      ozonTitle: "Швабра набор",
      ozonSpecProfile: {
        color: "Белый",
        sizeTokens: ["89см"],
        countTokens: ["1шт"],
        material: null,
        modelTokens: [],
        featureTokens: [],
        rawAttributes: [],
      },
    });

    expect(plan.resolutionMode).toBe("variant_label_payable_total");
    expect(plan.options.map((option) => option.label)).toEqual(["白色", "89cm", "1件"]);
  });

  test("resolveDetailPricingSelectionPlan returns manual review when a required group cannot be resolved", () => {
    const plan = resolveDetailPricingSelectionPlan({
      hasSkuSelection: true,
      groups: [
        {
          groupIndex: 0,
          groupLabel: "颜色",
          options: [
            { groupIndex: 0, optionIndex: 0, rowIndex: 0, label: "白色", imageUrls: [] },
            { groupIndex: 0, optionIndex: 1, rowIndex: 1, label: "黑色", imageUrls: [] },
          ],
        },
        {
          groupIndex: 1,
          groupLabel: "尺寸",
          options: [
            { groupIndex: 1, optionIndex: 0, rowIndex: 2, label: "60cm", imageUrls: [] },
            { groupIndex: 1, optionIndex: 1, rowIndex: 3, label: "89cm", imageUrls: [] },
          ],
        },
      ],
      rows: [],
      matchedImageUrl: "",
      ozonTitle: "Broom set",
      ozonSpecProfile: {
        color: "白色",
        sizeTokens: [],
        countTokens: [],
        material: null,
        modelTokens: [],
        featureTokens: [],
        rawAttributes: [],
      },
    });

    expect(plan).toEqual({
      resolutionMode: "manual_review_required_unknown_spec",
      options: [],
      matchedVariantLabel: null,
      row: null,
    });
  });

  test("resolveDetailPricingSelectionPlan accepts single price-inventory row as direct purchase row", () => {
    const plan = resolveDetailPricingSelectionPlan({
      hasSkuSelection: true,
      groups: [
        {
          groupIndex: 0,
          groupLabel: null,
          options: [
            {
              groupIndex: 0,
              optionIndex: 0,
              rowIndex: 0,
              label: "¥38 库存1431盒",
              imageUrls: [],
            },
          ],
        },
      ],
      rows: [{ rowIndex: 0, label: "¥38 库存1431盒", imageUrls: [] }],
      matchedImageUrl: "",
      ozonTitle: "pH test strips",
      ozonSpecProfile: {
        color: null,
        sizeTokens: [],
        countTokens: [],
        material: null,
        modelTokens: [],
        featureTokens: [],
        rawAttributes: [],
      },
    });

    expect(plan.resolutionMode).toBe("variant_label_payable_total");
    expect(plan.options.map((option) => option.rowIndex)).toEqual([0]);
  });

  test("didSelectionPlanApply returns true when all planned row indexes are selected", () => {
    expect(
      didSelectionPlanApply(
        [
          { groupIndex: 0, optionIndex: 0, rowIndex: 0, label: "白色", imageUrls: [] },
          { groupIndex: 1, optionIndex: 1, rowIndex: 3, label: "89cm", imageUrls: [] },
        ],
        {
          selectedRowIndexes: [0, 3],
          rows: [
            {
              rowIndex: 0,
              label: "白色",
              isSelected: true,
              isDisabled: false,
              className: "selected",
              ariaSelected: "true",
            },
            {
              rowIndex: 3,
              label: "89cm",
              isSelected: true,
              isDisabled: false,
              className: "selected",
              ariaSelected: "true",
            },
          ],
        },
      ),
    ).toBe(true);
  });

  test("didSelectionPlanApply returns false when any planned row index is missing", () => {
    expect(
      didSelectionPlanApply(
        [
          { groupIndex: 0, optionIndex: 0, rowIndex: 0, label: "白色", imageUrls: [] },
          { groupIndex: 1, optionIndex: 1, rowIndex: 3, label: "89cm", imageUrls: [] },
        ],
        {
          selectedRowIndexes: [0],
          rows: [
            {
              rowIndex: 0,
              label: "白色",
              isSelected: true,
              isDisabled: false,
              className: "selected",
              ariaSelected: "true",
            },
          ],
        },
      ),
    ).toBe(false);
  });

  test("didSelectionPlanApplyOrRefreshPriceSource accepts pages without selected class when submitOrder changed", () => {
    expect(
      didSelectionPlanApplyOrRefreshPriceSource(
        [
          { groupIndex: 0, optionIndex: 0, rowIndex: 0, label: "白色", imageUrls: [] },
        ],
        {
          selectedRowIndexes: [],
          rows: [
            {
              rowIndex: 0,
              label: "白色",
              isSelected: false,
              isDisabled: false,
              className: "expand-view-item v-flex",
              ariaSelected: null,
            },
          ],
        },
        "立即下单加采购车",
        "立即下单加采购车(已切换规格)",
      ),
    ).toBe(true);
  });

  test("isDetailSelectionRowActive detects active state from descendant classes", () => {
    expect(
      isDetailSelectionRowActive({
        className: "expand-view-item v-flex",
        ariaSelected: null,
        descendantClassNames: ["item-label", "sku-item selected-item"],
        descendantAriaSelected: [null, null],
      }),
    ).toBe(true);
  });

  test("isDetailSelectionRowActive detects active state from descendant aria-selected", () => {
    expect(
      isDetailSelectionRowActive({
        className: "expand-view-item v-flex",
        ariaSelected: null,
        descendantClassNames: ["item-label"],
        descendantAriaSelected: [null, "true"],
      }),
    ).toBe(true);
  });

  test("didQuantityIncrementApply returns true when quantity text changes", () => {
    expect(
      didQuantityIncrementApply(
        {
          quantityText: "1",
          submitOrderText: "商品金额 ¥10.00 运费 ¥5.00",
          plusCandidateCount: 1,
        },
        {
          quantityText: "2",
          submitOrderText: "商品金额 ¥12.80 运费 ¥6.00",
          plusCandidateCount: 1,
        },
      ),
    ).toBe(true);
  });

  test("didQuantityIncrementApply returns false when quantity and submitOrder stay the same", () => {
    expect(
      didQuantityIncrementApply(
        {
          quantityText: "1",
          submitOrderText: "商品金额 ¥10.00 运费 ¥5.00",
          plusCandidateCount: 1,
        },
        {
          quantityText: "1",
          submitOrderText: "商品金额 ¥10.00 运费 ¥5.00",
          plusCandidateCount: 1,
        },
      ),
    ).toBe(false);
  });

  test("parseInlineQuantityValue extracts plain numeric row quantities only", () => {
    expect(parseInlineQuantityValue("0")).toBe(0);
    expect(parseInlineQuantityValue("12")).toBe(12);
    expect(parseInlineQuantityValue("¥12.72")).toBeNull();
  });

  test("inline quantity helpers recognize unicode plus and minus symbols", () => {
    expect(isPlusLikeSymbol("+")).toBe(true);
    expect(isPlusLikeSymbol("＋")).toBe(true);
    expect(isMinusLikeSymbol("-")).toBe(true);
    expect(isMinusLikeSymbol("－")).toBe(true);
  });

  test("didInlineRowQuantityIncrement detects row-level quantity increases", () => {
    expect(
      didInlineRowQuantityIncrement(
        { quantityValue: 0 },
        { quantityValue: 1 },
      ),
    ).toBe(true);
    expect(
      didInlineRowQuantityIncrement(
        { quantityValue: 0 },
        { quantityValue: 0 },
      ),
    ).toBe(false);
  });

  test("didPriceSourceRefresh returns true when submitOrder text changes", () => {
    expect(
      didPriceSourceRefresh("商品金额 ¥10.00 运费 ¥5.00", "商品金额 ¥12.80 运费 ¥6.00"),
    ).toBe(true);
  });

  test("didPriceSourceRefresh returns false when submitOrder text stays the same", () => {
    expect(
      didPriceSourceRefresh("商品金额 ¥10.00 运费 ¥5.00", "商品金额 ¥10.00 运费 ¥5.00"),
    ).toBe(false);
  });

  test("isPriceInventoryOnlyLabel detects quantity-row style labels", () => {
    expect(isPriceInventoryOnlyLabel("¥38\n库存1431盒")).toBe(true);
    expect(isPriceInventoryOnlyLabel("月光白88cM高（三折叠）扫把+簸箕+浴室刮☆")).toBe(false);
  });

  test("deriveDetailPricingFailureCode detects token-empty detail failures", () => {
    expect(
      deriveDetailPricingFailureCode("detail page request failed FAIL_SYS_TOKEN_EMPTY::令牌为空"),
    ).toBe("detail_token_empty");
  });
});

describe("executeResultPageRecall", () => {
  test("returns first-pass results when crop coverage is already full", async () => {
    const calls: string[] = [];

    const recall = await executeResultPageRecall({
      forceFullCrop: false,
      inspectCropCoverage: async () => {
        calls.push("inspect");
        return "full";
      },
      scrapeCurrentPage: async () => {
        calls.push("scrape");
        return sampleResults;
      },
      applyFullCanvasCrop: async () => {
        calls.push("crop");
      },
    });

    expect(recall.results).toEqual(sampleResults);
    expect(recall.usedSecondPassFullCrop).toBe(false);
    expect(calls).toEqual(["inspect", "scrape"]);
  });

  test("runs full-crop retry when coverage is unknown", async () => {
    const calls: string[] = [];

    const recall = await executeResultPageRecall({
      forceFullCrop: false,
      inspectCropCoverage: async () => {
        calls.push("inspect");
        return "unknown";
      },
      scrapeCurrentPage: async () => {
        calls.push("scrape");
        return sampleResults;
      },
      applyFullCanvasCrop: async () => {
        calls.push("crop");
      },
    });

    expect(recall.results).toEqual(sampleResults);
    expect(recall.usedSecondPassFullCrop).toBe(true);
    expect(calls).toEqual(["inspect", "crop", "scrape"]);
  });

  test("runs full-crop retry when coverage is partial", async () => {
    const calls: string[] = [];

    const recall = await executeResultPageRecall({
      forceFullCrop: false,
      inspectCropCoverage: async () => {
        calls.push("inspect");
        return "partial";
      },
      scrapeCurrentPage: async () => {
        calls.push("scrape");
        return sampleResults;
      },
      applyFullCanvasCrop: async () => {
        calls.push("crop");
      },
    });

    expect(recall.results).toEqual(sampleResults);
    expect(recall.usedSecondPassFullCrop).toBe(true);
    expect(calls).toEqual(["inspect", "crop", "scrape"]);
  });

  test("still supports forced full-crop path", async () => {
    const calls: string[] = [];

    const recall = await executeResultPageRecall({
      forceFullCrop: true,
      inspectCropCoverage: async () => {
        calls.push("inspect");
        return "full";
      },
      scrapeCurrentPage: async () => {
        calls.push("scrape");
        return sampleResults;
      },
      applyFullCanvasCrop: async () => {
        calls.push("crop");
      },
    });

    expect(recall.results).toEqual(sampleResults);
    expect(recall.usedSecondPassFullCrop).toBe(true);
    expect(calls).toEqual(["crop", "scrape"]);
  });

  test("preserves FULL_CROP_NOT_APPLIED errors from crop expansion path", async () => {
    await expect(
      executeResultPageRecall({
        forceFullCrop: false,
        inspectCropCoverage: async () => "unknown",
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
