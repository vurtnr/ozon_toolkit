export interface BlockingAlertModel {
  code: string;
  message: string;
  blocking: boolean;
  action_label: string | null;
}

export function isResumeActionRequired(alert: BlockingAlertModel | null): boolean {
  if (!alert) return false;
  return alert.code === "ANTI_BOT_CHALLENGE" || alert.code === "RESUME_REQUIRED";
}

export function mapBlockingAlertTitle(code: string): string {
  if (code === "CHROME_NOT_FOUND") return "环境缺失预警";
  if (code === "LOGIN_REQUIRED") return "登录提醒";
  if (code === "ANTI_BOT_CHALLENGE") return "风控验证提醒";
  if (code === "RESUME_REQUIRED") return "任务已暂停";
  return "运行阻断";
}
