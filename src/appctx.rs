//! 运行上下文：数据根 + 工作区 + 布局的一次性解析。

use crate::config::device_id;
use crate::error::Result;
use crate::paths::{layout_for, resolve_data_root, WsLayout};
use crate::workspace::{discover, Workspace};
use std::path::{Path, PathBuf};

pub struct AppContext {
    pub data_root: PathBuf,
    pub layout: WsLayout,
    pub workspace: Workspace,
    pub device: String,
}

impl AppContext {
    pub fn discover(
        data_root_arg: Option<&Path>,
        cwd: &Path,
        explicit_root: Option<&Path>,
    ) -> Result<AppContext> {
        let data_root = resolve_data_root(data_root_arg)?;
        let workspace = discover(cwd, explicit_root)?;
        let layout = layout_for(&data_root, &workspace.workspace_id, &workspace.anchor_key);
        let device = device_id(&data_root)?;
        Ok(AppContext {
            data_root,
            layout,
            workspace,
            device,
        })
    }

    /// 团队源缓存根：按源身份隔离（契约 §3）。
    pub fn source_cache(&self, identity: &str) -> PathBuf {
        self.data_root
            .join("cache")
            .join(crate::ids::cache_key_from_identity(identity))
    }

    /// 可提交声明路径（可能尚未存在）。
    pub fn declaration_path(&self) -> Option<PathBuf> {
        Some(
            self.workspace
                .workspace_root
                .join(crate::workspace::AILOOM_DIR)
                .join(crate::workspace::DECLARATION_FILE),
        )
    }
}
