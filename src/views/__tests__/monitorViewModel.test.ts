import { describe, expect, test } from "bun:test";
import type { LogEventPayload, MonitorRow, TaskPhaseEventPayload } from "../../types/events";
import {
  getStagePresentation,
  resolveMonitorStage,
  summarizeMonitorBoard,
} from "../monitorViewModel";

function buildRow(overrides: Partial<MonitorRow> = {}): MonitorRow {
  return {
    rowIndex: 1,
    sku: "SKU-1",
    stage: "queued",
    status: "排队中",
    imageUrl: null,
    originalImageUrl: null,
    matchedImageUrl: null,
    itemUrl: null,
    price: null,
    elapsedText: null,
    isFinal: false,
    ...overrides,
  } as MonitorRow;
}

describe("monitorViewModel", () => {
  test("prefers task-level preflight phase before row execution starts", () => {
    const phase: TaskPhaseEventPayload = {
      phase: "resolving_ozon_products",
      label: "解析 Ozon 商品源",
      detail: "正在解析商品标题与主图",
      blocking: false,
    };

    const presentation = resolveMonitorStage(null, phase);

    expect(presentation.label).toBe("解析 Ozon 商品源");
    expect(presentation.tone).toBe("info");
    expect(presentation.detail).toBe("正在解析商品标题与主图");
  });

  test("prefers blocking login task phase over row stage", () => {
    const phase: TaskPhaseEventPayload = {
      phase: "waiting_for_1688_login",
      label: "等待 1688 登录",
      detail: "请先完成 1688 登录，完成后任务会继续",
      blocking: true,
    };

    const presentation = resolveMonitorStage(
      buildRow({
        stage: "queued",
        status: "排队中",
      }),
      phase,
    );

    expect(presentation.label).toBe("等待 1688 登录");
    expect(presentation.tone).toBe("warn");
    expect(presentation.detail).toContain("完成 1688 登录");
  });

  test("maps failed final rows to a danger stage presentation", () => {
    const presentation = getStagePresentation(
      buildRow({
        stage: "failed",
        status: "处理失败",
        isFinal: true,
      }),
    );

    expect(presentation.label).toBe("执行失败");
    expect(presentation.tone).toBe("danger");
    expect(presentation.emphasis).toBe("terminal");
  });

  test("summarizes active row and recent logs for the monitor board", () => {
    const rows = [
      buildRow({
        rowIndex: 1,
        stage: "completed",
        status: "已完成",
        price: "¥12.40",
        itemUrl: "https://detail.1688.com/offer/1.html",
        elapsedText: "18.2s",
        isFinal: true,
      }),
      buildRow({
        rowIndex: 2,
        sku: "SKU-2",
        stage: "screening_candidates",
        status: "候选已召回，AI 初筛中",
      }),
      buildRow({
        rowIndex: 3,
        sku: "SKU-3",
        stage: "failed",
        status: "未找到可用候选",
        isFinal: true,
      }),
    ];
    const logs: LogEventPayload[] = Array.from({ length: 7 }, (_, index) => ({
      level: index % 2 === 0 ? "info" : "warn",
      message: `log-${index + 1}`,
    }));

    const summary = summarizeMonitorBoard(rows, logs, {
      processed: 1,
      total: 3,
    });

    expect(summary.progressText).toBe("1 / 3");
    expect(summary.activeRow?.sku).toBe("SKU-2");
    expect(summary.completedCount).toBe(1);
    expect(summary.failedCount).toBe(1);
    expect(summary.pendingCount).toBe(1);
    expect(summary.recentLogs.map((entry) => entry.message)).toEqual([
      "log-7",
      "log-6",
      "log-5",
      "log-4",
      "log-3",
    ]);
  });
});
