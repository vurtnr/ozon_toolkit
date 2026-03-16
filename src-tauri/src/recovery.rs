use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;

use serde::Serialize;

pub const CODE_CHROME_NOT_FOUND: &str = "CHROME_NOT_FOUND";
pub const CODE_LOGIN_REQUIRED: &str = "LOGIN_REQUIRED";
pub const CODE_ANTI_BOT_CHALLENGE: &str = "ANTI_BOT_CHALLENGE";
pub const CODE_RESUME_REQUIRED: &str = "RESUME_REQUIRED";

#[derive(Debug, Clone, Serialize)]
pub struct BlockingAlertPayload {
    pub code: String,
    pub message: String,
    pub blocking: bool,
    pub action_label: Option<String>,
}

pub fn blocking_alert_for_code(code: &str) -> Option<BlockingAlertPayload> {
    match code {
        CODE_CHROME_NOT_FOUND => Some(BlockingAlertPayload {
            code: CODE_CHROME_NOT_FOUND.to_string(),
            message: "未能自动检测到 Chrome 浏览器，请前往【设置】手动指定执行路径".to_string(),
            blocking: true,
            action_label: None,
        }),
        CODE_LOGIN_REQUIRED => Some(BlockingAlertPayload {
            code: CODE_LOGIN_REQUIRED.to_string(),
            message:
                "检测到当前 1688 未登录。请在弹出的 Chrome 窗口中完成登录，系统会在检测到登录成功后自动继续。"
                    .to_string(),
            blocking: true,
            action_label: None,
        }),
        CODE_ANTI_BOT_CHALLENGE => Some(BlockingAlertPayload {
            code: CODE_ANTI_BOT_CHALLENGE.to_string(),
            message: "触发 1688 底层拦截，请在弹出的浏览器窗口中完成验证".to_string(),
            blocking: true,
            action_label: Some("已验证，继续执行".to_string()),
        }),
        CODE_RESUME_REQUIRED => Some(BlockingAlertPayload {
            code: CODE_RESUME_REQUIRED.to_string(),
            message: "任务已暂停，完成登录或验证后点击继续按钮恢复队列".to_string(),
            blocking: true,
            action_label: Some("继续执行".to_string()),
        }),
        _ => None,
    }
}

pub struct RecoveryGate {
    paused: AtomicBool,
}

impl RecoveryGate {
    pub const fn new() -> Self {
        Self {
            paused: AtomicBool::new(false),
        }
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

pub static GLOBAL_RECOVERY_GATE: LazyLock<RecoveryGate> = LazyLock::new(RecoveryGate::new);
