import path from "path";
import { Browser, Page } from "puppeteer";

export interface SearchResult {
  title: string;
  price: string;
  sales: string;
  moq: string;
  shopName: string;
  itemUrl: string;
  imageUrl: string;
  isAd: boolean;
  cosScore: number;
}

export interface SearchRecallResult {
  results: SearchResult[];
  usedSecondPassFullCrop: boolean;
}

export type DetailPricingResolutionMode =
  | "legacy_no_sku_selection"
  | "variant_image_payable_total"
  | "variant_label_payable_total"
  | "manual_review_required_unknown_spec";

export interface DetailVariantRow {
  rowIndex: number;
  label: string;
  imageUrls: string[];
}

export interface DetailSpecOption extends DetailVariantRow {
  groupIndex: number;
  optionIndex: number;
}

export interface DetailSpecGroup {
  groupIndex: number;
  groupLabel: string | null;
  options: DetailSpecOption[];
}

export interface DetailSpecGroupSection {
  label: string | null;
  rows: DetailVariantRow[];
}

export interface DetailPricingResult {
  resolutionMode: DetailPricingResolutionMode;
  price: string | null;
  matchedVariantLabel: string | null;
  basePrice: string | null;
  freightPrice: string | null;
  quantityPlusClicked?: boolean;
  submitOrderText?: string | null;
  diagnostics?: DetailPricingDiagnostics;
}

export interface DetailPricingDiagnostics {
  failureCode?: string | null;
  priceSource?: string | null;
  priceSourceRefreshed?: boolean | null;
  hasSkuSelection?: boolean | null;
  variantRowCount?: number | null;
  selectedRowIndex?: number | null;
  selectionAttempted?: boolean | null;
  selectionApplied?: boolean | null;
  quantityBefore?: string | null;
  quantityAfter?: string | null;
  submitOrderBeforeText?: string | null;
  submitOrderAfterText?: string | null;
  pageScreenshotBase64?: string | null;
  skuSelectionScreenshotBase64?: string | null;
  skuSelectionSnapshot?: {
    hasSkuSelection: boolean;
    groups: DetailSpecGroup[];
    rows: DetailVariantRow[];
  } | null;
  selectionStateBefore?: DetailSelectionSnapshot | null;
  selectionStateAfter?: DetailSelectionSnapshot | null;
  quantitySnapshotBefore?: DetailQuantitySnapshot | null;
  quantitySnapshotAfter?: DetailQuantitySnapshot | null;
  structuredDataProbe?: DetailStructuredDataProbe | null;
}

interface DetailStructuredDataProbe {
  globalStateKeys: string[];
  scriptSnippets: Array<{
    id: string | null;
    type: string | null;
    textSample: string;
  }>;
  networkEvents: Array<{
    url: string;
    status: number | null;
    method: string;
    resourceType: string;
    contentType: string | null;
    bodySample: string | null;
  }>;
}

interface DetailStructuredStateSnapshot {
  contextData: unknown;
  offerDetails: unknown;
}

export interface DetailSelectionSnapshot {
  selectedRowIndexes: number[];
  rows: Array<{
    rowIndex: number;
    label: string;
    isSelected: boolean;
    isDisabled: boolean;
    className: string;
    ariaSelected: string | null;
  }>;
}

export interface DetailQuantitySnapshot {
  quantityText: string | null;
  submitOrderText: string | null;
  plusCandidateCount: number;
}

interface DetailRowQuantityControlSnapshot {
  rowIndex: number;
  quantityText: string | null;
  quantityValue: number | null;
  hasPlusButton: boolean;
  hasMinusButton: boolean;
  controlCandidateCount: number;
}

const DETAIL_SELECTION_ACTIVE_HINTS = [
  "selected",
  "active",
  "current",
  "checked",
  "actived",
  "is-select",
  "selected-item",
  "sku-selected",
];

export interface OzonSpecProfile {
  color?: string | null;
  sizeTokens: string[];
  countTokens: string[];
  material?: string | null;
  modelTokens: string[];
  featureTokens: string[];
  rawAttributes: Array<{ key: string; value: string }>;
}

interface RectBox {
  left: number;
  top: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
}

interface CropPoint {
  x: number;
  y: number;
}

interface CursorProbePoint extends CropPoint {
  cursor: string;
}

type CursorProbeMode = "move" | "resize";
type CropCoverageState = "full" | "unknown" | "partial";

interface FullCanvasCropPlan {
  moveStart: CropPoint;
  moveEnd: CropPoint;
  resizeStart: CropPoint;
  resizeEnd: CropPoint;
}

type SearchResultsPageLike = Pick<Page, "waitForSelector" | "waitForNetworkIdle">;
type ClosableTabLike = {
  url: () => string;
  isClosed?: () => boolean;
};

interface SearchResultRecord {
  title: string;
  priceMajor: string;
  priceMinor: string;
  legacyPriceText: string;
  cardText: string;
  sales: string;
  moq: string;
  shopName: string;
  itemUrl: string;
  imageUrl: string;
  isAd: boolean;
  cosScore: number;
}

interface SearchResultSnapshot {
  records: SearchResultRecord[];
  reachedBottom: boolean;
}

export interface ResultScrollState {
  visibleResultCount: number;
  targetResultCount: number;
  reachedBottom: boolean;
  totalScrolled: number;
  maxScrollDistance: number;
}

const CROP_EDGE_PADDING = 6;
const DEFAULT_RESULT_LIMIT = 36;
const CAMERA_READY_SETTLE_MS = 500;
const SEARCH_CONFIRM_POLL_INTERVAL_MS = 250;
const SEARCH_CONFIRM_MAX_ATTEMPTS = 14;
const IMMEDIATE_RESULT_ENTRY_TIMEOUT_MS = 2_500;
const RESULT_ENTRY_TIMEOUT_MS = 15_000;
const RESULT_ENTRY_POLL_INTERVAL_MS = 250;
const RESULT_SELECTOR_TIMEOUT_MS = 8_000;
const RESULT_IDLE_TIMEOUT_MS = 5_000;
const RESULT_SECONDARY_SELECTOR_TIMEOUT_MS = 4_000;
const RESULT_SCROLL_INTERVAL_MS = 160;
const RESULT_SCROLL_DISTANCE_MIN = 120;
const RESULT_SCROLL_DISTANCE_MAX = 200;
const RESULT_SCROLL_MAX_DISTANCE = 4_000;
const RESULT_POST_SCROLL_SETTLE_MS = 450;
const RESULT_BOTTOM_GAP_PX = 120;
const SEARCH_CONFIRM_LABELS = ["搜索图片", "开始搜索", "立即搜索"];
const RESULT_URL_HINTS = [
  "youyuan",
  "offer_search",
  "imagesearch",
  "tab=imagesearch",
  "tab=imageSearch",
];

