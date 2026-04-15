use crate::env_flag_true;
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, Registry, fmt, prelude::*};

/// 本地审计日志：未启用 Langfuse、上报失败，或 `AUDIT_LOG_ALWAYS` 开启时写入
pub const AUDIT_TARGET: &str = "ollama_audit";

/// 初始化滚动文件日志（须持有返回的 `WorkerGuard` 直至进程结束，否则缓冲可能未刷盘）
pub fn init_tracing() -> WorkerGuard {
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| "logs".to_string());
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("failed to create LOG_DIR {log_dir:?}: {e}");
    }

    let rot = std::env::var("LOG_ROTATION").unwrap_or_default();
    let rot_key = rot.to_ascii_lowercase();
    let (rotation, rotation_label) = match rot_key.as_str() {
        "hourly" | "hour" => (Rotation::HOURLY, "hourly"),
        "minutely" | "minute" => (Rotation::MINUTELY, "minutely"),
        _ => (Rotation::DAILY, "daily"),
    };

    let file_appender = RollingFileAppender::new(rotation, &log_dir, "ollama-proxy");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let registry = Registry::default().with(filter).with(file_layer);

    if env_flag_true("LOG_DISABLE_STDOUT") {
        registry.init();
    } else {
        let stdout_layer = fmt::layer().with_writer(std::io::stdout).with_target(true);
        registry.with(stdout_layer).init();
    }

    info!("rolling log file: directory={log_dir:?} prefix=ollama-proxy rotation={rotation_label}");

    guard
}
