//! 运行上下文：数据根 + 工作区 + 布局的一次性解析。

use crate::config::device_id;
use crate::error::Result;
use crate::paths::{layout_for, resolve_data_root, WsLayout};
use crate::workspace::{discover, Workspace};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct AppContext {
    pub data_root: PathBuf,
    pub layout: WsLayout,
    pub workspace: Workspace,
    pub device: String,
    /// 本机按源身份选择的下载父目录；保存在 machine 中，不提交。
    pub source_cache_roots: BTreeMap<String, PathBuf>,
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
        let source_cache_roots = Self::load_source_cache_roots(&workspace.workspace_root)?;
        Ok(AppContext {
            data_root,
            layout,
            workspace,
            device,
            source_cache_roots,
        })
    }

    /// 团队源缓存根：按源身份隔离（契约 §3）。
    pub fn source_cache(&self, identity: &str) -> PathBuf {
        self.source_cache_root(identity)
            .join(crate::ids::cache_key_from_identity(identity))
    }

    pub fn default_source_cache_root(&self) -> PathBuf {
        self.data_root.join("cache")
    }

    pub fn source_cache_root(&self, identity: &str) -> PathBuf {
        self.source_cache_roots
            .get(identity)
            .cloned()
            .unwrap_or_else(|| self.default_source_cache_root())
    }

    pub fn source_cache_settings_path(&self) -> PathBuf {
        self.workspace
            .workspace_root
            .join(".ailoom/machine/source-storage.json")
    }

    pub fn load_source_cache_roots(root: &Path) -> Result<BTreeMap<String, PathBuf>> {
        let path = root.join(".ailoom/machine/source-storage.json");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(e.into()),
        };
        let roots: BTreeMap<String, PathBuf> = serde_json::from_slice(&bytes).map_err(|e| {
            crate::error::Error::new(
                crate::error::code::SOURCE_CACHE_CORRUPT,
                format!("本机下载目录配置损坏：{e}"),
            )
            .context(serde_json::json!({"path":path}))
        })?;
        if roots.values().any(|root| !root.is_absolute()) {
            return Err(crate::error::Error::new(
                crate::error::code::SOURCE_CACHE_CORRUPT,
                "本机下载目录配置必须使用绝对路径",
            ));
        }
        Ok(roots)
    }

    pub fn set_source_cache_root(&mut self, identity: &str, root: &Path) {
        if root == self.default_source_cache_root() {
            self.source_cache_roots.remove(identity);
        } else {
            self.source_cache_roots.insert(identity.into(), root.into());
        }
    }

    pub fn save_source_cache_roots(&self) -> Result<()> {
        let path = self.source_cache_settings_path();
        let dir = path.parent().unwrap();
        std::fs::create_dir_all(dir)?;
        let ignore = dir.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(ignore, "*\n")?;
        }
        crate::sync_common::atomic_write(
            &path,
            &serde_json::to_vec_pretty(&self.source_cache_roots)?,
        )
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
