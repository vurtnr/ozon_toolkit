import { describe, expect, test } from "bun:test";
import {
  isResumeActionRequired,
  mapBlockingAlertTitle,
  type BlockingAlertModel,
} from "../blockingAlert";

describe("Blocking alert helpers", () => {
  test("maps CHROME_NOT_FOUND to environment warning title", () => {
    expect(mapBlockingAlertTitle("CHROME_NOT_FOUND")).toBe("环境缺失预警");
  });

  test("maps LOGIN_REQUIRED to login warning title", () => {
    expect(mapBlockingAlertTitle("LOGIN_REQUIRED")).toBe("登录提醒");
  });

  test("marks only manual verification states as resumable", () => {
    const antiBot: BlockingAlertModel = {
      code: "ANTI_BOT_CHALLENGE",
      message: "challenge",
      blocking: true,
      action_label: "已验证，继续执行",
    };

    const loginRequired: BlockingAlertModel = {
      code: "LOGIN_REQUIRED",
      message: "login",
      blocking: true,
      action_label: "已登录，继续执行",
    };

    const paused: BlockingAlertModel = {
      code: "RESUME_REQUIRED",
      message: "paused",
      blocking: true,
      action_label: "已验证，继续执行",
    };

    const chrome: BlockingAlertModel = {
      code: "CHROME_NOT_FOUND",
      message: "missing",
      blocking: true,
      action_label: null,
    };

    expect(isResumeActionRequired(antiBot)).toBe(true);
    expect(isResumeActionRequired(loginRequired)).toBe(false);
    expect(isResumeActionRequired(paused)).toBe(true);
    expect(isResumeActionRequired(chrome)).toBe(false);
  });
});
