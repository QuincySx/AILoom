//! 命令行定义。只注册实际实现的命令；新增能力由对应卡追加。

use clap::{Parser, Subcommand};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Parser, Debug)]
#[command(
    name = "ailoom",
    version,
    about = "团队 AI 资源、经验与工作观测管理工具",
    after_help = "更多文档见仓库 docs/ 目录"
)]
pub struct Cli {
    /// 机器可读 JSON 输出（成功结果走 stdout，错误走 stderr）
    #[arg(long, global = true)]
    pub json: bool,

    /// 注入机器数据根（缓存/事件/索引等），默认取平台适当目录；测试必用
    #[arg(long, value_name = "DIR", global = true)]
    pub data_root: Option<std::path::PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// 输出版本与构建信息（AIL-002）
    Version,
    /// 绑定团队源与项目/角色，生成本机锁（AIL-005）
    Init {
        /// 团队资源源 Git URL（禁止内嵌凭据）
        #[arg(long)]
        url: Option<String>,
        /// 源 ref（分支/标签），默认 main
        #[arg(long)]
        ref_: Option<String>,
        /// 源别名，默认 team
        #[arg(long)]
        name: Option<String>,
        /// 使用本地目录作为源（相对当前工作区根）
        #[arg(long)]
        local_path: Option<String>,
        /// 绑定的逻辑项目（可重复；出现即整体替换）
        #[arg(long = "project")]
        projects: Vec<String>,
        /// 绑定的职能角色（可重复；出现即整体替换）
        #[arg(long = "role")]
        roles: Vec<String>,
        /// 启用的宿主工具（可重复：claude/codex；出现即整体替换）
        #[arg(long = "target")]
        targets: Vec<String>,
        /// 显式更新源到 ref 最新 commit
        #[arg(long)]
        refresh: bool,
        /// 关闭内置资源（召回 Agent/总结 Skill）部署
        #[arg(long)]
        no_builtin: bool,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 展示工作区绑定与声明/锁一致性（AIL-005）
    Status {
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 预览同步差异（无写入，AIL-007）
    Plan {
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 执行同步计划（AIL-008）
    Sync {
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// 仅恢复未完成的同步 journal
        #[arg(long)]
        recover: bool,
        /// 先推进源锁到声明 ref 最新，再同步（自动同步 / 手动跟上团队仓）
        #[arg(long)]
        refresh: bool,
        /// 由 hook 后台调度：更新 auto_sync 状态；勿手动依赖
        #[arg(long, hide = true)]
        from_auto: bool,
    },
    /// Hook 事件采集：stdin payload → 标准事件（AIL-018，由宿主调用）
    Hook {
        /// 宿主工具
        #[arg(long)]
        tool: String,
        /// 事件类型：session-start/prompt/tool/stop
        #[arg(long)]
        event: String,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// Hook 注册管理：install/remove（AIL-018）
    Hooks {
        /// 动作：install | remove | exec
        #[arg(long)]
        action: String,
        /// exec 的资源 ID
        #[arg(long)]
        id: Option<String>,
        /// exec 事件（透传给团队 hook）
        #[arg(long)]
        event: Option<String>,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 会话指标与摘要：metrics | summary | ingest（AIL-019/020）
    Session {
        /// 动作：metrics | summary | ingest
        #[arg(long)]
        action: String,
        /// 会话 ID
        #[arg(long)]
        session: Option<String>,
        /// ingest 的 transcript 文件
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// 显式共享目标路径（白名单计数版）
        #[arg(long)]
        share: Option<std::path::PathBuf>,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 迁移独立源到同仓模式（AIL-036，copy→校验→切换→备份）
    Migrate {
        /// 现有独立团队源目录
        #[arg(long)]
        from: std::path::PathBuf,
        /// 子树路径（默认 .ailoom-team）
        #[arg(long, default_value = ".ailoom-team")]
        subtree: String,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 批量导入目录文档为知识（AIL-034）
    Import {
        /// 目标项目
        #[arg(long)]
        project: Option<String>,
        /// 导入目录（Markdown）
        #[arg(long)]
        dir: std::path::PathBuf,
        /// 目标：shared 或 project:<id>
        #[arg(long)]
        target: String,
        /// 资源类型：learning | doc
        #[arg(long, default_value = "doc")]
        kind: String,
        /// 预览后确认执行
        #[arg(long)]
        execute: bool,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// PR/MR 知识候选（AIL-035，显式指定 URL）
    Pr {
        /// 动作：draft
        #[arg(long, default_value = "draft")]
        action: String,
        /// GitHub PR URL
        #[arg(long)]
        url: String,
        /// 目标项目
        #[arg(long)]
        project: Option<String>,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 数据留存与清理：rotate | export | cleanup（AIL-037）
    Data {
        /// 动作：rotate | export | cleanup
        #[arg(long)]
        action: String,
        /// export 目标目录
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// rotate 阈值（MB）
        #[arg(long, default_value = "10")]
        max_size_mb: f64,
        /// 预览清理项
        #[arg(long)]
        dry_run: bool,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 成员名册：list | projects | register | remove（AIL-024）
    Members {
        /// 动作
        #[arg(long)]
        action: String,
        /// 项目 ID
        #[arg(long)]
        project: Option<String>,
        /// 成员 ID
        #[arg(long)]
        member: Option<String>,
        /// 提交信息
        #[arg(long, default_value = "ailoom: 名册更新")]
        message: String,
        /// PR 创建方式：auto 或 manual
        #[arg(long, default_value = "auto")]
        provider: String,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 团队包依赖：check | install（AIL-033）
    Packages {
        /// 动作：check | install
        #[arg(long)]
        action: String,
        /// install 需显式确认
        #[arg(long)]
        yes: bool,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 代码事实与图谱：build | query（AIL-026/027，Rust 优先）
    Code {
        /// 动作：build | query
        #[arg(long)]
        action: String,
        /// 查询关键词
        #[arg(long)]
        query: Option<String>,
        /// 图扩展跳数（0-2）
        #[arg(long, default_value = "1")]
        hops: usize,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 本地实时看板（AIL-021，仅 loopback）
    Dashboard {
        /// 监听端口
        #[arg(long, default_value = "7777")]
        port: u16,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 团队统计上报与汇总：push | digest | status（AIL-022）
    Report {
        /// 动作：push | digest | status
        #[arg(long)]
        action: String,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 知识反馈与维护：feedback | maintenance | promote（AIL-028）
    Knowledge {
        /// 动作：feedback | maintenance | promote
        #[arg(long)]
        action: String,
        /// 学习 ID
        #[arg(long)]
        id: Option<String>,
        /// 反馈方向：useful | not-useful
        #[arg(long)]
        useful: bool,
        /// 晋升草稿正文
        #[arg(long)]
        text: Option<String>,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 检索本地知识索引（AIL-016）
    Recall {
        /// 查询关键词（支持中文）
        #[arg(long = "query", short = 'q')]
        query: String,
        /// 过滤资源类型：learning/rule/doc/skill
        #[arg(long)]
        kind: Option<String>,
        /// 返回条数上限
        #[arg(long, default_value = "10")]
        limit: usize,
        /// 强制重建索引
        #[arg(long)]
        rebuild: bool,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 贡献经验文档（AIL-015）
    Contribute {
        /// 经验 Markdown 文档（frontmatter 含 title）
        #[arg(long)]
        file: std::path::PathBuf,
        /// 归属项目
        #[arg(long)]
        project: Option<String>,
        /// 显式共享给全团队
        #[arg(long)]
        shared: bool,
        /// namespace（缺省按清单推导）
        #[arg(long)]
        namespace: Option<String>,
        /// 提交信息
        #[arg(long, default_value = "ailoom: 新增经验")]
        message: String,
        /// PR 创建方式：auto 或 manual
        #[arg(long, default_value = "auto")]
        provider: String,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 贡献资源修改：从修改过的源克隆建立变更集并推送（AIL-014）
    Push {
        /// 修改过的团队源克隆目录
        #[arg(long)]
        from: std::path::PathBuf,
        /// 提交信息（也用作 PR 标题）
        #[arg(long, default_value = "ailoom: 资源更新")]
        message: String,
        /// PR 创建方式：auto（GitHub+gh）或 manual
        #[arg(long, default_value = "auto")]
        provider: String,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 工作区体检（AIL-013）
    Doctor {
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 按托管清单安全卸载（AIL-013）
    Uninstall {
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// 预览后确认执行
        #[arg(long)]
        execute: bool,
    },
}

#[derive(serde::Serialize)]
pub struct VersionInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub schema_version: u32,
    pub msrv: &'static str,
}

pub fn version_info() -> VersionInfo {
    VersionInfo {
        name: "ailoom",
        version: env!("CARGO_PKG_VERSION"),
        schema_version: SCHEMA_VERSION,
        msrv: env!("CARGO_PKG_RUST_VERSION"),
    }
}