function normalizeImageKey(imageUrl: string): string {
  const trimmed = imageUrl.trim();
  if (!trimmed) return "";
  try {
    const url = new URL(trimmed);
    const tail = url.pathname.split("/").pop() || "";
    return tail.toLowerCase();
  } catch {
    return trimmed.split(/[?#]/)[0]?.split("/").pop()?.toLowerCase() || "";
  }
}

function normalizePriceDigits(raw: string): string | null {
  const match = raw.replace(/,/g, "").match(/(\d+(?:\.\d+)?)/);
  return match ? match[1] : null;
}

function parseAmount(raw: string | null | undefined): number | null {
  if (!raw) return null;
  const normalized = normalizePriceDigits(raw);
  if (!normalized) return null;
  const value = Number.parseFloat(normalized);
  return Number.isFinite(value) ? value : null;
}

function formatYuanAmount(amount: number): string {
  return `¥${amount.toFixed(2)}`;
}

export function extractNumericTokens(text: string): string[] {
  const seen = new Set<string>();
  const tokens = (text.match(/\d+(?:\.\d+)?/g) || [])
    .map((token) => token.trim())
    .filter((token) => token.length >= 2)
    .filter((token) => {
      if (seen.has(token)) return false;
      seen.add(token);
      return true;
    });
  return tokens;
}

const COLOR_CANONICAL_MAP: Array<{ pattern: RegExp; canonical: string }> = [
  { pattern: /^(?:белый|белая|белое|white|offwhite|off-white|米白色?|白色?)$/i, canonical: "白色" },
  { pattern: /^(?:черный|чёрный|черная|чёрная|black|黑色?)$/i, canonical: "黑色" },
  { pattern: /^(?:серый|серая|grey|gray|灰色?)$/i, canonical: "灰色" },
  { pattern: /^(?:синий|синяя|blue|голубой|голубая|蓝色?)$/i, canonical: "蓝色" },
  { pattern: /^(?:красный|красная|red|红色?)$/i, canonical: "红色" },
  { pattern: /^(?:зеленый|зелёный|зеленая|зелёная|green|绿色?)$/i, canonical: "绿色" },
  { pattern: /^(?:розовый|розовая|pink|粉色?)$/i, canonical: "粉色" },
  { pattern: /^(?:желтый|жёлтый|желтая|жёлтая|yellow|黄色?)$/i, canonical: "黄色" },
  { pattern: /^(?:фиолетовый|фиолетовая|purple|紫色?)$/i, canonical: "紫色" },
  { pattern: /^(?:оранжевый|оранжевая|orange|橙色?)$/i, canonical: "橙色" },
  { pattern: /^(?:бежевый|бежевая|beige|米色?)$/i, canonical: "米色" },
  { pattern: /^(?:коричневый|коричневая|brown|棕色?)$/i, canonical: "棕色" },
  { pattern: /^(?:серебристый|серебристая|silver|银色?)$/i, canonical: "银色" },
  { pattern: /^(?:золотой|золотая|gold|金色?)$/i, canonical: "金色" },
  { pattern: /^(?:прозрачный|прозрачная|transparent|透明)$/i, canonical: "透明" },
];

function normalizeComparableText(value: string | null | undefined): string {
  let normalized = (value || "").trim().toLowerCase();
  if (!normalized) return "";

  normalized = normalized
    .replace(/[（）()【】\[\]{}]/g, " ")
    .replace(/[\/|,;+]/g, " ")
    .replace(/\s+/g, " ")
    .trim();

  normalized = normalized
    .replace(/(\d+(?:\.\d+)?)\s*(?:см|cm)(?=\s|$)/gi, "$1cm")
    .replace(/(\d+(?:\.\d+)?)\s*(?:мм|mm)(?=\s|$)/gi, "$1mm")
    .replace(/(\d+(?:\.\d+)?)\s*(?:мл|ml)(?=\s|$)/gi, "$1ml")
    .replace(/(\d+(?:\.\d+)?)\s*(?:л|l)(?=\s|$)/gi, "$1l")
    .replace(/(\d+(?:\.\d+)?)\s*(?:кг|kg)(?=\s|$)/gi, "$1kg")
    .replace(/(\d+(?:\.\d+)?)\s*(?:г|gr|gram)(?=\s|$)/gi, "$1g")
    .replace(/(\d+(?:\.\d+)?)\s*(?:шт|pcs?|pieces?|件|个|個|只|支|条)(?=\s|$)/gi, "$1件")
    .replace(/(\d+(?:\.\d+)?)\s*(?:уп|упак|pack|packs)(?=\s|$)/gi, "$1包")
    .replace(/(\d+(?:\.\d+)?)\s*(?:набор|комплект|set|sets|套)(?=\s|$)/gi, "$1套");

  const parts = normalized
    .split(/\s+/)
    .map((part) => canonicalizeColorToken(part) || part)
    .filter((part) => part.length > 0);

  return parts.join(" ");
}

function canonicalizeColorToken(value: string | null | undefined): string | null {
  const normalized = (value || "").trim().toLowerCase();
  if (!normalized) return null;

  for (const entry of COLOR_CANONICAL_MAP) {
    if (entry.pattern.test(normalized)) {
      return entry.canonical;
    }
  }

  return null;
}

export function normalizeComparableSpecToken(value: string | null | undefined): string {
  return normalizeComparableText(value);
}

function comparableIncludes(label: string, token: string): boolean {
  const normalizedLabel = normalizeComparableText(label);
  const normalizedToken = normalizeComparableText(token);
  if (!normalizedLabel || !normalizedToken) return false;
  if (normalizedLabel.includes(normalizedToken)) return true;

  const numericToken = extractNumericTokens(normalizedToken)[0];
  if (!numericToken) return false;

  const labelParts = normalizedLabel.split(/\s+/);
  return labelParts.some((part) => part === numericToken);
}

export function pickVariantRowByImage(
  rows: DetailVariantRow[],
  matchedImageUrl: string,
): DetailVariantRow | null {
  const targetKey = normalizeImageKey(matchedImageUrl);
  if (!targetKey) return null;

  let best: { row: DetailVariantRow; score: number } | null = null;
  for (const row of rows) {
    let rowScore = -1;
    for (const imageUrl of row.imageUrls) {
      const imageKey = normalizeImageKey(imageUrl);
      if (!imageKey) continue;
      if (imageKey === targetKey) {
        rowScore = Math.max(rowScore, 3);
      } else if (imageKey.includes(targetKey) || targetKey.includes(imageKey)) {
        rowScore = Math.max(rowScore, 2);
      }
    }
    if (rowScore < 0) continue;
    if (!best || rowScore > best.score) {
      best = { row, score: rowScore };
    }
  }

  return best?.row ?? null;
}

export function pickVariantRowByLabel(
  rows: DetailVariantRow[],
  ozonTitle: string,
): DetailVariantRow | null {
  const titleTokens = new Set(extractNumericTokens(ozonTitle));
  if (titleTokens.size === 0) return null;

  let best: { row: DetailVariantRow; score: number } | null = null;
  for (const row of rows) {
    const labelTokens = extractNumericTokens(row.label);
    if (labelTokens.length === 0) continue;
    const matchedCount = labelTokens.filter((token) => titleTokens.has(token)).length;
    if (matchedCount === 0) continue;

    const score = matchedCount * 10 + labelTokens.join("").length;
    if (!best || score > best.score) {
      best = { row, score };
    }
  }

  return best?.row ?? null;
}

function scoreVariantRowAgainstSpecProfile(
  row: DetailVariantRow,
  profile: OzonSpecProfile | null | undefined,
): number {
  if (!profile) return 0;

  const label = normalizeComparableText(row.label);
  let score = 0;

  if (profile.color?.trim()) {
    if (comparableIncludes(label, profile.color)) {
      score += 40;
    }
  }

  for (const token of profile.sizeTokens || []) {
    if (comparableIncludes(label, token)) {
      score += 30;
    }
  }

  for (const token of profile.countTokens || []) {
    if (comparableIncludes(label, token)) {
      score += 25;
    }
  }

  for (const token of profile.modelTokens || []) {
    const normalized = normalizeComparableText(token);
    if (normalized && label.includes(normalized)) {
      score += 20;
    }
  }

  return score;
}

export function resolveDetailPricingDecision(input: {
  hasSkuSelection: boolean;
  rows: DetailVariantRow[];
  matchedImageUrl: string;
  ozonTitle: string;
  ozonSpecProfile?: OzonSpecProfile | null;
}): {
  resolutionMode: DetailPricingResolutionMode;
  row: DetailVariantRow | null;
} {
  if (!input.hasSkuSelection) {
    return { resolutionMode: "legacy_no_sku_selection", row: null };
  }

  const matchedByImage = pickVariantRowByImage(input.rows, input.matchedImageUrl);
  if (matchedByImage) {
    return {
      resolutionMode: "variant_image_payable_total",
      row: matchedByImage,
    };
  }

  if (input.ozonSpecProfile) {
    const scoredRows = input.rows
      .map((row) => ({
        row,
        score: scoreVariantRowAgainstSpecProfile(row, input.ozonSpecProfile),
      }))
      .filter((entry) => entry.score > 0)
      .sort((a, b) => b.score - a.score);

    const best = scoredRows[0];
    const second = scoredRows[1];
    if (best && (!second || best.score - second.score >= 20)) {
      return {
        resolutionMode: "variant_label_payable_total",
        row: best.row,
      };
    }
  }

  const matchedByLabel = pickVariantRowByLabel(input.rows, input.ozonTitle);
  if (matchedByLabel) {
    return {
      resolutionMode: "variant_label_payable_total",
      row: matchedByLabel,
    };
  }

  return {
    resolutionMode: "manual_review_required_unknown_spec",
    row: null,
  };
}

function isColorGroupLabel(label: string | null | undefined): boolean {
  const normalized = (label || "").toLowerCase();
  return /颜色|色彩|色号|цвет/.test(normalized);
}

function isSizeGroupLabel(label: string | null | undefined): boolean {
  const normalized = (label || "").toLowerCase();
  return /尺寸|大小|长度|规格|size|размер|длина/.test(normalized);
}

function isCountGroupLabel(label: string | null | undefined): boolean {
  const normalized = (label || "").toLowerCase();
  return /数量|件数|套装|规格数|count|qty|数量选择|колич/.test(normalized);
}

function scoreOptionForGroup(
  option: DetailSpecOption,
  groupLabel: string | null,
  ozonTitle: string,
  profile: OzonSpecProfile | null | undefined,
): number {
  const label = normalizeComparableText(option.label);

  if (isColorGroupLabel(groupLabel)) {
    return profile?.color && comparableIncludes(label, profile.color) ? 100 : 0;
  }

  if (isSizeGroupLabel(groupLabel)) {
    let score = 0;
    for (const token of profile?.sizeTokens || []) {
      if (comparableIncludes(label, token)) score += 100;
    }
    return score;
  }

  if (isCountGroupLabel(groupLabel)) {
    let score = 0;
    for (const token of profile?.countTokens || []) {
      if (comparableIncludes(label, token)) score += 100;
    }
    return score;
  }

  return scoreVariantRowAgainstSpecProfile(option, profile) + (
    pickVariantRowByLabel([{ rowIndex: option.rowIndex, label: option.label, imageUrls: option.imageUrls }], ozonTitle)
      ? 10
      : 0
  );
}

function pickBestOptionInGroup(input: {
  group: DetailSpecGroup;
  ozonTitle: string;
  ozonSpecProfile?: OzonSpecProfile | null;
}): DetailSpecOption | null {
  const scored = input.group.options
    .map((option) => ({
      option,
      score: scoreOptionForGroup(
        option,
        input.group.groupLabel,
        input.ozonTitle,
        input.ozonSpecProfile,
      ),
    }))
    .filter((entry) => entry.score > 0)
    .sort((a, b) => b.score - a.score);

  const best = scored[0];
  const second = scored[1];
  if (!best) return null;
  if (second && best.score === second.score) return null;
  return best.option;
}

export function resolveDetailPricingSelectionPlan(input: {
  hasSkuSelection: boolean;
  groups: DetailSpecGroup[];
  rows: DetailVariantRow[];
  matchedImageUrl: string;
  ozonTitle: string;
  ozonSpecProfile?: OzonSpecProfile | null;
}): {
  resolutionMode: DetailPricingResolutionMode;
  options: DetailSpecOption[];
  matchedVariantLabel: string | null;
  row: DetailVariantRow | null;
} {
  if (!input.hasSkuSelection) {
    return {
      resolutionMode: "legacy_no_sku_selection",
      options: [],
      matchedVariantLabel: null,
      row: null,
    };
  }

  if (input.groups.length <= 1) {
    if (input.rows.length === 1 && isPriceInventoryOnlyLabel(input.rows[0]?.label)) {
      const option = input.groups[0]?.options[0];
      return {
        resolutionMode: "variant_label_payable_total",
        options: option ? [option] : [],
        matchedVariantLabel: input.rows[0]?.label || null,
        row: input.rows[0] ?? null,
      };
    }

    const decision = resolveDetailPricingDecision({
      hasSkuSelection: input.hasSkuSelection,
      rows: input.rows,
      matchedImageUrl: input.matchedImageUrl,
      ozonTitle: input.ozonTitle,
      ozonSpecProfile: input.ozonSpecProfile,
    });

    const option =
      decision.row &&
      input.groups[0]?.options.find((candidate) => candidate.rowIndex === decision.row?.rowIndex);

    return {
      resolutionMode: decision.resolutionMode,
      options: option ? [option] : [],
      matchedVariantLabel: decision.row?.label || null,
      row: decision.row,
    };
  }

  const options: DetailSpecOption[] = [];
  for (const group of input.groups) {
    const option = pickBestOptionInGroup({
      group,
      ozonTitle: input.ozonTitle,
      ozonSpecProfile: input.ozonSpecProfile,
    });
    if (!option) {
      return {
        resolutionMode: "manual_review_required_unknown_spec",
        options: [],
        matchedVariantLabel: null,
        row: null,
      };
    }
    options.push(option);
  }

  return {
    resolutionMode: "variant_label_payable_total",
    options,
    matchedVariantLabel: options.map((option) => option.label).join(" / ") || null,
    row: null,
  };
}

export function didSelectionPlanApply(
  options: DetailSpecOption[],
  snapshot: DetailSelectionSnapshot | null,
): boolean {
  if (!snapshot) return false;
  if (options.length === 0) return false;

  const selected = new Set(snapshot.selectedRowIndexes);
  return options.every((option) => selected.has(option.rowIndex));
}

export function didSelectionPlanApplyOrRefreshPriceSource(
  options: DetailSpecOption[],
  snapshot: DetailSelectionSnapshot | null,
  beforeText: string | null | undefined,
  afterText: string | null | undefined,
): boolean {
  return (
    didSelectionPlanApply(options, snapshot) ||
    didPriceSourceRefresh(beforeText, afterText)
  );
}

export function didQuantityIncrementApply(
  before: DetailQuantitySnapshot | null,
  after: DetailQuantitySnapshot | null,
): boolean {
  if (!before || !after) return false;

  if (
    before.quantityText &&
    after.quantityText &&
    before.quantityText.trim() !== after.quantityText.trim()
  ) {
    return true;
  }

  if (
    before.submitOrderText &&
    after.submitOrderText &&
    before.submitOrderText.trim() !== after.submitOrderText.trim()
  ) {
    return true;
  }

  return false;
}

export function didPriceSourceRefresh(
  beforeText: string | null | undefined,
  afterText: string | null | undefined,
): boolean {
  const before = (beforeText || "").trim();
  const after = (afterText || "").trim();
  return before.length > 0 && after.length > 0 && before !== after;
}

export function isPriceInventoryOnlyLabel(label: string | null | undefined): boolean {
  const normalized = (label || "").replace(/\s+/g, " ").trim();
  if (!normalized) return false;
  if (!/¥\s*\d/.test(normalized)) return false;
  if (!/库存\s*\d+/.test(normalized)) return false;
  return !/[【】☆]/.test(normalized);
}

export function parseInlineQuantityValue(value: string | null | undefined): number | null {
  const normalized = (value || "").trim();
  if (!normalized) return null;
  const match = normalized.match(/^\d+$/);
  if (!match) return null;
  const parsed = Number.parseInt(match[0], 10);
  return Number.isFinite(parsed) ? parsed : null;
}

export function didInlineRowQuantityIncrement(
  before: Pick<DetailRowQuantityControlSnapshot, "quantityValue"> | null,
  after: Pick<DetailRowQuantityControlSnapshot, "quantityValue"> | null,
): boolean {
  if (!before || !after) return false;
  if (before.quantityValue === null || after.quantityValue === null) return false;
  return after.quantityValue > before.quantityValue;
}

function hasInlineRowQuantityControls(
  snapshot: DetailRowQuantityControlSnapshot | null,
): boolean {
  if (!snapshot) return false;
  if (snapshot.quantityValue === null) return false;
  return snapshot.hasPlusButton || snapshot.controlCandidateCount >= 2;
}

export function isPlusLikeSymbol(text: string): boolean {
  return ["+", "＋", "﹢"].includes(text);
}

export function isMinusLikeSymbol(text: string): boolean {
  return ["-", "−", "－", "﹣", "—"].includes(text);
}

export function deriveDetailPricingFailureCode(message: string): string | null {
  const bracketed = message.match(/\[(.*?)\]/)?.[1]?.trim().toLowerCase().replace(/\s+/g, "_");
  if (bracketed) return bracketed;
  if (/FAIL_SYS_TOKEN_EMPTY/i.test(message)) return "detail_token_empty";
  return null;
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function normalizeStructuredVariantLabel(value: string | null | undefined): string | null {
  const normalized = (value || "").replace(/\s+/g, " ").trim();
  if (!normalized) return null;
  if (normalized.length > 120) return null;
  if (/^https?:\/\//i.test(normalized)) return null;
  if (/^\d+(?:\.\d+)?$/.test(normalized)) return null;
  if (isBlockedStructuredVariantLabel(normalized)) return null;
  if (!/[\u4e00-\u9fffA-Za-zА-Яа-я0-9]/.test(normalized)) return null;
  return normalized;
}

function isBlockedStructuredVariantLabel(value: string): boolean {
  const normalized = value.trim().toLowerCase();
  if (!normalized) return true;

  return [
    "立即下单",
    "加采购车",
    "跨境铺货",
    "普通买家",
    "商品sku预览",
    "官方验货",
    "下单面板",
    "规格选择器",
    "下单面板主图",
    "满50包邮",
    "首单减2元",
    "分享再减28元",
    "酸碱度范围",
    "总体尺寸",
    "产品类型",
    "保质期",
  ].some((token) => normalized === token.toLowerCase());
}

function collectStructuredImageUrls(value: unknown): string[] {
  if (!isPlainRecord(value)) return [];
  const candidates = [
    value.imageUrl,
    value.imgUrl,
    value.image,
    value.picUrl,
    value.thumbnail,
    value.icon,
  ];
  return candidates
    .filter((candidate): candidate is string => typeof candidate === "string")
    .map((candidate) => candidate.trim())
    .filter((candidate) => /^https?:\/\//i.test(candidate));
}

export function extractStructuredVariantRowsFromData(data: unknown): DetailVariantRow[] {
  const seenNodes = new Set<unknown>();
  const seenLabels = new Set<string>();
  const rows: DetailVariantRow[] = [];
  const labelKeys = [
    "displayName",
    "skuName",
    "specName",
    "name",
    "label",
    "value",
    "text",
    "title",
  ];

  const isAllowedStructuredSkuPath = (pathText: string): boolean =>
    /(salep?rop|sku(model|info|infos)?|specselector|skupreview|skupreview|tradeprop)/i.test(pathText);

  const visit = (value: unknown, path: string[] = []): void => {
    if (value === null || value === undefined) return;
    if (typeof value !== "object") return;
    if (seenNodes.has(value)) return;
    seenNodes.add(value);

    if (Array.isArray(value)) {
      for (const item of value) {
        visit(item, path);
      }
      return;
    }

    const pathText = path.join(".").toLowerCase();
    const keys = Object.keys(value);
    const hasNestedCollections = Object.values(value).some(
      (child) => Array.isArray(child) && child.length > 0,
    );
    const pathLooksRelevant =
      isAllowedStructuredSkuPath(pathText) ||
      keys.some((key) => isAllowedStructuredSkuPath(`${pathText}.${key}`));
    const pathLooksBlocked =
      /(attribute|productinfo|button|moduletitle|detailattribute|panel|preview|official|freight|carriage)/i.test(
        pathText,
      );

    if (pathLooksRelevant && !pathLooksBlocked) {
      for (const key of labelKeys) {
        if (hasNestedCollections && ["name", "label", "title"].includes(key)) {
          continue;
        }
        const label = normalizeStructuredVariantLabel(
          typeof value[key] === "string" ? (value[key] as string) : null,
        );
        if (!label) continue;
        if (seenLabels.has(label)) continue;
        seenLabels.add(label);
        rows.push({
          rowIndex: rows.length,
          label,
          imageUrls: collectStructuredImageUrls(value),
        });
        break;
      }
    }

    for (const [key, child] of Object.entries(value)) {
      visit(child, [...path, key]);
    }
  };

  visit(data, []);
  return rows;
}

export function extractStructuredFreightPriceFromNetworkEvents(
  events: DetailStructuredDataProbe["networkEvents"] | null | undefined,
): string | null {
  for (const event of events || []) {
    const url = event.url || "";
    const body = event.bodySample || "";
    if (!/freightinfoservice|getfreightinfowithscene/i.test(url)) continue;
    const match = body.match(/"totalCost"\s*:\s*(\d+(?:\.\d+)?)/);
    if (!match) continue;
    const amount = Number.parseFloat(match[1]);
    if (!Number.isFinite(amount)) continue;
    return formatYuanAmount(amount);
  }

  return null;
}

async function readStructuredDetailStateSnapshot(page: Page): Promise<DetailStructuredStateSnapshot | null> {
  try {
    return await page.evaluate(() => {
      const clone = (value: unknown): unknown => {
        try {
          return JSON.parse(JSON.stringify(value ?? null));
        } catch {
          return null;
        }
      };

      return {
        contextData: clone((window as Window & { context?: { result?: { data?: unknown } } }).context?.result?.data ?? null),
        offerDetails: clone((window as Window & { offer_details?: unknown }).offer_details ?? null),
      };
    });
  } catch {
    return null;
  }
}

export function isDetailSelectionRowActive(input: {
  className: string;
  ariaSelected?: string | null;
  descendantClassNames?: string[];
  descendantAriaSelected?: Array<string | null>;
}): boolean {
  const tokens = [
    input.className || "",
    ...(input.descendantClassNames || []),
    input.ariaSelected || "",
    ...(input.descendantAriaSelected || []),
  ]
    .join(" ")
    .toLowerCase();

  if (input.ariaSelected === "true") return true;
  if ((input.descendantAriaSelected || []).some((value) => value === "true")) return true;

  return DETAIL_SELECTION_ACTIVE_HINTS.some((hint) => tokens.includes(hint));
}

export function buildDetailSpecGroupsFromSections(
  sections: DetailSpecGroupSection[],
): DetailSpecGroup[] {
  return sections
    .filter((section) => section.rows.length > 0)
    .map((section, groupIndex) => ({
      groupIndex,
      groupLabel: section.label?.trim() || null,
      options: section.rows.map((row, optionIndex) => ({
        ...row,
        groupIndex,
        optionIndex,
      })),
    }));
}

async function readDetailVariantRows(page: Page): Promise<{
  hasSkuSelection: boolean;
  groups: DetailSpecGroup[];
  rows: DetailVariantRow[];
}> {
  const domSnapshot = await page.evaluate(() => {
    const normalizeLabel = (value: string | null | undefined): string | null => {
      const normalized = (value || "").replace(/\s+/g, " ").trim();
      return normalized || null;
    };

    const container = document.getElementById("skuSelection");
    if (!(container instanceof HTMLElement)) {
      return { hasSkuSelection: false, groups: [], rows: [] };
    }

    const toRow = (row: HTMLElement, rowIndex: number) => {
      const label =
        row.querySelector<HTMLElement>("span.item-label")?.innerText?.trim() ||
        row.innerText?.trim() ||
        "";
      const imageUrls = Array.from(row.querySelectorAll<HTMLImageElement>("img"))
        .map((img) => img.currentSrc || img.src || "")
        .filter((value) => value.trim().length > 0);

      return {
        rowIndex,
        label,
        imageUrls,
      };
    };

    const directChildren = Array.from(container.children).filter(
      (child): child is HTMLElement => child instanceof HTMLElement,
    );

    let pendingLabel: string | null = null;
    let globalRowIndex = 0;
    const sections: Array<{ label: string | null; rows: DetailVariantRow[] }> = [];

    for (const child of directChildren) {
      const rows = Array.from(
        child.querySelectorAll<HTMLElement>('div.expand-view-item.v-flex'),
      );

      if (rows.length === 0) {
        const text = normalizeLabel(child.innerText || child.textContent || "");
        if (text) pendingLabel = text;
        continue;
      }

      const rowsInSection = rows.map((row) => toRow(row, globalRowIndex++));
      let sectionLabel = pendingLabel;

      if (!sectionLabel) {
        const labelNode = Array.from(child.children).find((candidate) => {
          if (!(candidate instanceof HTMLElement)) return false;
          if (candidate.matches('div.expand-view-item.v-flex')) return false;
          return normalizeLabel(candidate.innerText || candidate.textContent || "") !== null;
        });
        sectionLabel = normalizeLabel(
          labelNode instanceof HTMLElement
            ? labelNode.innerText || labelNode.textContent || ""
            : null,
        );
      }

      sections.push({
        label: sectionLabel,
        rows: rowsInSection,
      });
      pendingLabel = null;
    }

    if (sections.length === 0) {
      const rows = Array.from(
        container.querySelectorAll<HTMLElement>('div.expand-view-item.v-flex'),
      ).map((row) => toRow(row, globalRowIndex++));
      if (rows.length > 0) {
        sections.push({ label: null, rows });
      }
    }

    const groups = sections
      .filter((section) => section.rows.length > 0)
      .map((section, groupIndex) => ({
        groupIndex,
        groupLabel: section.label?.trim() || null,
        options: section.rows.map((row, optionIndex) => ({
          ...row,
          groupIndex,
          optionIndex,
        })),
      }));
    const rows = groups.flatMap((group) =>
      group.options.map((option) => ({
        rowIndex: option.rowIndex,
        label: option.label,
        imageUrls: option.imageUrls,
      })),
    );

    return { hasSkuSelection: true, groups, rows };
  });

  if (domSnapshot.rows.length > 0 || !domSnapshot.hasSkuSelection) {
    return domSnapshot;
  }

  const structuredState = await readStructuredDetailStateSnapshot(page);
  const structuredRows = [
    ...extractStructuredVariantRowsFromData(structuredState?.contextData),
    ...extractStructuredVariantRowsFromData(structuredState?.offerDetails),
  ];
  const dedupedRows = structuredRows.filter((row, index, rows) =>
    rows.findIndex((candidate) => candidate.label === row.label) === index,
  );

  if (dedupedRows.length === 0) {
    return domSnapshot;
  }

  return {
    hasSkuSelection: true,
    groups: buildDetailSpecGroupsFromSections([{ label: null, rows: dedupedRows }]),
    rows: dedupedRows,
  };
}

async function capturePageScreenshotBase64(page: Page): Promise<string | null> {
  try {
    const screenshot = await page.screenshot({ type: "png", fullPage: true });
    return Buffer.from(screenshot).toString("base64");
  } catch {
    return null;
  }
}

async function captureElementScreenshotBase64(
  page: Page,
  selector: string,
): Promise<string | null> {
  const handle = await page.$(selector);
  if (!handle) return null;

  try {
    const screenshot = await handle.screenshot({ type: "png" });
    return Buffer.from(screenshot).toString("base64");
  } catch {
    return null;
  } finally {
    await handle.dispose().catch(() => undefined);
  }
}

async function readDetailSelectionSnapshot(page: Page): Promise<DetailSelectionSnapshot | null> {
  try {
    return await page.evaluate((activeHints) => {
      const getClassName = (node: Element | null | undefined): string => {
        if (!node) return "";
        const raw = (node as Element & { className?: unknown }).className;
        if (typeof raw === "string") return raw;
        if (
          raw &&
          typeof raw === "object" &&
          "baseVal" in raw &&
          typeof (raw as { baseVal?: unknown }).baseVal === "string"
        ) {
          return (raw as { baseVal: string }).baseVal;
        }
        return "";
      };

      const container = document.getElementById("skuSelection");
      if (!(container instanceof HTMLElement)) return null;

      const rows = Array.from(
        container.querySelectorAll<HTMLElement>('div.expand-view-item.v-flex'),
      ).map((row, index) => {
        const label =
          row.querySelector<HTMLElement>("span.item-label")?.innerText?.trim() ||
          row.innerText?.trim() ||
          "";
        const className = getClassName(row);
        const ariaSelected = row.getAttribute("aria-selected");
        const descendants = Array.from(row.querySelectorAll("*"));
        const descendantClassNames = descendants
          .map((node) => getClassName(node))
          .filter((value) => value.trim().length > 0);
        const descendantAriaSelected = descendants.map((node) => node.getAttribute("aria-selected"));
        const tokens = [
          className,
          ...descendantClassNames,
          ariaSelected || "",
          ...descendantAriaSelected.filter((value): value is string => typeof value === "string"),
        ]
          .join(" ")
          .toLowerCase();
        const isSelected =
          ariaSelected === "true" ||
          descendantAriaSelected.some((value) => value === "true") ||
          activeHints.some((hint) => tokens.includes(hint));
        const isDisabled =
          row.getAttribute("aria-disabled") === "true" ||
          row.hasAttribute("disabled") ||
          `${className} ${descendantClassNames.join(" ")}`.toLowerCase().includes("disabled");

        return {
          rowIndex: index,
          label,
          isSelected,
          isDisabled,
          className,
          ariaSelected,
        };
      });

      return {
        selectedRowIndexes: rows.filter((row) => row.isSelected).map((row) => row.rowIndex),
        rows,
      };
    }, DETAIL_SELECTION_ACTIVE_HINTS);
  } catch {
    return null;
  }
}

async function readDetailRowQuantityControlSnapshot(
  page: Page,
  rowIndex: number,
): Promise<DetailRowQuantityControlSnapshot | null> {
  return page.evaluate((targetRowIndex) => {
    const collectLineState = (row: HTMLElement, container: HTMLElement) => {
      const rowRect = row.getBoundingClientRect();
      const rowCenterY = rowRect.top + rowRect.height / 2;
      const allNodes = Array.from(container.querySelectorAll<HTMLElement>("button, div, span, a"));
      const sameLineNodes = allNodes.filter((node) => {
        const rect = node.getBoundingClientRect();
        if (rect.width < 12 || rect.height < 12 || rect.bottom <= 0 || rect.right <= 0) return false;
        const centerY = rect.top + rect.height / 2;
        return Math.abs(centerY - rowCenterY) <= Math.max(rowRect.height, 48);
      });

      const quantityNode =
        sameLineNodes.find((node) => /^\d+$/.test((node.innerText || node.textContent || "").trim())) ||
        null;
      const hasPlusButton = sameLineNodes.some((node) =>
        ["+", "＋", "﹢"].includes((node.innerText || node.textContent || "").replace(/\s+/g, "")),
      );
      const hasMinusButton = sameLineNodes.some((node) =>
        ["-", "−", "－", "﹣", "—"].includes((node.innerText || node.textContent || "").replace(/\s+/g, "")),
      );
      const quantityText = quantityNode
        ? (quantityNode.innerText || quantityNode.textContent || "").trim() || null
        : null;

      return {
        quantityText,
        quantityValue: quantityText && /^\d+$/.test(quantityText) ? Number.parseInt(quantityText, 10) : null,
        hasPlusButton,
        hasMinusButton,
        controlCandidateCount: sameLineNodes.length,
      };
    };

    const rows = Array.from(
      document.querySelectorAll<HTMLElement>('#skuSelection div.expand-view-item.v-flex'),
    );
    const row = rows[targetRowIndex];
    if (!(row instanceof HTMLElement)) return null;
    const container = document.getElementById("skuSelection");
    if (!(container instanceof HTMLElement)) return null;

    const candidates = Array.from(row.querySelectorAll<HTMLElement>("button, div, span, a"));
    const visibleCandidates = candidates.filter((node) => {
      const rect = node.getBoundingClientRect();
      return rect.width >= 12 && rect.height >= 12 && rect.bottom > 0 && rect.right > 0;
    });
    const quantityNode =
      visibleCandidates.find((node) => /^\d+$/.test((node.innerText || node.textContent || "").trim())) ||
      null;
    const hasPlusButton = visibleCandidates.some((node) =>
      isPlusLikeSymbol((node.innerText || node.textContent || "").replace(/\s+/g, "")),
    );
    const hasMinusButton = visibleCandidates.some((node) =>
      isMinusLikeSymbol((node.innerText || node.textContent || "").replace(/\s+/g, "")),
    );
    const quantityText = quantityNode
      ? (quantityNode.innerText || quantityNode.textContent || "").trim() || null
      : null;

    const directState = {
      rowIndex: targetRowIndex,
      quantityText,
      quantityValue: quantityText && /^\d+$/.test(quantityText) ? Number.parseInt(quantityText, 10) : null,
      hasPlusButton,
      hasMinusButton,
      controlCandidateCount: visibleCandidates.length,
    };

    if (directState.quantityValue !== null || directState.hasPlusButton) {
      return directState;
    }

    return {
      rowIndex: targetRowIndex,
      ...collectLineState(row, container),
    };
  }, rowIndex);
}

async function readDetailQuantitySnapshot(page: Page): Promise<DetailQuantitySnapshot | null> {
  return page.evaluate(() => {
    const submitOrder = document.getElementById("submitOrder");
    const plusCandidates = Array.from(
      document.querySelectorAll<HTMLElement>("button, div, span, a"),
    ).filter((node) => {
      const text = (node.innerText || node.textContent || "").replace(/\s+/g, "");
      if (text !== "+") return false;
      const rect = node.getBoundingClientRect();
      return rect.width >= 16 && rect.height >= 16 && rect.bottom > 0 && rect.right > 0;
    });

    const quantityText =
      plusCandidates
        .map((node) => (node.parentElement?.innerText || "").trim())
        .find((text) => /\d/.test(text)) || null;

    return {
      quantityText,
      submitOrderText: submitOrder?.textContent?.trim() || null,
      plusCandidateCount: plusCandidates.length,
    };
  });
}

async function collectDetailStructuredDataProbe(
  page: Page,
  networkEvents: DetailStructuredDataProbe["networkEvents"],
): Promise<DetailStructuredDataProbe> {
  const pageProbe = await page.evaluate(() => {
    const interestingGlobalKeys = Object.keys(window)
      .filter((key) => /sku|price|offer|detail|config|data|state|trade/i.test(key))
      .slice(0, 40);
    const scriptSnippets = Array.from(document.querySelectorAll<HTMLScriptElement>("script"))
      .map((script) => {
        const text = (script.textContent || "").replace(/\s+/g, " ").trim();
        return {
          id: script.id || null,
          type: script.type || null,
          textSample: text.slice(0, 400),
        };
      })
      .filter((entry) => {
        const signature = `${entry.id || ""} ${entry.type || ""} ${entry.textSample}`.toLowerCase();
        return /sku|price|offer|detail|cargo|freight|rate|config|state|data/i.test(signature);
      })
      .slice(0, 20);

    return {
      globalStateKeys: interestingGlobalKeys,
      scriptSnippets,
    };
  });

  return {
    globalStateKeys: pageProbe.globalStateKeys,
    scriptSnippets: pageProbe.scriptSnippets,
    networkEvents: networkEvents.slice(-20),
  };
}

async function selectVariantRow(page: Page, rowIndex: number): Promise<void> {
  const rows = await page.$$('#skuSelection div.expand-view-item.v-flex');
  const row = rows[rowIndex];
  if (!row) {
    throw new Error(`[DETAIL_VARIANT_SELECT_FAILED] row=${rowIndex}`);
  }

  const clickable =
    (await row.$("span.item-label")) ||
    (await row.$("img")) ||
    (await row.$("[role='button']")) ||
    (await row.$("button")) ||
    row;

  await clickable.evaluate((node) => {
    if (!(node instanceof HTMLElement)) return;
    node.scrollIntoView({ block: "center", inline: "nearest" });
  });

  const tryClick = async (target: typeof clickable) => {
    try {
      await target.click({ delay: 50 });
      return true;
    } catch {
      return false;
    }
  };

  let clicked = await tryClick(clickable);
  if (!clicked && clickable !== row) {
    clicked = await tryClick(row);
  }

  if (!clicked) {
    const box = await clickable.boundingBox();
    if (box) {
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2, { delay: 50 });
      clicked = true;
    }
  }

  await clickable.dispose().catch(() => undefined);
  await Promise.all(
    rows.map((handle) => handle.dispose().catch(() => undefined)),
  );

  if (!clicked) {
    throw new Error(`[DETAIL_VARIANT_SELECT_FAILED] row=${rowIndex}`);
  }

  await delay(250);
}

async function clickInlineRowQuantityPlusButton(page: Page, rowIndex: number): Promise<void> {
  const clicked = await page.evaluate((targetRowIndex) => {
    const rows = Array.from(
      document.querySelectorAll<HTMLElement>('#skuSelection div.expand-view-item.v-flex'),
    );
    const row = rows[targetRowIndex];
    const container = document.getElementById("skuSelection");
    if (!(row instanceof HTMLElement) || !(container instanceof HTMLElement)) return false;

    const rowRect = row.getBoundingClientRect();
    const rowCenterY = rowRect.top + rowRect.height / 2;
    const plusCandidates = Array.from(container.querySelectorAll<HTMLElement>("button, div, span, a"))
      .filter((node) => {
        const text = (node.innerText || node.textContent || "").replace(/\s+/g, "");
        if (!["+", "＋", "﹢"].includes(text)) return false;
        const rect = node.getBoundingClientRect();
        if (rect.width < 12 || rect.height < 12 || rect.bottom <= 0 || rect.right <= 0) return false;
        const centerY = rect.top + rect.height / 2;
        return Math.abs(centerY - rowCenterY) <= Math.max(rowRect.height, 48);
      })
      .map((node) => ({ node, rect: node.getBoundingClientRect() }))
      .sort((a, b) => b.rect.left - a.rect.left);

    const target = plusCandidates[0]?.node;
    if (!(target instanceof HTMLElement)) return false;
    target.scrollIntoView({ block: "center", inline: "nearest" });
    target.click();
    return true;
  }, rowIndex);

  if (!clicked) {
    throw new Error(`[DETAIL_ROW_QUANTITY_PLUS_NOT_FOUND] row=${rowIndex}`);
  }

  await delay(250);
}

async function clickQuantityPlusButton(page: Page): Promise<void> {
  const clicked = await page.evaluate(() => {
    const submitOrder = document.getElementById("submitOrder");
    const candidates = Array.from(
      document.querySelectorAll<HTMLElement>("button, div, span, a"),
    ).filter((node) => {
      const text = (node.innerText || node.textContent || "").replace(/\s+/g, "");
      if (text !== "+") return false;
      const rect = node.getBoundingClientRect();
      return rect.width >= 16 && rect.height >= 16 && rect.bottom > 0 && rect.right > 0;
    });

    if (candidates.length === 0) return false;

    const submitRect = submitOrder?.getBoundingClientRect() || null;
    const scored = candidates
      .map((node) => {
        const rect = node.getBoundingClientRect();
        const parentText = (node.parentElement?.innerText || "").replace(/\s+/g, "");
        let score = rect.left + rect.top;

        if (/\d/.test(parentText)) score += 500;
        if (parentText.includes("-")) score += 200;
        if (submitRect) {
          const verticalDistance = Math.abs(rect.top - submitRect.top);
          score += Math.max(0, 400 - verticalDistance);
        }

        return { node, score };
      })
      .sort((a, b) => b.score - a.score);

    const winner = scored[0]?.node;
    if (!(winner instanceof HTMLElement)) return false;
    winner.scrollIntoView({ block: "center", inline: "nearest" });
    winner.click();
    return true;
  });

  if (!clicked) {
    throw new Error("[DETAIL_QUANTITY_PLUS_CLICK_FAILED]");
  }
}

async function waitForSubmitOrderTotals(page: Page): Promise<void> {
  await page.waitForFunction(() => {
    const submitOrder = document.getElementById("submitOrder");
    if (!(submitOrder instanceof HTMLElement)) return false;
    const totalPrice = submitOrder.querySelector<HTMLElement>('span[class*="total-price"]');
    const freight = submitOrder.querySelector<HTMLElement>(
      'span[class*="total-freight-fee"]',
    );
    const priceText = (totalPrice?.innerText || totalPrice?.textContent || "").trim();
    const freightText = (freight?.innerText || freight?.textContent || "").trim();
    return /\d/.test(priceText) && /\d/.test(freightText);
  }, { timeout: 10_000 });
}

async function readSubmitOrderTotals(page: Page): Promise<{
  basePrice: string | null;
  freightPrice: string | null;
  totalPrice: string | null;
  submitOrderText: string | null;
}> {
  return page.evaluate(() => {
    const submitOrder = document.getElementById("submitOrder");
    if (!(submitOrder instanceof HTMLElement)) {
      return {
        basePrice: null,
        freightPrice: null,
        totalPrice: null,
        submitOrderText: null,
      };
    }

    const basePrice =
      submitOrder.querySelector<HTMLElement>('span[class*="total-price"]')?.innerText?.trim() ||
      null;
    const freightPrice =
      submitOrder
        .querySelector<HTMLElement>('span[class*="total-freight-fee"]')
        ?.innerText?.trim() || null;

    const baseValue = parseAmount(basePrice);
    const freightValue = parseAmount(freightPrice);
    const totalPrice =
      baseValue !== null && freightValue !== null
        ? formatYuanAmount(baseValue + freightValue)
        : null;

    return {
      basePrice,
      freightPrice,
      totalPrice,
      submitOrderText: submitOrder.innerText?.trim() || null,
    };
  });
}

export async function resolve1688DetailPricing(
  browser: Browser,
  itemUrl: string,
  cardPrice: string,
  matchedImageUrl: string,
  ozonTitle: string,
  ozonSpecProfile?: OzonSpecProfile | null,
): Promise<DetailPricingResult> {
  const page = await browser.newPage();
  const networkEvents: DetailStructuredDataProbe["networkEvents"] = [];
  let selectedVariantLabel: string | null = null;
  let selectedRowIndex: number | null = null;
  let selectionAttempted = false;
  let selectionApplied = false;
  let quantityPlusClicked = false;
  let submitOrderText: string | null = null;
  let basePriceText: string | null = null;
  let freightPriceText: string | null = null;
  let submitOrderBeforeText: string | null = null;
  let submitOrderAfterText: string | null = null;
  let quantityBefore: string | null = null;
  let quantityAfter: string | null = null;
  let hasSkuSelection = false;
  let variantRowCount = 0;
  let pageScreenshotBase64: string | null = null;
  let skuSelectionScreenshotBase64: string | null = null;
  let skuSelectionSnapshot:
    | { hasSkuSelection: boolean; groups: DetailSpecGroup[]; rows: DetailVariantRow[] }
    | null = null;
  let selectionStateBefore: DetailSelectionSnapshot | null = null;
  let selectionStateAfter: DetailSelectionSnapshot | null = null;
  let quantitySnapshotBefore: DetailQuantitySnapshot | null = null;
  let quantitySnapshotAfter: DetailQuantitySnapshot | null = null;
  let structuredDataProbe: DetailStructuredDataProbe | null = null;
  try {
    page.on("response", async (response) => {
      try {
        const request = response.request();
        const resourceType = request.resourceType();
        const url = response.url();
        if (!["xhr", "fetch"].includes(resourceType)) return;
        if (!/sku|price|offer|detail|cargo|freight|rate|order/i.test(url)) return;

        const headers = response.headers();
        const contentType =
          headers["content-type"] ||
          headers["Content-Type"] ||
          null;
        let bodySample: string | null = null;
        if (
          contentType &&
          /(json|javascript|text|html)/i.test(contentType)
        ) {
          try {
            bodySample = (await response.text()).replace(/\s+/g, " ").trim().slice(0, 500);
          } catch {
            bodySample = null;
          }
        }

        networkEvents.push({
          url,
          status: response.status(),
          method: request.method(),
          resourceType,
          contentType,
          bodySample,
        });
      } catch {
        // Ignore diagnostics-only probe failures.
      }
    });

    await page.goto(itemUrl, {
      waitUntil: "domcontentloaded",
      timeout: 60_000,
    });
    await page.waitForSelector("body", { timeout: 15_000 });
    await delay(800);

    const snapshot = await readDetailVariantRows(page);
    skuSelectionSnapshot = snapshot;
    hasSkuSelection = snapshot.hasSkuSelection;
    variantRowCount = snapshot.rows.length;
    pageScreenshotBase64 = await capturePageScreenshotBase64(page);
    if (hasSkuSelection) {
      skuSelectionScreenshotBase64 = await captureElementScreenshotBase64(page, "#skuSelection");
      selectionStateBefore = await readDetailSelectionSnapshot(page);
    }
    structuredDataProbe = await collectDetailStructuredDataProbe(page, networkEvents);
    const selectionPlan = resolveDetailPricingSelectionPlan({
      hasSkuSelection: snapshot.hasSkuSelection,
      groups: snapshot.groups,
      rows: snapshot.rows,
      matchedImageUrl,
      ozonTitle,
      ozonSpecProfile,
    });

    if (selectionPlan.resolutionMode === "legacy_no_sku_selection") {
      return {
        resolutionMode: selectionPlan.resolutionMode,
        price: cardPrice || null,
        matchedVariantLabel: null,
        basePrice: null,
        freightPrice: null,
        quantityPlusClicked: false,
        submitOrderText: null,
        diagnostics: {
          failureCode: null,
          priceSource: "card_price_fallback",
          priceSourceRefreshed: null,
          hasSkuSelection,
          variantRowCount,
          selectedRowIndex: null,
          selectionAttempted: false,
          selectionApplied: false,
          quantityBefore: null,
          quantityAfter: null,
          submitOrderBeforeText: null,
          submitOrderAfterText: null,
          pageScreenshotBase64,
          skuSelectionScreenshotBase64,
          skuSelectionSnapshot,
          selectionStateBefore,
          selectionStateAfter,
          quantitySnapshotBefore,
          quantitySnapshotAfter,
          structuredDataProbe,
        },
      };
    }

    if (
      selectionPlan.resolutionMode === "manual_review_required_unknown_spec" ||
      selectionPlan.options.length === 0
    ) {
      return {
        resolutionMode: "manual_review_required_unknown_spec",
        price: null,
        matchedVariantLabel: null,
        basePrice: null,
        freightPrice: null,
        quantityPlusClicked: false,
        submitOrderText: null,
        diagnostics: {
          failureCode: "manual_review_required_unknown_spec",
          priceSource: null,
          priceSourceRefreshed: null,
          hasSkuSelection,
          variantRowCount,
          selectedRowIndex: null,
          selectionAttempted: false,
          selectionApplied: false,
          quantityBefore: null,
          quantityAfter: null,
          submitOrderBeforeText: null,
          submitOrderAfterText: null,
          pageScreenshotBase64,
          skuSelectionScreenshotBase64,
          skuSelectionSnapshot,
          selectionStateBefore,
          selectionStateAfter,
          quantitySnapshotBefore,
          quantitySnapshotAfter,
          structuredDataProbe,
        },
      };
    }

    selectedVariantLabel = selectionPlan.matchedVariantLabel;
    selectedRowIndex = selectionPlan.options[0]?.rowIndex ?? null;
    selectionAttempted = true;
    submitOrderBeforeText = await page.evaluate(() => {
      const submitOrder = document.getElementById("submitOrder");
      return submitOrder?.textContent?.trim() || null;
    });

    const inlineRowQuantityBefore =
      selectedRowIndex !== null
        ? await readDetailRowQuantityControlSnapshot(page, selectedRowIndex)
        : null;

    if (hasInlineRowQuantityControls(inlineRowQuantityBefore) && selectedRowIndex !== null) {
      selectionApplied = true;
      quantityBefore = inlineRowQuantityBefore?.quantityText || null;
      quantitySnapshotBefore = await readDetailQuantitySnapshot(page);
      await clickInlineRowQuantityPlusButton(page, selectedRowIndex);
      quantityPlusClicked = true;
      await delay(300);
      const inlineRowQuantityAfter = await readDetailRowQuantityControlSnapshot(page, selectedRowIndex);
      quantityAfter = inlineRowQuantityAfter?.quantityText || null;
      quantitySnapshotAfter = await readDetailQuantitySnapshot(page);
      if (!didInlineRowQuantityIncrement(inlineRowQuantityBefore, inlineRowQuantityAfter)) {
        throw new Error("[DETAIL_QUANTITY_NOT_APPLIED]");
      }
    } else {
      for (const option of selectionPlan.options) {
        await selectVariantRow(page, option.rowIndex);
        await delay(120);
      }
      submitOrderAfterText = await page.evaluate(() => {
        const submitOrder = document.getElementById("submitOrder");
        return submitOrder?.textContent?.trim() || null;
      });
      selectionStateAfter = await readDetailSelectionSnapshot(page);
      selectionApplied = didSelectionPlanApplyOrRefreshPriceSource(
        selectionPlan.options,
        selectionStateAfter,
        submitOrderBeforeText,
        submitOrderAfterText,
      );
      if (!selectionApplied) {
        throw new Error("[DETAIL_VARIANT_SELECTION_NOT_APPLIED]");
      }
      await delay(300);
      quantityBefore = await page.evaluate(() => {
        const submitOrder = document.getElementById("submitOrder");
        return submitOrder?.textContent?.trim() || null;
      });
      quantitySnapshotBefore = await readDetailQuantitySnapshot(page);
      await clickQuantityPlusButton(page);
      quantityPlusClicked = true;
    }

    await waitForSubmitOrderTotals(page);
    const totals = await readSubmitOrderTotals(page);
    submitOrderText = totals.submitOrderText;
    submitOrderAfterText = totals.submitOrderText;
    if (!quantityAfter) {
      quantityAfter = totals.submitOrderText;
    }
    if (!quantitySnapshotAfter) {
      quantitySnapshotAfter = await readDetailQuantitySnapshot(page);
    }
    if (!didQuantityIncrementApply(quantitySnapshotBefore, quantitySnapshotAfter)) {
      throw new Error("[DETAIL_QUANTITY_NOT_APPLIED]");
    }
    if (!didPriceSourceRefresh(submitOrderBeforeText, submitOrderAfterText)) {
      throw new Error("[DETAIL_PRICE_NOT_REFRESHED]");
    }
    basePriceText = totals.basePrice;
    freightPriceText = totals.freightPrice;

    if (!totals.totalPrice) {
      throw new Error("[DETAIL_PRICE_TOTAL_UNAVAILABLE]");
    }

    return {
      resolutionMode: selectionPlan.resolutionMode,
      price: totals.totalPrice,
      matchedVariantLabel: selectionPlan.matchedVariantLabel,
      basePrice: totals.basePrice,
      freightPrice: totals.freightPrice,
      quantityPlusClicked,
      submitOrderText,
      diagnostics: {
        failureCode: null,
        priceSource: "submit_order_text",
        priceSourceRefreshed: didPriceSourceRefresh(
          submitOrderBeforeText,
          submitOrderAfterText,
        ),
        hasSkuSelection,
        variantRowCount,
        selectedRowIndex,
        selectionAttempted,
        selectionApplied,
        quantityBefore,
        quantityAfter,
        submitOrderBeforeText,
        submitOrderAfterText,
        pageScreenshotBase64,
        skuSelectionScreenshotBase64,
        skuSelectionSnapshot,
        selectionStateBefore,
        selectionStateAfter,
        quantitySnapshotBefore,
        quantitySnapshotAfter,
        structuredDataProbe,
      },
    };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const diagnostic: DetailPricingDiagnostics = {
      failureCode: deriveDetailPricingFailureCode(message),
      priceSource: submitOrderText ? "submit_order_text" : null,
      priceSourceRefreshed: didPriceSourceRefresh(
        submitOrderBeforeText,
        submitOrderAfterText,
      ),
      hasSkuSelection,
      variantRowCount,
      selectedRowIndex,
      selectionAttempted,
      selectionApplied,
      quantityBefore,
      quantityAfter,
      submitOrderBeforeText,
      submitOrderAfterText,
      pageScreenshotBase64,
      skuSelectionScreenshotBase64,
      skuSelectionSnapshot,
      selectionStateBefore,
      selectionStateAfter,
      quantitySnapshotBefore,
      quantitySnapshotAfter,
      structuredDataProbe,
    };
    const legacyDiagnostic = {
      matchedVariantLabel: selectedVariantLabel,
      quantityPlusClicked,
      basePriceText,
      freightPriceText,
      submitOrderText,
    };
    throw new Error(
      `${message} diag=${JSON.stringify({ ...diagnostic, ...legacyDiagnostic })}`,
    );
  } finally {
    await page.close().catch(() => undefined);
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function normalizeCursor(cursor: string): string {
  return (cursor || "").trim().toLowerCase();
}

function isMoveCursor(cursor: string): boolean {
  const value = normalizeCursor(cursor);
  return value === "move" || value.includes("grab") || value.includes("grabbing");
}

function isResizeCursor(cursor: string): boolean {
  const value = normalizeCursor(cursor);
  return value.includes("resize") || value.includes("nwse") || value.includes("nesw") || value.includes("ew-resize") || value.includes("ns-resize");
}

function matchesCursorMode(cursor: string, mode: CursorProbeMode): boolean {
  return mode === "move" ? isMoveCursor(cursor) : isResizeCursor(cursor);
}

export function pickBestCursorProbePoint(
  probes: CursorProbePoint[],
  mode: CursorProbeMode,
  canvasRect: RectBox,
): CropPoint | null {
  const candidates = probes.filter((probe) => mode === "move" ? isMoveCursor(probe.cursor) : isResizeCursor(probe.cursor));
  if (candidates.length === 0) return null;

  if (mode === "move") {
    const center = {
      x: canvasRect.left + canvasRect.width / 2,
      y: canvasRect.top + canvasRect.height / 2,
    };
    const best = candidates.reduce((best, point) => {
      const currentDistance = Math.hypot(point.x - center.x, point.y - center.y);
      const bestDistance = Math.hypot(best.x - center.x, best.y - center.y);
      return currentDistance < bestDistance ? point : best;
    });
    return { x: best.x, y: best.y };
  }

  const best = candidates.reduce((best, point) => {
    const currentScore = point.x + point.y;
    const bestScore = best.x + best.y;
    return currentScore > bestScore ? point : best;
  });
  return { x: best.x, y: best.y };
}

export function deriveCursorBounds(probes: CursorProbePoint[], mode: CursorProbeMode): RectBox | null {
  const candidates = probes.filter((probe) => matchesCursorMode(probe.cursor, mode));
  if (candidates.length === 0) return null;

  let left = candidates[0].x;
  let top = candidates[0].y;
  let right = candidates[0].x;
  let bottom = candidates[0].y;

  for (const point of candidates) {
    if (point.x < left) left = point.x;
    if (point.y < top) top = point.y;
    if (point.x > right) right = point.x;
    if (point.y > bottom) bottom = point.y;
  }

  return {
    left,
    top,
    right,
    bottom,
    width: right - left,
    height: bottom - top,
  };
}

export function pickResizeStartFromBounds(
  moveBounds: RectBox,
  canvasRect: RectBox,
  offset: number = 4,
): CropPoint {
  return {
    x: clamp(moveBounds.right + offset, canvasRect.left + CROP_EDGE_PADDING, canvasRect.right - CROP_EDGE_PADDING),
    y: clamp(moveBounds.bottom + offset, canvasRect.top + CROP_EDGE_PADDING, canvasRect.bottom - CROP_EDGE_PADDING),
  };
}

export function isLikelyCropCanvasRect(canvasRect: RectBox): boolean {
  if (canvasRect.width < 220 || canvasRect.height < 220) return false;
  const ratio = canvasRect.width / canvasRect.height;
  return ratio > 0.75 && ratio < 1.35;
}

export function evaluateResizeCoverage(
  beforeResizeBounds: RectBox | null,
  afterResizeBounds: RectBox | null,
  canvasRect: RectBox,
): {
  ok: boolean;
  reason: string;
  metrics: {
    growthX: number;
    growthY: number;
    coverageX: number;
    coverageY: number;
    rightGap: number;
    bottomGap: number;
  };
} {
  if (!beforeResizeBounds || !afterResizeBounds) {
    return {
      ok: false,
      reason: "missing-bounds",
      metrics: { growthX: 0, growthY: 0, coverageX: 0, coverageY: 0, rightGap: -1, bottomGap: -1 },
    };
  }

  const growthX = afterResizeBounds.width / Math.max(beforeResizeBounds.width, 1);
  const growthY = afterResizeBounds.height / Math.max(beforeResizeBounds.height, 1);
  const coverageX = afterResizeBounds.width / Math.max(canvasRect.width, 1);
  const coverageY = afterResizeBounds.height / Math.max(canvasRect.height, 1);
  const rightGap = canvasRect.right - afterResizeBounds.right;
  const bottomGap = canvasRect.bottom - afterResizeBounds.bottom;

  const failed: string[] = [];
  if (coverageX < 0.78) failed.push("coverageX");
  if (coverageY < 0.78) failed.push("coverageY");
  if (rightGap > 28) failed.push("rightGap");
  if (bottomGap > 28) failed.push("bottomGap");

  return {
    ok: failed.length === 0,
    reason: failed.length === 0 ? "ok" : failed.join("+"),
    metrics: { growthX, growthY, coverageX, coverageY, rightGap, bottomGap },
  };
}

export function evaluateExistingCropCoverage(
  selectionRect: RectBox | null,
  canvasRect: RectBox,
): CropCoverageState {
  if (!selectionRect) {
    return "unknown";
  }

  const coverageX = selectionRect.width / Math.max(canvasRect.width, 1);
  const coverageY = selectionRect.height / Math.max(canvasRect.height, 1);
  const leftGap = selectionRect.left - canvasRect.left;
  const topGap = selectionRect.top - canvasRect.top;
  const rightGap = canvasRect.right - selectionRect.right;
  const bottomGap = canvasRect.bottom - selectionRect.bottom;

  const edgeTolerance = 28;
  const coverageTolerance = 0.94;
  if (
    coverageX >= coverageTolerance &&
    coverageY >= coverageTolerance &&
    leftGap <= edgeTolerance &&
    topGap <= edgeTolerance &&
    rightGap <= edgeTolerance &&
    bottomGap <= edgeTolerance
  ) {
    return "full";
  }

  return "partial";
}

export function buildResizeHandleCandidates(
  moveBounds: RectBox,
  canvasRect: RectBox,
  span: number = 8,
  step: number = 4,
): CropPoint[] {
  const points: CropPoint[] = [];
  const seen = new Set<string>();
  const clampIntoCanvas = (point: CropPoint): CropPoint => ({
    x: clamp(point.x, canvasRect.left + CROP_EDGE_PADDING, canvasRect.right - CROP_EDGE_PADDING),
    y: clamp(point.y, canvasRect.top + CROP_EDGE_PADDING, canvasRect.bottom - CROP_EDGE_PADDING),
  });

  const push = (point: CropPoint): void => {
    const clamped = clampIntoCanvas(point);
    const key = `${clamped.x}:${clamped.y}`;
    if (seen.has(key)) return;
    seen.add(key);
    points.push(clamped);
  };

  // Always try the geometric bottom-right corner first.
  push({ x: Math.round(moveBounds.right), y: Math.round(moveBounds.bottom) });

  for (let dx = -span; dx <= span; dx += step) {
    for (let dy = -span; dy <= span; dy += step) {
      if (dx === 0 && dy === 0) continue;
      push({
        x: Math.round(moveBounds.right + dx),
        y: Math.round(moveBounds.bottom + dy),
      });
    }
  }

  return points;
}

function clampPointInCanvas(point: CropPoint, canvasRect: RectBox, edgePadding: number): CropPoint {
  return {
    x: clamp(point.x, canvasRect.left + edgePadding, canvasRect.right - edgePadding),
    y: clamp(point.y, canvasRect.top + edgePadding, canvasRect.bottom - edgePadding),
  };
}

export function buildFullCanvasCropPlan(
  selectionRect: RectBox,
  canvasRect: RectBox,
  edgePadding: number = CROP_EDGE_PADDING,
): FullCanvasCropPlan {
  const centerOffsetX = selectionRect.width / 2;
  const centerOffsetY = selectionRect.height / 2;
  const moveStart = clampPointInCanvas(
    {
      x: selectionRect.left + centerOffsetX,
      y: selectionRect.top + centerOffsetY,
    },
    canvasRect,
    edgePadding,
  );

  const moveEnd = {
    x: canvasRect.left + edgePadding + centerOffsetX,
    y: canvasRect.top + edgePadding + centerOffsetY,
  };

  const resizeStart = clampPointInCanvas(
    {
      x: selectionRect.right - 4,
      y: selectionRect.bottom - 4,
    },
    canvasRect,
    edgePadding,
  );

  const resizeEnd = {
    x: canvasRect.right - edgePadding,
    y: canvasRect.bottom - edgePadding,
  };

  return { moveStart, moveEnd, resizeStart, resizeEnd };
}

export function limitSearchResults(results: SearchResult[], limit: number = DEFAULT_RESULT_LIMIT): SearchResult[] {
  return results.slice(0, limit);
}

export function shouldStopResultScroll(state: ResultScrollState): boolean {
  return (
    state.visibleResultCount >= state.targetResultCount ||
    state.reachedBottom ||
    state.totalScrolled >= state.maxScrollDistance
  );
}

export function shouldNavigateTo1688Home(currentUrl: string): boolean {
  const url = (currentUrl || "").trim().toLowerCase();
  if (!url.startsWith("https://www.1688.com/")) {
    return true;
  }

  try {
    const parsed = new URL(url);
    return parsed.pathname !== "/";
  } catch {
    return true;
  }
}

export function shouldEnsureHomePageBeforeSessionCheck(currentUrl: string): boolean {
  const url = (currentUrl || "").trim().toLowerCase();
  if (!url.startsWith("http")) {
    return true;
  }
  if (shouldNavigateTo1688Home(url)) {
    if (
      url.includes("login.1688.com") ||
      url.includes("member/signin") ||
      url.includes("passport.alibaba.com") ||
      url.includes("sec.") ||
      url.includes("punish") ||
      url.includes("captcha") ||
      url.includes("verify")
    ) {
      return false;
    }
    return true;
  }
  return false;
}

export function isLikelySearchResultsUrl(currentUrl: string): boolean {
  const url = (currentUrl || "").trim().toLowerCase();
  if (!url.startsWith("http")) {
    return false;
  }

  return RESULT_URL_HINTS.some((hint) => url.includes(hint));
}

export function shouldKeepWaitingForSearchConfirm(
  currentUrl: string,
  hasVisibleResults: boolean,
): boolean {
  if (hasVisibleResults) {
    return false;
  }

  return !isLikelySearchResultsUrl(currentUrl);
}

function shouldAutoCloseTab(currentUrl: string): boolean {
  const url = (currentUrl || "").trim().toLowerCase();
  return url === "about:blank" || url.includes("1688.com");
}

export function selectClosableTabs<T extends ClosableTabLike>(
  pages: T[],
  keepPages: T[],
): T[] {
  const keepSet = new Set(keepPages);
  return pages.filter((page) => {
    if (keepSet.has(page)) return false;
    if (page.isClosed?.()) return false;
    return shouldAutoCloseTab(page.url());
  });
}

async function cleanupAutomationTabs(browser: Pick<Browser, "pages">, keepPages: Page[]): Promise<void> {
  const pages = await browser.pages();
  const closableTabs = selectClosableTabs(pages, keepPages);
  for (const tab of closableTabs) {
    await tab.close().catch(() => undefined);
  }
}

export function assemblePriceFromFragments(
  majorFragment: string,
  minorFragment: string,
  fallbackText: string,
): string {
  const normalize = (value: string): string => (value || "").replace(/\s+/g, "");
  const extractNumeric = (value: string): string => {
    const cleaned = normalize(value).replace(/[^\d.]/g, "");
    const matched = cleaned.match(/\d+(?:\.\d+)?/);
    return matched ? matched[0] : "";
  };

  const merged = extractNumeric(`${normalize(majorFragment)}${normalize(minorFragment)}`);
  if (merged) return merged;
  return extractNumeric(fallbackText);
}

export function extractSalesText(cardText: string): string {
  const normalized = (cardText || "").replace(/\s+/g, " ").trim();
  if (!normalized) return "";

  const patterns = [
    /(?:月销|销量|累计成交|成交|已售|售出)\s*[:：]?\s*([0-9]+(?:\.[0-9]+)?(?:万|千)?\+?)/i,
    /([0-9]+(?:\.[0-9]+)?(?:万|千)?\+?)\s*(?:人付款|人已付款|笔成交|件成交|笔已售|件已售)/i,
  ];

  for (const pattern of patterns) {
    const matched = normalized.match(pattern);
    const value = matched?.[1]?.trim();
    if (value) return value;
  }

  return "";
}

export type ResultPageRecallOptions = {
  forceFullCrop: boolean;
  inspectCropCoverage: () => Promise<CropCoverageState>;
  scrapeCurrentPage: () => Promise<SearchResult[]>;
  applyFullCanvasCrop: () => Promise<void>;
};

export async function executeResultPageRecall(
  options: ResultPageRecallOptions,
): Promise<SearchRecallResult> {
  let shouldApplyFullCrop = options.forceFullCrop;

  if (!shouldApplyFullCrop) {
    console.log("👀 [第一重检索] 采用 Ozon 源图执行 1688 首次搜索...");
    const coverage = await options.inspectCropCoverage();
    shouldApplyFullCrop = coverage !== "full";

    if (!shouldApplyFullCrop) {
      return {
        results: await options.scrapeCurrentPage(),
        usedSecondPassFullCrop: false,
      };
    }

    console.log(`📐 [第二重检索] 当前裁剪覆盖状态=${coverage}，执行整图纠偏重搜...`);
  }

  console.log("📐 [第二重爆破] 启动机械臂拖动 Canvas 拉满全图...");

  try {
    await options.applyFullCanvasCrop();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error("❌ 强制重绘操作受阻，1688 页面可能未响应:", error);
    if (message.includes("[FULL_CROP_NOT_APPLIED]")) {
      throw error;
    }
    throw new Error(`[FULL_CROP_NOT_APPLIED] ${message}`);
  }

  return {
    results: await options.scrapeCurrentPage(),
    usedSecondPassFullCrop: true,
  };
}

export async function openCropDialogAndWaitForCanvas(
  resultPage: Pick<Page, "waitForFunction" | "evaluate" | "waitForSelector">,
): Promise<void> {
  console.log("⏳ 等待裁剪面板出现...");
  await resultPage.waitForFunction(() => {
    const cut1 = document.querySelector(".cut-btn");
    const cut2 = document.querySelector('div[class*="cutBtn"]');
    return cut1 !== null || cut2 !== null;
  }, { timeout: 15_000 });

  await resultPage.evaluate(() => {
    const cutBtn =
      document.querySelector(".cut-btn") ||
      document.querySelector('div[class*="cutBtn"]');
    if (cutBtn) {
      cutBtn.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
      cutBtn.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
      cutBtn.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
      cutBtn.click();
    }
  });

  await resultPage.waitForSelector("#croper-canvas", {
    visible: true,
    timeout: 20_000,
  });
  await new Promise((resolve) => setTimeout(resolve, 1_500));
}

export async function waitForSearchResults(
  resultPage: SearchResultsPageLike,
): Promise<boolean> {
  const selector = 'div[class*="searchOfferWrapper"]';
  const cardsReady = await resultPage
    .waitForSelector(selector, { timeout: RESULT_SELECTOR_TIMEOUT_MS })
    .then(() => true)
    .catch(() => false);

  if (cardsReady) {
    return true;
  }

  await resultPage.waitForNetworkIdle({ timeout: RESULT_IDLE_TIMEOUT_MS }).catch(() => {});
  return resultPage
    .waitForSelector(selector, { timeout: RESULT_SECONDARY_SELECTOR_TIMEOUT_MS })
    .then(() => true)
    .catch(() => false);
}

async function clickSearchConfirmIfPresent(
  page: Pick<Page, "evaluate">,
): Promise<boolean> {
  return page.evaluate(
    async ({ labels, maxAttempts, pollIntervalMs, resultUrlHints, resultSelector }) => {
      const normalizedLabels = labels.map((label) => label.replace(/\s+/g, ""));
      const isVisible = (element: HTMLElement): boolean => {
        const style = window.getComputedStyle(element);
        const rect = element.getBoundingClientRect();
        return (
          style.display !== "none" &&
          style.visibility !== "hidden" &&
          style.pointerEvents !== "none" &&
          rect.width > 0 &&
          rect.height > 0
        );
      };

      const clickLikeUser = (element: HTMLElement) => {
        element.scrollIntoView({ block: "center", inline: "center" });
        const events = [
          "pointerover",
          "mouseover",
          "pointerenter",
          "mouseenter",
          "pointerdown",
          "mousedown",
          "pointerup",
          "mouseup",
          "click",
        ];
        for (const eventName of events) {
          element.dispatchEvent(new MouseEvent(eventName, { bubbles: true, cancelable: true, composed: true }));
        }
        element.click();
      };

      return new Promise<boolean>((resolve) => {
        let attempts = 0;
        const timer = setInterval(() => {
          attempts += 1;
          const currentUrl = (window.location.href || "").trim().toLowerCase();
          const hasVisibleResults = document.querySelector(resultSelector) !== null;
          const alreadyEnteredResults =
            hasVisibleResults ||
            resultUrlHints.some((hint) => currentUrl.includes(hint));

          if (alreadyEnteredResults) {
            clearInterval(timer);
            resolve(false);
            return;
          }

          const candidates = Array.from(
            document.querySelectorAll<HTMLElement>("button, a, div, span"),
          ).filter((element) => {
            const text = (element.innerText || element.textContent || "").replace(/\s+/g, "");
            return normalizedLabels.includes(text) && isVisible(element);
          });

          if (candidates.length > 0) {
            clearInterval(timer);
            clickLikeUser(candidates[0]);
            resolve(true);
            return;
          }

          if (attempts >= maxAttempts) {
            clearInterval(timer);
            resolve(false);
          }
        }, pollIntervalMs);
      });
    },
    {
      labels: SEARCH_CONFIRM_LABELS,
      maxAttempts: SEARCH_CONFIRM_MAX_ATTEMPTS,
      pollIntervalMs: SEARCH_CONFIRM_POLL_INTERVAL_MS,
      resultUrlHints: RESULT_URL_HINTS,
      resultSelector: 'div[class*="searchOfferWrapper"]',
    },
  );
}

async function resolveResultPageAfterUpload(
  browser: Browser,
  sourcePage: Page,
  newTargetPromise: Promise<import("puppeteer").Target | null>,
  timeoutMs: number = RESULT_ENTRY_TIMEOUT_MS,
): Promise<Page | null> {
  let promisedPage: Page | null = null;
  void newTargetPromise.then(async (target) => {
    promisedPage = target ? await target.page().catch(() => null) : null;
  });

  const startedAt = Date.now();
  while (Date.now() - startedAt < timeoutMs) {
    if (promisedPage && isLikelySearchResultsUrl(promisedPage.url())) {
      return promisedPage;
    }

    if (isLikelySearchResultsUrl(sourcePage.url())) {
      return sourcePage;
    }

    const currentPageHasCards =
      (await sourcePage.$('div[class*="searchOfferWrapper"]')) !== null;
    if (currentPageHasCards) {
      return sourcePage;
    }

    const allPages = await browser.pages();
    const latestResultPage = [...allPages]
      .reverse()
      .find((candidate) => isLikelySearchResultsUrl(candidate.url()));
    if (latestResultPage) {
      return latestResultPage;
    }

    await delay(RESULT_ENTRY_POLL_INTERVAL_MS);
  }

  return null;
}

export async function search1688ByImage(
  browser: Browser,
  page: Page,
  imagePath: string,
  forceFullCrop: boolean = false, 
  targetKeywords: string[] = [],
): Promise<SearchRecallResult> {
  const CAMERA_ICON_SELECTOR = ".image-file-reader-wrapper";
  const absoluteImgPath = path.resolve(imagePath);
  let resultPage: Page | null = null;

  const readSearchResultSnapshot = async (): Promise<SearchResultSnapshot> => {
    return resultPage!.evaluate(({ keywords, bottomGapPx }) => {
      const cards = Array.from(document.querySelectorAll('div[class*="searchOfferWrapper"]'));
      const parsedItems = cards.map((card) => {
        const titleEl = card.querySelector('div[class*="titleText"]');
        const title = titleEl ? titleEl.innerText.trim() : "";
        const cardText = (card.textContent || "").replace(/\s+/g, " ").trim();
        const priceContainer = card.querySelector('div[class*="priceItem"]');
        let priceMajor = "";
        let priceMinor = "";
        if (priceContainer instanceof HTMLElement) {
          const priceNodes = Array.from(priceContainer.querySelectorAll(":scope > div"));
          const yuanIndex = priceNodes.findIndex((node) => (node.textContent || "").replace(/\s+/g, "") === "¥");
          if (yuanIndex >= 0) {
            priceMajor = (priceNodes[yuanIndex + 1]?.textContent || "").trim();
            priceMinor = (priceNodes[yuanIndex + 2]?.textContent || "").trim();
          }
        }

        const legacyPriceEl = card.querySelector('div[class*="textMain"]');
        const legacyPriceText = legacyPriceEl ? legacyPriceEl.textContent?.trim() || "" : "";
        const shopEl = card.querySelector('div[class*="shopName"]');
        const shopName = shopEl ? shopEl.innerText.trim() : "";
        const imgEl = card.querySelector('img[class*="mainImg"]');
        const imageUrl = imgEl ? imgEl.src || imgEl.getAttribute("data-src") : "";
        const reportData = card.getAttribute("data-aplus-report") || card.getAttribute("data-tracker") || "";
        const isAd = reportData.includes("offerType:e_p4p") || reportData.includes("offerType:p4p");

        let cosScore = 0;
        const scoreMatch = reportData.match(/cosScore.*?([\d\.]+)/i);
        if (scoreMatch && scoreMatch[1]) cosScore = parseFloat(scoreMatch[1]);

        let itemUrl = "";
        const wwEl = card.querySelector(".J_WangWang");
        if (wwEl) {
          try {
            const extra = JSON.parse(wwEl.getAttribute("data-extra") || "{}");
            if (extra.offerId) itemUrl = `https://detail.1688.com/offer/${extra.offerId}.html`;
          } catch (e) {}
        }
        if (!itemUrl) {
          const match = reportData.match(/object_id@(\d+)/);
          if (match && match[1]) itemUrl = `https://detail.1688.com/offer/${match[1]}.html`;
        }
        return { title, priceMajor, priceMinor, legacyPriceText, cardText, sales: "", moq: "", shopName, itemUrl, imageUrl, isAd, cosScore };
      });

      const isScoreValid = parsedItems.filter((item) => item.cosScore > 0).length > 0;
      const records = parsedItems.filter((item) => {
        if (!item.title || !item.itemUrl || item.isAd) return false;
        if (isScoreValid && item.cosScore < 0.3) return false;
        if (keywords && keywords.length > 0) {
          const isMatchKeyword = keywords.some((kw) => item.title.includes(kw));
          if (!isMatchKeyword) return false;
        }
        return true;
      });
      const reachedBottom =
        Math.ceil(window.scrollY + window.innerHeight) >=
        Math.ceil(document.body.scrollHeight) - bottomGapPx;
      return { records, reachedBottom };
    }, { keywords: targetKeywords, bottomGapPx: RESULT_BOTTOM_GAP_PX });
  };

  const scrollResultPageStep = async (): Promise<number> => {
    return resultPage!.evaluate(({ minDistance, maxDistance }) => {
      const distance =
        Math.floor(Math.random() * (maxDistance - minDistance + 1)) + minDistance;
      window.scrollBy(0, distance);
      return distance;
    }, {
      minDistance: RESULT_SCROLL_DISTANCE_MIN,
      maxDistance: RESULT_SCROLL_DISTANCE_MAX,
    });
  };

  const scrapeCurrentPage = async (): Promise<SearchResult[]> => {
    let snapshot = await readSearchResultSnapshot();
    let totalScrolled = 0;

    while (
      !shouldStopResultScroll({
        visibleResultCount: snapshot.records.length,
        targetResultCount: DEFAULT_RESULT_LIMIT,
        reachedBottom: snapshot.reachedBottom,
        totalScrolled,
        maxScrollDistance: RESULT_SCROLL_MAX_DISTANCE,
      })
    ) {
      totalScrolled += await scrollResultPageStep();
      await new Promise((resolve) => setTimeout(resolve, RESULT_SCROLL_INTERVAL_MS));
      snapshot = await readSearchResultSnapshot();
    }

    await new Promise((resolve) => setTimeout(resolve, RESULT_POST_SCROLL_SETTLE_MS));
    snapshot = await readSearchResultSnapshot();

    const normalizedData: SearchResult[] = snapshot.records.map((item) => {
      const priceValue = assemblePriceFromFragments(item.priceMajor, item.priceMinor, item.legacyPriceText);
      return {
        title: item.title,
        price: priceValue ? `¥${priceValue}` : "暂无",
        sales: extractSalesText(item.cardText),
        moq: item.moq,
        shopName: item.shopName,
        itemUrl: item.itemUrl,
        imageUrl: item.imageUrl,
        isAd: item.isAd,
        cosScore: item.cosScore,
      };
    });

    // 保持页面原始排序并只取前 36 个结果回传 Rust
    return limitSearchResults(normalizedData);
  };

  try {
    await cleanupAutomationTabs(browser, [page]);

    // 阶段一：激活常驻主阵地，防止页面休眠
    await page.bringToFront();
    const needsHomeNavigation =
      shouldNavigateTo1688Home(page.url()) ||
      (await page.$(CAMERA_ICON_SELECTOR)) === null;
    if (needsHomeNavigation) {
      await page.goto("https://www.1688.com/", {
        waitUntil: "domcontentloaded",
        timeout: 60_000,
      });
    }

    const currentUrl = page.url();
    const hasSlider = (await page.$('.nc-container, #baxia-dialog-content, #nc_1_n1z, iframe[src*="punish"]')) !== null;
    if (currentUrl.includes("login") || currentUrl.includes("sec.") || hasSlider) {
      throw new Error("[ANTI_BOT_CHALLENGE] 触发 1688 底层拦截，请在浏览器中完成验证后重试");
    }

    const cameraHandle = await page.waitForSelector(CAMERA_ICON_SELECTOR, { visible: true, timeout: 30000 });
    await new Promise((r) => setTimeout(r, CAMERA_READY_SETTLE_MS));

    // 预埋标签页捕捉器
    const newTargetPromise = browser.waitForTarget((t) => t.type() === "page" && t.url().includes("1688.com") && t.url() !== page.url(), { timeout: 30000 }).catch(() => null);

    // 触发文件上传
    const [fileChooser] = await Promise.all([
      page.waitForFileChooser({ timeout: 15000 }),
      cameraHandle!.click().catch(async () => {
        await page.evaluate((sel) => document.querySelector(sel)?.click(), CAMERA_ICON_SELECTOR);
      }),
    ]);

    await fileChooser.accept([absoluteImgPath]);

    // 阶段二：先快速探测是否已自动进入结果页，只有未进入时才等待“搜索图片/开始搜索”确认按钮。
    resultPage = await resolveResultPageAfterUpload(
      browser,
      page,
      newTargetPromise,
      IMMEDIATE_RESULT_ENTRY_TIMEOUT_MS,
    );
    let searchConfirmed = false;

    if (!resultPage) {
      searchConfirmed = await clickSearchConfirmIfPresent(page);
      resultPage = await resolveResultPageAfterUpload(browser, page, newTargetPromise);
    }

    if (!resultPage && !searchConfirmed) {
      searchConfirmed = await clickSearchConfirmIfPresent(page);
      resultPage = await resolveResultPageAfterUpload(
        browser,
        page,
        newTargetPromise,
        IMMEDIATE_RESULT_ENTRY_TIMEOUT_MS,
      );
    }

    if (!resultPage) throw new Error("[IMAGE_SEARCH_NOT_ENTERED_RESULT_PAGE] 未能成功进入搜索结果页");
    await resultPage.bringToFront();
    const resultsReady = await waitForSearchResults(resultPage);
    if (!resultsReady && !isLikelySearchResultsUrl(resultPage.url())) {
      throw new Error("[IMAGE_SEARCH_NOT_ENTERED_RESULT_PAGE] 未能成功进入搜索结果页");
    }

    const dragMouse = async (start: CropPoint, end: CropPoint, steps: number = 28): Promise<void> => {
      await resultPage!.mouse.move(start.x, start.y);
      await resultPage!.mouse.down();
      await resultPage!.mouse.move(end.x, end.y, { steps });
      await new Promise((r) => setTimeout(r, 200));
      await resultPage!.mouse.up();
    };

    const getCroperCanvasRect = async (): Promise<RectBox | null> => {
      const canvasHandle = await resultPage!.$("#croper-canvas");
      if (!canvasHandle) return null;
      const box = await canvasHandle.boundingBox();
      if (!box || box.width < 50 || box.height < 50) return null;
      const rect = {
        left: box.x,
        top: box.y,
        right: box.x + box.width,
        bottom: box.y + box.height,
        width: box.width,
        height: box.height,
      };
      return isLikelyCropCanvasRect(rect) ? rect : null;
    };

    const readCursorAtPoint = async (point: CropPoint): Promise<string> => {
      return await resultPage!.evaluate((payload) => {
        const { x, y } = payload;
        const canvas = document.querySelector("#croper-canvas");
        const hit = document.elementFromPoint(x, y);
        const picks: string[] = [];

        if (hit instanceof HTMLElement) {
          picks.push(window.getComputedStyle(hit).cursor || hit.style.cursor || "");
        }
        if (canvas instanceof HTMLElement) {
          picks.push(window.getComputedStyle(canvas).cursor || canvas.style.cursor || "");
        }
        if (document.body instanceof HTMLElement) {
          picks.push(window.getComputedStyle(document.body).cursor || document.body.style.cursor || "");
        }

        const normalized = picks.map((cursor) => (cursor || "").toLowerCase().trim());
        const preferred = normalized.find((cursor) => cursor.length > 0 && cursor !== "default" && cursor !== "auto");
        return preferred || normalized[0] || "";
      }, point);
    };

    const scanCanvasCursorPoints = async (
      canvasRect: RectBox,
      rows: number,
      cols: number,
      edgePadding: number = 8,
    ): Promise<CursorProbePoint[]> => {
      const points: CursorProbePoint[] = [];
      const minX = canvasRect.left + edgePadding;
      const maxX = canvasRect.right - edgePadding;
      const minY = canvasRect.top + edgePadding;
      const maxY = canvasRect.bottom - edgePadding;

      for (let r = 0; r < rows; r++) {
        for (let c = 0; c < cols; c++) {
          const x = cols === 1 ? (minX + maxX) / 2 : minX + (maxX - minX) * (c / (cols - 1));
          const y = rows === 1 ? (minY + maxY) / 2 : minY + (maxY - minY) * (r / (rows - 1));
          const point = { x: Math.round(x), y: Math.round(y) };
          await resultPage!.mouse.move(point.x, point.y);
          await new Promise((rsv) => setTimeout(rsv, 12));
          const cursor = await readCursorAtPoint(point);
          points.push({ ...point, cursor });
        }
      }
      return points;
    };

    const findCursorProbe = async (
      mode: CursorProbeMode,
      canvasRect: RectBox,
      passes: Array<{ rows: number; cols: number; edgePadding: number }> = [
        { rows: 7, cols: 7, edgePadding: 10 },
        { rows: 13, cols: 13, edgePadding: 6 },
      ],
    ): Promise<{ point: CropPoint | null; probes: CursorProbePoint[] }> => {
      const allProbes: CursorProbePoint[] = [];
      for (const pass of passes) {
        const probes = await scanCanvasCursorPoints(canvasRect, pass.rows, pass.cols, pass.edgePadding);
        allProbes.push(...probes);
        const picked = pickBestCursorProbePoint(allProbes, mode, canvasRect);
        if (picked) {
          return { point: picked, probes: allProbes };
        }
      }
      return { point: null, probes: allProbes };
    };

    const moveBoundsProbePasses: Array<{ rows: number; cols: number; edgePadding: number }> = [
      { rows: 15, cols: 15, edgePadding: 4 },
      { rows: 21, cols: 21, edgePadding: 3 },
    ];

    const searchCursorAroundPoint = async (
      center: CropPoint,
      mode: CursorProbeMode,
      canvasRect: RectBox,
      radius: number = 30,
      step: number = 4,
    ): Promise<CropPoint | null> => {
      const points: CropPoint[] = [];
      for (let dx = -radius; dx <= radius; dx += step) {
        for (let dy = -radius; dy <= radius; dy += step) {
          points.push({
            x: clamp(Math.round(center.x + dx), canvasRect.left + CROP_EDGE_PADDING, canvasRect.right - CROP_EDGE_PADDING),
            y: clamp(Math.round(center.y + dy), canvasRect.top + CROP_EDGE_PADDING, canvasRect.bottom - CROP_EDGE_PADDING),
          });
        }
      }

      points.sort((a, b) => {
        const da = Math.hypot(a.x - center.x, a.y - center.y);
        const db = Math.hypot(b.x - center.x, b.y - center.y);
        return da - db;
      });

      for (const point of points) {
        await resultPage!.mouse.move(point.x, point.y);
        await new Promise((rsv) => setTimeout(rsv, 10));
        const cursor = await readCursorAtPoint(point);
        if (matchesCursorMode(cursor, mode)) return point;
      }

      return null;
    };

    const formatProbeSummary = (probes: CursorProbePoint[]): string => {
      return probes
        .filter((item) => item.cursor.length > 0)
        .slice(0, 24)
        .map((item) => `${item.cursor}@(${item.x},${item.y})`)
        .join(", ");
    };

    const ensureCropDialogReady = async (timeoutMs: number = 20000): Promise<RectBox> => {
      const startedAt = Date.now();
      while (Date.now() - startedAt < timeoutMs) {
        const rect = await getCroperCanvasRect();
        if (rect) {
          const isModalReady = await resultPage!.evaluate((payload) => {
            const canvas = document.querySelector("#croper-canvas");
            if (!(canvas instanceof HTMLElement)) return false;
            const centerX = Math.round(payload.left + payload.width / 2);
            const centerY = Math.round(payload.top + payload.height / 2);
            const hit = document.elementFromPoint(centerX, centerY);
            const hitOnCanvas = hit === canvas || (hit instanceof Node && canvas.contains(hit));
            const confirmBtn = Array.from(document.querySelectorAll("button, div, span"))
              .find((el) => (el.textContent || "").trim() === "确认");
            const cancelBtn = Array.from(document.querySelectorAll("button, div, span"))
              .find((el) => (el.textContent || "").trim() === "取消");
            return !!confirmBtn && !!cancelBtn && hitOnCanvas;
          }, rect);
          if (isModalReady) return rect;
        }
        await new Promise((r) => setTimeout(r, 250));
      }
      throw new Error("[FULL_CROP_NOT_APPLIED] 裁剪弹窗未进入可操作状态（可能误命中缩略图画布）");
    };

    const clickCropDialogAction = async (label: "确认" | "取消"): Promise<void> => {
      await resultPage!.evaluate((targetLabel) => {
        const visibleDialogs = Array.from(document.querySelectorAll('div[role="dialog"], div[class*="dialog"]'))
          .filter((node) => {
            if (!(node instanceof HTMLElement)) return false;
            const style = window.getComputedStyle(node);
            const rect = node.getBoundingClientRect();
            return style.display !== "none" && style.visibility !== "hidden" && rect.width > 50 && rect.height > 50;
          });
        const targetDialog = visibleDialogs.find((dialog) => dialog.querySelector("canvas")) || visibleDialogs[0];
        const scope = targetDialog || document;
        const action = Array.from(scope.querySelectorAll("button, div, span"))
          .find((el) => (el.textContent || "").trim() === targetLabel);
        if (action instanceof HTMLElement) {
          action.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
          action.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
          action.click();
        }
      }, label);
    };

    const inspectCropCoverage = async (): Promise<CropCoverageState> => {
      try {
        await openCropDialogAndWaitForCanvas(resultPage!);
        const canvasRect = await ensureCropDialogReady(20_000);
        const moveProbe = await findCursorProbe("move", canvasRect, moveBoundsProbePasses);
        const selectionRect = deriveCursorBounds(moveProbe.probes, "move");
        const coverage = evaluateExistingCropCoverage(selectionRect, canvasRect);
        await clickCropDialogAction("取消");
        return coverage;
      } catch (error) {
        console.warn("⚠️ 无法可靠判断当前裁剪覆盖范围，按 unknown 处理:", error);
        return "unknown";
      }
    };

    return await executeResultPageRecall({
      forceFullCrop,
      inspectCropCoverage,
      scrapeCurrentPage,
      applyFullCanvasCrop: async () => {
            await openCropDialogAndWaitForCanvas(resultPage!);
            const canvasRect = await ensureCropDialogReady(20000);

            const moveProbe = await findCursorProbe("move", canvasRect);
            if (!moveProbe.point) {
              const probeSummary = moveProbe.probes
                .filter((item) => item.cursor.length > 0)
                .slice(0, 20)
                .map((item) => `${item.cursor}@(${item.x},${item.y})`)
                .join(", ");
              throw new Error(`未探测到可拖拽 move 光标点，样本=${probeSummary || "none"}`);
            }

            const moveTarget = {
              x: canvasRect.left + 26,
              y: canvasRect.top + 26,
            };

            console.log("📐 步骤 1/2：探测 move 光标后，拖拽选框到左上角...");
            await dragMouse(moveProbe.point, moveTarget, 24);
            await new Promise((r) => setTimeout(r, 450));

            const beforeResizeProbe = await findCursorProbe("move", canvasRect, moveBoundsProbePasses);
            let moveBoundsBeforeResize = deriveCursorBounds(beforeResizeProbe.probes, "move");
            if (!moveBoundsBeforeResize) {
              throw new Error(`[FULL_CROP_NOT_APPLIED] 第一步移动后无法识别选框范围，样本=${formatProbeSummary(beforeResizeProbe.probes) || "none"}`);
            }
            console.log("🧭 move_bounds_before_resize:", moveBoundsBeforeResize);

            const resizeTarget = {
              x: canvasRect.right - CROP_EDGE_PADDING,
              y: canvasRect.bottom - CROP_EDGE_PADDING,
            };

            let coverage = evaluateResizeCoverage(moveBoundsBeforeResize, moveBoundsBeforeResize, canvasRect);
            let moveBoundsAfterResize: RectBox | null = null;

            for (let attempt = 1; attempt <= 3; attempt++) {
              const handleCandidates = buildResizeHandleCandidates(moveBoundsBeforeResize, canvasRect, 10, 3);
              let resizeStart: CropPoint | null = null;
              for (const candidate of handleCandidates) {
                const cursor = await readCursorAtPoint(candidate);
                if (isResizeCursor(cursor)) {
                  resizeStart = candidate;
                  break;
                }
              }

              if (!resizeStart) {
                const fallbackStart = pickResizeStartFromBounds(moveBoundsBeforeResize, canvasRect);
                resizeStart = await searchCursorAroundPoint(fallbackStart, "resize", canvasRect, 42, 3);
              }

              if (!resizeStart) {
                throw new Error(`[FULL_CROP_NOT_APPLIED] 第${attempt}次拉伸前未定位到 resize 控制点`);
              }

              console.log(`📐 步骤 2/2（第${attempt}次）：从`, resizeStart, "拉伸到", resizeTarget);
              await dragMouse(resizeStart, resizeTarget, attempt === 1 ? 30 : 26);
              await new Promise((r) => setTimeout(r, 380));

              const afterResizeMoveProbe = await findCursorProbe("move", canvasRect, moveBoundsProbePasses);
              moveBoundsAfterResize = deriveCursorBounds(afterResizeMoveProbe.probes, "move");
              console.log(`🧭 move_bounds_after_resize_attempt_${attempt}:`, moveBoundsAfterResize);

              coverage = evaluateResizeCoverage(moveBoundsBeforeResize, moveBoundsAfterResize, canvasRect);
              console.log(`🧭 coverage_ratio_attempt_${attempt}:`, coverage.metrics, "result:", coverage.reason);

              if (coverage.ok) break;

              // Coverage still fails: reset the box to top-left and retry a different handle start.
              if (attempt < 3) {
                const resetMoveProbe = await findCursorProbe("move", canvasRect, moveBoundsProbePasses);
                if (!resetMoveProbe.point) {
                  throw new Error(`[FULL_CROP_NOT_APPLIED] 第${attempt}次拉伸失败后无法重置选框，coverage=${coverage.reason}`);
                }
                await dragMouse(resetMoveProbe.point, moveTarget, 22);
                await new Promise((r) => setTimeout(r, 280));

                const resetBoundsProbe = await findCursorProbe("move", canvasRect, moveBoundsProbePasses);
                const resetBounds = deriveCursorBounds(resetBoundsProbe.probes, "move");
                if (!resetBounds) {
                  throw new Error(`[FULL_CROP_NOT_APPLIED] 第${attempt}次拉伸后重置失败，无法识别选框范围`);
                }
                moveBoundsBeforeResize = resetBounds;
                console.log(`🧭 move_bounds_before_resize_retry_${attempt}:`, moveBoundsBeforeResize);
              }
            }

            if (!coverage.ok) {
              throw new Error(`[FULL_CROP_NOT_APPLIED] 拉伸覆盖校验失败：${coverage.reason} metrics=${JSON.stringify(coverage.metrics)}`);
            }

            await clickCropDialogAction("确认");
            
            console.log("✅ 全图覆盖重绘完成！已提交，等待最新数据刷新...");
            await waitForSearchResults(resultPage);
      },
    });

  } catch (error) {
    console.error(`❌ 处理图片 ${imagePath} 发生异常:`, error);
    // 🌟 核心修改：绝对不吞没致命报错，将其透传回 server.ts 和 Rust！
    throw error; 
  } finally {
    // 阶段四：阅后即焚，关掉所有自动化遗留页，把干净的 1688 首页留给下一次搜索
    await cleanupAutomationTabs(browser, [page]);
  }
}
