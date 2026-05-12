import { describe, expect, test } from "bun:test";
import {
  appendRowResult,
  createEmptyMonitor,
  setBlockingAlert,
  setTaskPhase,
  markTaskDone,
  updateProgress,
} from "../../composables/useTaskEvents";

describe("Monitor helpers", () => {
  test("upserts incoming row result by row index", () => {
    const state = createEmptyMonitor();
    appendRowResult(state, {
      row_index: 2,
      sku: "SKU-2",
      stage: "searching_1688_source_image",
      status: "processing",
      recall_mode: null,
      image_url: null,
      original_image_url: "data:image/png;base64,source-preview",
      matched_image_url: null,
      item_url: null,
      price: null,
      elapsed_text: null,
      is_final: false,
    } as any);
    appendRowResult(state, {
      row_index: 2,
      sku: "SKU-2",
      stage: "completed",
      status: "processed",
      recall_mode: "source_first_pass",
      image_url: null,
      original_image_url: "data:image/png;base64,source-preview",
      matched_image_url: "https://img.1688.com/2.jpg",
      item_url: "https://detail.1688.com/offer/2.html",
      price: "¥5.20",
      elapsed_text: "12.3s",
      is_final: true,
    } as any);

    expect(state.rows).toHaveLength(1);
    expect(state.rows[0]?.sku).toBe("SKU-2");
    expect(state.rows[0]?.stage).toBe("completed");
    expect(state.rows[0]?.price).toBe("¥5.20");
    expect(state.rows[0]?.elapsedText).toBe("12.3s");
    expect(state.rows[0]?.isFinal).toBe(true);
    expect(Reflect.get(state.rows[0] as object, "recallMode")).toBe("source_first_pass");
    expect(Reflect.get(state.rows[0] as object, "originalImageUrl")).toBe(
      "data:image/png;base64,source-preview",
    );
    expect(Reflect.get(state.rows[0] as object, "matchedImageUrl")).toBe(
      "https://img.1688.com/2.jpg",
    );
  });

  test("keeps rows ordered by row index when staged updates arrive", () => {
    const state = createEmptyMonitor();
    appendRowResult(state, {
      row_index: 3,
      sku: "SKU-3",
      stage: "queued",
      status: "queued",
      recall_mode: null,
      image_url: null,
      original_image_url: null,
      matched_image_url: null,
      item_url: null,
      price: null,
      elapsed_text: null,
      is_final: false,
    } as any);
    appendRowResult(state, {
      row_index: 1,
      sku: "SKU-1",
      stage: "queued",
      status: "queued",
      recall_mode: null,
      image_url: null,
      original_image_url: null,
      matched_image_url: null,
      item_url: null,
      price: null,
      elapsed_text: null,
      is_final: false,
    } as any);

    expect(state.rows.map((row) => row.rowIndex)).toEqual([1, 3]);
  });

  test("retains known thumbnails when later stage updates omit image fields", () => {
    const state = createEmptyMonitor();
    appendRowResult(state, {
      row_index: 1,
      sku: "SKU-1",
      stage: "searching_1688_source_image",
      status: "processing",
      recall_mode: null,
      image_url: null,
      original_image_url: "data:image/png;base64,source-preview",
      matched_image_url: null,
      item_url: null,
      price: null,
      elapsed_text: null,
      is_final: false,
    } as any);
    appendRowResult(state, {
      row_index: 1,
      sku: "SKU-1",
      stage: "screening_candidates",
      status: "整图纠偏后已召回 8 个候选，AI复核中",
      recall_mode: "full_crop_second_pass",
      image_url: null,
      original_image_url: null,
      matched_image_url: null,
      item_url: null,
      price: null,
      elapsed_text: "4.2s",
      is_final: false,
    } as any);

    expect(Reflect.get(state.rows[0] as object, "originalImageUrl")).toBe(
      "data:image/png;base64,source-preview",
    );
    expect(Reflect.get(state.rows[0] as object, "recallMode")).toBe("full_crop_second_pass");
    expect(state.rows[0]?.elapsedText).toBe("4.2s");
  });

  test("updates progress fields", () => {
    const state = createEmptyMonitor();
    updateProgress(state, { processed: 3, total: 10 });
    expect(state.progress.processed).toBe(3);
    expect(state.progress.total).toBe(10);
  });

  test("resets row board when a new task starts (processed=0)", () => {
    const state = createEmptyMonitor();
    appendRowResult(state, {
      row_index: 1,
      sku: "SKU-1",
      stage: "completed",
      status: "processed",
      recall_mode: "source_first_pass",
      image_url: null,
      original_image_url: "data:image/png;base64,source-preview",
      matched_image_url: "https://img.1688.com/1.jpg",
      item_url: null,
      price: null,
      elapsed_text: null,
      is_final: true,
    } as any);
    setBlockingAlert(state, {
      code: "ANTI_BOT_CHALLENGE",
      message: "challenge",
      blocking: true,
      action_label: "已验证，继续执行",
    });

    updateProgress(state, { processed: 0, total: 5 });

    expect(state.rows).toHaveLength(0);
    expect(state.alert).toBeNull();
    expect(Reflect.get(state as object, "taskPhase")).toBeNull();
    expect(state.progress.total).toBe(5);
  });

  test("stores task phase and clears it on a new task reset", () => {
    const state = createEmptyMonitor();

    setTaskPhase(state, {
      phase: "resolving_ozon_products",
      label: "解析 Ozon 商品源",
      detail: "正在抓取商品详情与首图",
      blocking: false,
    });

    expect(Reflect.get(state as object, "taskPhase")).toEqual({
      phase: "resolving_ozon_products",
      label: "解析 Ozon 商品源",
      detail: "正在抓取商品详情与首图",
      blocking: false,
    });

    updateProgress(state, { processed: 0, total: 2 });

    expect(Reflect.get(state as object, "taskPhase")).toBeNull();
  });

  test("marks task done summary", () => {
    const state = createEmptyMonitor();
    markTaskDone(state, {
      excel_path: "/tmp/input.xlsx",
      status: "completed",
      processed_rows: 10,
      total_rows: 10,
      result_path: "/tmp/result.xlsx",
    });

    expect(state.done?.status).toBe("completed");
    expect(state.done?.result_path).toBe("/tmp/result.xlsx");
  });
});
