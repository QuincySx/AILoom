//! 结构化日志：只走 stderr，级别由 `AILOOM_LOG` 控制（error|warn|info|debug，默认 info）。

use std::io::Write;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }
}

fn max_level() -> Level {
    static LEVEL: OnceLock<Level> = OnceLock::new();
    *LEVEL.get_or_init(|| match std::env::var("AILOOM_LOG").as_deref() {
        Ok("error") => Level::Error,
        Ok("warn") => Level::Warn,
        Ok("debug") | Ok("trace") => Level::Debug,
        _ => Level::Info,
    })
}

pub fn log(level: Level, message: impl std::fmt::Display) {
    if level <= max_level() {
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(stderr, "[ailoom:{}] {message}", level.as_str());
    }
}

pub fn error(message: impl std::fmt::Display) {
    log(Level::Error, message);
}

pub fn warn(message: impl std::fmt::Display) {
    log(Level::Warn, message);
}

pub fn info(message: impl std::fmt::Display) {
    log(Level::Info, message);
}

pub fn debug(message: impl std::fmt::Display) {
    log(Level::Debug, message);
}
