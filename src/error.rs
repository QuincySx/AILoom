//! AILoom 错误类型：稳定 code、退出码、message、context 与可选修复建议。
//! 码表见 docs/CONTRACTS.md §5（AIL-001 冻结）。

use serde_json::{json, Value};
use std::fmt;

/// 按契约冻结的错误码常量（段：E0 用法 E1 工作区 E2 源/Git E3 清单/资源 E4 计划/同步
/// E5 宿主/适配 E6 知识 E7 事件 E8 上报/导入/贡献 E9 未分类）。
pub mod code {
    pub const USAGE: &str = "E0001";
    pub const WORKSPACE_ROOT_NOT_FOUND: &str = "E1001";
    pub const WORKSPACE_INVALID: &str = "E1002";
    pub const REFUSE_GLOBAL_WRITE: &str = "E1003";
    pub const SOURCE_NOT_CACHED: &str = "E2001";
    pub const SOURCE_FETCH_FAILED: &str = "E2002";
    pub const SOURCE_INVALID_REF: &str = "E2003";
    pub const SOURCE_URL_CREDENTIAL: &str = "E2004";
    pub const SOURCE_CACHE_CORRUPT: &str = "E2005";
    pub const SOURCE_CONFLICT: &str = "E2006";
    pub const GIT_COMMAND_FAILED: &str = "E2007";
    pub const SCHEMA_VERSION: &str = "E3001";
    pub const MANIFEST_MISSING_FIELD: &str = "E3002";
    pub const PATH_TRAVERSAL: &str = "E3003";
    pub const UNKNOWN_REFERENCE: &str = "E3004";
    pub const OWNERLESS_RESOURCE: &str = "E3005";
    pub const RESOURCE_ID_CONFLICT: &str = "E3006";
    pub const ILLEGAL_PATH: &str = "E3007";
    pub const DUPLICATE_ITEM: &str = "E3008";
    pub const PRECONDITION_FAILED: &str = "E4001";
    pub const TARGET_CONFLICT: &str = "E4002";
    pub const LOCK_HELD: &str = "E4003";
    pub const WRITE_FAILED: &str = "E4004";
    pub const JOURNAL_RESTORE_FAILED: &str = "E4005";
    /// 托管清单（managed-manifest.json）损坏：无法判断哪些文件由 AILoom 部署。
    pub const MANAGED_MANIFEST_CORRUPT: &str = "E4006";
    pub const HOST_UNSUPPORTED: &str = "E5001";
    pub const RENDER_FAILED: &str = "E5002";
    pub const CAPABILITY_UNKNOWN: &str = "E5003";
    pub const USER_CONTENT_CONFLICT: &str = "E5004";
    pub const INDEX_CORRUPT: &str = "E6001";
    pub const LEARNING_SCOPE_INVALID: &str = "E6002";
    /// 知识库位置/可迁移恢复状态无效或与本机已有状态冲突（v1.4）。
    pub const KNOWLEDGE_STATE_CONFLICT: &str = "E6003";
    pub const EVENT_PAYLOAD_INVALID: &str = "E7001";
    pub const REPORT_NOT_CONFIRMED: &str = "E8001";
    pub const IMPORT_OUT_OF_SCOPE: &str = "E8002";
    pub const CONTRIBUTION_DIVERGED: &str = "E8101";
    pub const PR_CREATE_FAILED: &str = "E8102";
    pub const INTERNAL: &str = "E9000";
    /// 本地网页服务（启动/停止/状态/自启动）操作失败（v1.4，退出码沿用 1）。
    pub const SERVICE_FAILED: &str = "E9101";
}

/// 「冲突」类错误：前置条件/版本不符、目标冲突、用户内容冲突、知识库状态冲突。
/// 控制台据此返回 409，前端提示重新预览或核对，而不是当作输入错误。
pub fn is_conflict(code: &str) -> bool {
    matches!(
        code,
        code::PRECONDITION_FAILED
            | code::TARGET_CONFLICT
            | code::USER_CONTENT_CONFLICT
            | code::KNOWLEDGE_STATE_CONFLICT
    )
}

