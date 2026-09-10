//! 输出约定：成功 JSON envelope 走 stdout，人类文本走 stdout，错误/日志一律 stderr。

use serde_json::{json, Value};

/// 输出成功 JSON envelope：`{"schema_version":1,"result":…}`，单行、无日志混入。
pub fn emit_json(result: &Value) {
    println!("{}", json!({ "schema_version": 1, "result": result }));
}

/// 输出人类可读成功信息。
pub fn emit_text(message: impl std::fmt::Display) {
    println!("{message}");
}
