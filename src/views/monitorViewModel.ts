import type {
  LogEventPayload,
  MonitorRow,
  ProgressEventPayload,
  TaskPhaseEventPayload,
} from "../types/events";

export type StageTone = "info" | "ready" | "warn" | "danger";
export type StageEmphasis = "live" | "terminal";

export interface StagePresentation {
  label: string;
  tone: StageTone;
  emphasis: StageEmphasis;
}

export interface ResolvedMonitorStage extends StagePresentation {
  detail: string;
  source: "task_phase" | "row" | "idle";
}

export interface MonitorBoardSummary {
  progressText: string;
  activeRow: MonitorRow | null;
  completedCount: number;
  failedCount: number;
  pendingCount: number;
  recentLogs: LogEventPayload[];
  taskPhase: TaskPhaseEventPayload | null;
}

export interface OutcomeSummaryCard {
  label: string;
  value: string;
  detail: string;
  tone: StageTone;
}

const STAGE_PRESENTATIONS: Record<string, StagePresentation> = {
  queued: { label: "排队中", tone: "info", emphasis: "live" },
  resolving_ozon_sku: { label: "Ozon 搜索 SKU", tone: "info", emphasis: "live" },
  planning_search_image: { label: "生成搜索图", tone: "info", emphasis: "live" },
  searching_1688_primary: {
    label: "1688 首轮搜索",
    tone: "info",
    emphasis: "live",
  },
  searching_1688_fallback: {
    label: "1688 兜底搜索",
    tone: "warn",
    emphasis: "live",
  },
  candidates_recalled: {
    label: "候选已召回",
    tone: "info",
    emphasis: "live",
  },
  screening_candidates: {
    label: "AI 初筛中",
    tone: "info",
    emphasis: "live",
  },
  final_review: { label: "严格终审", tone: "warn", emphasis: "live" },
  writing_diagnostics: {
    label: "写入诊断",
    tone: "warn",
    emphasis: "live",
  },
  completed: { label: "处理完成", tone: "ready", emphasis: "terminal" },
  failed: { label: "执行失败", tone: "danger", emphasis: "terminal" },
};

const TASK_PHASE_TONES: Record<string, StageTone> = {
  validating_runtime: "info",
  resolving_ozon_products: "info",
  warming_ozon_session: "info",
  waiting_for_1688_login: "warn",
  waiting_for_ozon_verification: "danger",
  running_1688_and_ai: "info",
  exporting_results: "ready",
};

function resolveTerminalFailurePresentation(status: string): StagePresentation | null {
  if (
    status.includes("Ozon触发风控") ||
    status.includes("未完成浏览器验证") ||
    status.includes("等待验证")
  ) {
    return {
      label: "等待验证",
      tone: "danger",
      emphasis: "terminal",
    };
  }

  if (
    status.includes("Ozon商品已下架") ||
    status.includes("不可访问") ||
    status.includes("Ozon链接无效") ||
    status.includes("未解析到Ozon商品") ||
    status.includes("Ozon 未找到 SKU") ||
    status.includes("Ozon主图抓取失败")
  ) {
    return {
      label: "源图不可用",
      tone: "warn",
      emphasis: "terminal",
    };
  }

  return null;
}

export function hasLockedResult(row: Pick<MonitorRow, "itemUrl">): boolean {
  return Boolean(row.itemUrl);
}

export function getStagePresentation(
  row: Pick<MonitorRow, "stage" | "status" | "isFinal" | "itemUrl">,
): StagePresentation {
  if (row.stage === "failed") {
    return STAGE_PRESENTATIONS.failed;
  }

  if (row.isFinal && hasLockedResult(row)) {
    return {
      label: "结果已锁定",
      tone: "ready",
      emphasis: "terminal",
    };
  }

  if (row.isFinal) {
    const specialized = resolveTerminalFailurePresentation(row.status);
    if (specialized) {
      return specialized;
    }
    return {
      label: "未锁定结果",
      tone: row.status.includes("失败") ? "danger" : "warn",
      emphasis: "terminal",
    };
  }

  return (
    STAGE_PRESENTATIONS[row.stage] || {
      label: "处理中",
      tone: "info",
      emphasis: "live",
    }
  );
}

export function resolveMonitorStage(
  activeRow: MonitorRow | null,
  taskPhase: TaskPhaseEventPayload | null,
): ResolvedMonitorStage {
  const shouldPreferTaskPhase = Boolean(
    taskPhase &&
      (taskPhase.blocking || taskPhase.phase !== "running_1688_and_ai"),
  );

  if (taskPhase && shouldPreferTaskPhase) {
    return {
      label: taskPhase.label,
      tone: TASK_PHASE_TONES[taskPhase.phase] || (taskPhase.blocking ? "warn" : "info"),
      emphasis: "live",
      detail: taskPhase.detail,
      source: "task_phase",
    };
  }

  if (activeRow) {
    return {
      ...getStagePresentation(activeRow),
      detail: activeRow.status,
      source: "row",
    };
  }

  if (taskPhase) {
    return {
      label: taskPhase.label,
      tone: TASK_PHASE_TONES[taskPhase.phase] || "info",
      emphasis: "live",
      detail: taskPhase.detail,
      source: "task_phase",
    };
  }

  return {
    label: "等待任务启动",
    tone: "info",
    emphasis: "live",
    detail: "上传 Excel 后，系统会按阶段推进任务。",
    source: "idle",
  };
}

export function resolveOutcomeSummaryCard(
  failedCount: number,
  taskPhase: TaskPhaseEventPayload | null,
): OutcomeSummaryCard {
  if (taskPhase?.blocking) {
    return {
      label: "任务阻断中",
      value: taskPhase.label,
      detail: taskPhase.detail,
      tone: TASK_PHASE_TONES[taskPhase.phase] || "warn",
    };
  }

  return {
    label: "未锁定结果",
    value: String(failedCount),
    detail: "已完成处理但未找到可交付商品的行数。",
    tone: failedCount > 0 ? "warn" : "info",
  };
}

export function summarizeMonitorBoard(
  rows: MonitorRow[],
  logs: LogEventPayload[],
  progress: ProgressEventPayload,
  taskPhase: TaskPhaseEventPayload | null = null,
): MonitorBoardSummary {
  const lastRow = rows.length > 0 ? rows[rows.length - 1] : null;
  const activeRow = rows.find((row) => !row.isFinal) || lastRow;
  const completedCount = rows.filter(
    (row) => row.isFinal && hasLockedResult(row),
  ).length;
  const failedCount = rows.filter(
    (row) => row.isFinal && !hasLockedResult(row),
  ).length;
  const pendingCount = rows.filter((row) => !row.isFinal).length;

  return {
    progressText: `${progress.processed} / ${progress.total}`,
    activeRow,
    completedCount,
    failedCount,
    pendingCount,
    recentLogs: logs.slice(-5).reverse(),
    taskPhase,
  };
}

export function getLogTone(level: string): StageTone {
  if (level === "error") return "danger";
  if (level === "warn") return "warn";
  if (level === "info") return "info";
  return "ready";
}