/// 按契约退出的稳定退出码映射：码段前缀决定类。
pub fn exit_code_for(code: &str) -> i32 {
    // E8100-E8199 贡献/PR 是 E8 段内单独的类（契约 §5）。
    if code.starts_with("E81") {
        return 18;
    }
    let seg = code.get(1..2).unwrap_or("9");
    match seg {
        "0" => 2,  // 用法
        "1" => 10, // 工作区
        "2" => 11, // 源/Git
        "3" => 12, // 清单/资源
        "4" => 13, // 计划/同步
        "5" => 14, // 宿主/适配
        "6" => 15, // 知识
        "7" => 16, // 事件
        "8" => 17, // 上报/导入
        _ => 1,    // 未分类
    }
}

/// 统一业务错误。`code` 必须来自 [`code`] 常量，保证稳定。
#[derive(Debug, Clone)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub context: Value,
    pub fix: Option<String>,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Error {
            code: code.to_string(),
            message: message.into(),
            context: Value::Null,
            fix: None,
        }
    }

    pub fn context(mut self, context: Value) -> Self {
        self.context = context;
        self
    }

    pub fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    pub fn exit_code(&self) -> i32 {
        exit_code_for(&self.code)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "message": self.message,
            "context": self.context,
            "fix": self.fix,
        })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if !self.context.is_null() {
            write!(f, " | context: {}", self.context)?;
        }
        if let Some(fix) = &self.fix {
            write!(f, " | 建议: {fix}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::new(code::INTERNAL, format!("IO 错误: {err}"))
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::new(code::INTERNAL, format!("JSON 错误: {err}"))
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!("TOML 解析错误: {err}"),
        )
    }
}

impl From<toml::ser::Error> for Error {
    fn from(err: toml::ser::Error) -> Self {
        Error::new(code::INTERNAL, format!("TOML 序列化错误: {err}"))
    }
}

impl From<std::path::StripPrefixError> for Error {
    fn from(err: std::path::StripPrefixError) -> Self {
        Error::new(code::INTERNAL, format!("路径前缀剥离失败: {err}"))
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(err: serde_yaml::Error) -> Self {
        Error::new(
            code::MANIFEST_MISSING_FIELD,
            format!("YAML 解析错误: {err}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 码表一致性：每个错误码常量都必须登记在 CONTRACTS §5，并能映射到非 0 退出码。
    #[test]
    fn every_code_is_registered_in_contract() {
        let src = include_str!("error.rs");
        let contract = include_str!("../docs/CONTRACTS.md");
        let start = contract.find("## 5. 错误码与退出码").unwrap();
        let section = &contract[start..start + contract[start..].find("## 6.").unwrap()];
        let mut missing = Vec::new();
        for line in src.lines() {
            let Some(rest) = line.trim().strip_prefix("pub const ") else {
                continue;
            };
            let Some(value) = rest
                .split('"')
                .nth(1)
                .filter(|v| v.starts_with('E') && v.len() == 5)
            else {
                continue;
            };
            if !section.contains(value) {
                missing.push(value.to_string());
            }
            assert_ne!(exit_code_for(value), 0);
        }
        assert!(missing.is_empty(), "CONTRACTS §5 未登记: {missing:?}");
    }

    #[test]
    fn exit_codes_follow_segment_table() {
        assert_eq!(exit_code_for("E0001"), 2);
        assert_eq!(exit_code_for("E8001"), 17);
        assert_eq!(exit_code_for("E8102"), 18);
        assert_eq!(exit_code_for("E1001"), 10);
        assert_eq!(exit_code_for("E2003"), 11);
        assert_eq!(exit_code_for("E3003"), 12);
        assert_eq!(exit_code_for("E4001"), 13);
        assert_eq!(exit_code_for("E5001"), 14);
        assert_eq!(exit_code_for("E6001"), 15);
        assert_eq!(exit_code_for("E7001"), 16);
        assert_eq!(exit_code_for("E8101"), 18);
        assert_eq!(exit_code_for("E9000"), 1);
    }

    #[test]
    fn error_json_has_stable_fields() {
        let err = Error::new(code::TARGET_CONFLICT, "目标已存在")
            .context(json!({"path": "/x/y"}))
            .fix("ailoom plan 查看差异");
        let v = err.to_json();
        assert_eq!(v["code"], "E4002");
        assert_eq!(v["context"]["path"], "/x/y");
        assert!(v["fix"].is_string());
    }
}
