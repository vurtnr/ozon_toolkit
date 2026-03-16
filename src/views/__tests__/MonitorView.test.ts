import { describe, expect, test } from "bun:test";
import {
  appendRowResult,
  createEmptyMonitor,
  setBlockingAlert,
  markTaskDone,
  updateProgress,
} from "../../composables/useTaskEvents";

describe("Monitor helpers", () => {
  test("upserts incoming row result by row index", () => {
    const state = createEmptyMonitor();
    appendRowResult(state, {
      row_index: 2,
      sku: "SKU-2",
      stage: "planning_search_image",
      status: "processing",
      image_url: null,
      item_url: null,
      price: null,
      elapsed_text: null,
      is_final: false,
    });
    appendRowResult(state, {
      row_index: 2,
      sku: "SKU-2",
      stage: "completed",
      status: "processed",
      image_url: null,
      item_url: "https://detail.1688.com/offer/2.html",
      price: "¥5.20",
      elapsed_text: "12.3s",
      is_final: true,
    });

    expect(state.rows).toHaveLength(1);
    expect(state.rows[0]?.sku).toBe("SKU-2");
    expect(state.rows[0]?.stage).toBe("completed");
    expect(state.rows[0]?.price).toBe("¥5.20");
    expect(state.rows[0]?.elapsedText).toBe("12.3s");
    expect(state.rows[0]?.isFinal).toBe(true);
  });

  test("keeps rows ordered by row index when staged updates arrive", () => {
    const state = createEmptyMonitor();
    appendRowResult(state, {
      row_index: 3,
      sku: "SKU-3",
      stage: "queued",
      status: "queued",
      image_url: null,
      item_url: null,
      price: null,
      elapsed_text: null,
      is_final: false,
    });
    appendRowResult(state, {
      row_index: 1,
      sku: "SKU-1",
      stage: "queued",
      status: "queued",
      image_url: null,
      item_url: null,
      price: null,
      elapsed_text: null,
      is_final: false,
    });

    expect(state.rows.map((row) => row.rowIndex)).toEqual([1, 3]);
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
      image_url: null,
      item_url: null,
      price: null,
      elapsed_text: null,
      is_final: true,
    });
    setBlockingAlert(state, {
      code: "ANTI_BOT_CHALLENGE",
      message: "challenge",
      blocking: true,
      action_label: "已验证，继续执行",
    });

    updateProgress(state, { processed: 0, total: 5 });

    expect(state.rows).toHaveLength(0);
    expect(state.alert).toBeNull();
    expect(state.progress.total).toBe(5);
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
