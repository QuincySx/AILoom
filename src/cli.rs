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
    /// 输出版本与构建信息
    Version,
    /// 团队资源源脚手架：生成 ailoom.toml + resources 骨架
    Source {
        /// 生成目录
        #[arg(long)]
        dir: std::path::PathBuf,
        /// team_id（[a-z0-9-]{1,64}）
        #[arg(long, default_value = "my-team")]
        team_id: String,
        /// 声明的逻辑项目（可重复，默认 a）
        #[arg(long = "project")]
        projects: Vec<String>,
        /// 声明的职能角色（可重复，默认 dev）
        #[arg(long = "role")]
        roles: Vec<String>,
        /// 只生成最小骨架（不带示例资源）
        #[arg(long)]
        minimal: bool,
        /// 允许合并进已有团队源目录
        #[arg(long)]
        force: bool,
        /// 目录还不是 Git 仓库时执行 git init + 初始提交
        #[arg(long)]
        git: bool,
    },
    /// 绑定团队源与项目/角色，生成本机锁
    Init {
        /// 初始化项目知识库位置（不要求先绑定团队资源源）
        #[arg(long)]
        knowledge_path: Option<std::path::PathBuf>,
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
        /// 启用的 AI 工具（可重复：claude/codex/grok/pi/opencode/cursor；出现即整体替换）
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
    /// 展示工作区绑定与声明/锁一致性
    Status {
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 预览同步差异（无写入）
    Plan {
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 执行同步计划
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
    /// Hook 事件采集：stdin payload → 标准事件（由宿主调用）
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
    /// Hook 注册管理：install | remove | exec
    Hooks {
        /// 动作：install | remove | exec
        #[arg(long, value_parser = ["install", "remove", "exec"])]
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
    /// 会话指标与摘要：metrics | summary | ingest
    Session {
        /// 动作：metrics | summary | ingest
        #[arg(long, value_parser = ["metrics", "summary", "ingest"])]
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
    /// 迁移独立源到同仓模式（copy→校验→切换→备份）
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
    /// 批量导入目录文档为知识
    Import {
        /// 目标项目
        #[arg(long)]
        project: Option<String>,
        /// 导入源（Markdown 目录或本地 Git 仓库）
        #[arg(long)]
        dir: std::path::PathBuf,
        /// 显式仓库列表文件（每行一个目录/仓库路径；# 注释）
        #[arg(long)]
        repo_list: Option<std::path::PathBuf>,
        /// 目标：shared 或 project:<id>
        #[arg(long)]
        target: String,
        /// 资源类型：learning | doc
        #[arg(long, default_value = "doc", value_parser = ["learning", "doc"])]
        kind: String,
        /// 预览后确认执行
        #[arg(long)]
        execute: bool,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// PR/MR 知识候选（显式指定 URL）
    Pr {
        /// 动作：draft
        #[arg(long, default_value = "draft", value_parser = ["draft"])]
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
    /// 数据留存与清理：rotate | export | cleanup
    Data {
        /// 动作：rotate | export | cleanup
        #[arg(long, value_parser = ["rotate", "export", "cleanup"])]
        action: String,
        /// export 目标目录
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// rotate 阈值（MB，需大于 0）
        #[arg(long, default_value = "10", value_parser = positive_f64)]
        max_size_mb: f64,
        /// 预览清理项
        #[arg(long)]
        dry_run: bool,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 成员名册：list | projects | register | remove
    Members {
        /// 动作
        #[arg(long, value_parser = ["list", "projects", "register", "remove"])]
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
    /// 团队包依赖：check | install
    Packages {
        /// 动作：check | install
        #[arg(long, value_parser = ["check", "install"])]
        action: String,
        /// install 需显式确认
        #[arg(long)]
        yes: bool,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 代码事实与图谱：build | query（Rust 优先）
    Code {
        /// 动作：build | query
        #[arg(long, value_parser = ["build", "query"])]
        action: String,
        /// 查询关键词
        #[arg(long)]
        query: Option<String>,
        /// 图扩展跳数（0-2）
        #[arg(long, default_value = "1", value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(0..=2))]
        hops: usize,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// Hook 事件实时看板（仅 loopback；独立于网页控制台，日常管理用 ailoom web）
    Dashboard {
        /// 监听端口
        #[arg(long, default_value = "7777")]
        port: u16,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 团队统计上报与汇总：push | retry | digest | status
    Report {
        /// 动作：push | retry | digest | status
        #[arg(long, value_parser = ["push", "retry", "digest", "status"])]
        action: String,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 项目知识库与知识维护：status | init | save | recall | sync | move | …（详见 --action）
    Knowledge {
        /// 动作：status | init | checkpoint | recover | clone | move | save | recall | sync；或 feedback | maintenance | promote | archive | restore
        #[arg(long, value_parser = ["status", "init", "checkpoint", "recover", "clone", "move", "configure", "save", "recall", "sync", "feedback", "maintenance", "promote", "archive", "restore"])]
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
        /// 稳定反馈事件身份（同 id 重试幂等）
        #[arg(long)]
        feedback_id: Option<String>,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// 初始化位置或迁移目标（可为独立仓库子目录）
        #[arg(long)]
        path: Option<std::path::PathBuf>,
        /// 确认预览的指纹；move 默认仅预览
        #[arg(long)]
        expected: Option<String>,
        #[arg(long)]
        execute: bool,
        /// 迁移成功后同步到已配置的远端
        #[arg(long)]
        sync_after: bool,
        /// Git 同步地址；空字符串关闭远端同步
        #[arg(long)]
        remote: Option<String>,
        #[arg(long)]
        branch: Option<String>,
        /// 远端知识分支内的子目录
        #[arg(long)]
        subdir: Option<String>,
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// 检索本地知识索引
    Recall {
        /// 查询关键词（支持中文）
        #[arg(long = "query", short = 'q')]
        query: String,
        /// 过滤资源类型：learning/rule/doc/skill
        #[arg(long, value_parser = ["learning", "rule", "doc", "skill"])]
        kind: Option<String>,
        /// 返回条数上限（1-100）
        #[arg(long, default_value = "10", value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=100))]
        limit: usize,
        /// 强制重建索引
        #[arg(long)]
        rebuild: bool,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 贡献经验文档
    Contribute {
        /// 经验 Markdown 文档（frontmatter 含 title）
        #[arg(long)]
        file: std::path::PathBuf,
        /// 归属项目（仅团队源 PR 流程；项目知识库模式下不需要）
        #[arg(long)]
        project: Option<String>,
        /// 显式共享给全团队（仅团队源 PR 流程）
        #[arg(long)]
        shared: bool,
        /// namespace，缺省按清单推导（仅团队源 PR 流程）
        #[arg(long)]
        namespace: Option<String>,
        /// 提交信息（仅团队源 PR 流程）
        #[arg(long, default_value = "ailoom: 新增经验")]
        message: String,
        /// PR 创建方式（仅团队源 PR 流程）
        #[arg(long, default_value = "auto", value_parser = ["auto", "manual"])]
        provider: String,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 同仓贡献：把当前子树资源改动提交到隔离分支
    ContributeSelf {
        /// 提交信息
        #[arg(long, default_value = "ailoom: 同仓资源贡献")]
        message: String,
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// 贡献资源修改：从修改过的源克隆建立变更集并推送
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
    /// 资源库：init | import | import-git | import-entry | list | sources | check-update | update | delete | recover
    Library {
        /// 动作：init | import | import-git | import-entry | list | sources | check-update | update | delete | recover
        #[arg(long, value_parser = ["init", "import", "import-git", "import-entry", "list", "sources", "check-update", "update", "delete", "recover"])]
        action: String,
        /// import-git / import-entry 的仓库 URL 或发现入口（skills.sh/…）
        #[arg(long)]
        url: Option<String>,
        /// import-entry：发现入口（等价 --url，语义更明确）
        #[arg(long)]
        entry: Option<String>,
        /// import-git 的仓库内 skill 子目录
        #[arg(long)]
        path: Option<String>,
        /// import-git 的 ref（分支/标签/commit）
        #[arg(long)]
        git_ref: Option<String>,
        /// check-update / update / delete 的 skill 名（delete 也可写完整资源 ID）
        #[arg(long)]
        skill: Option<String>,
        /// update：只应用这次检查的候选版本
        #[arg(long)]
        preview_id: Option<String>,
        /// import 的技能目录
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
        /// import 重命名
        #[arg(long)]
        name: Option<String>,
        /// import 预览后确认执行（默认只预览）
        #[arg(long)]
        execute: bool,
    },
    /// 个人模式：effective | select | instructions | plan | sync | recover | deploy-status | undo | scan-skills | migrate-nongit
    Personal {
        /// 动作：effective | select | instructions | plan | sync | recover | deploy-status | undo | scan-skills | migrate-nongit
        #[arg(long, value_parser = ["effective", "select", "instructions", "plan", "sync", "recover", "deploy-status", "undo", "scan-skills", "migrate-nongit"])]
        action: String,
        /// undo 的任务 ID（apply/sync 任务持久化后的 id）
        #[arg(long)]
        id: Option<String>,
        /// scan-skills 的 Skill 根相对路径（可选，例如 skills）
        #[arg(long)]
        sub: Option<String>,
        /// 完整资源 ID（source/kind/namespace/name）
        #[arg(long)]
        resource: Option<String>,
        /// 宿主名（claude/codex/…）
        #[arg(long)]
        host: Option<String>,
        /// 三态：enable | disable | inherit
        #[arg(long)]
        state: Option<String>,
        /// 子项目相对路径
        #[arg(long)]
        subproject: Option<String>,
        /// 作用到当前 Worktree（默认仓库层）
        #[arg(long)]
        worktree: bool,
        /// instructions：从文件读取个人指令
        #[arg(long)]
        file: Option<std::path::PathBuf>,
        /// instructions：清除条目
        #[arg(long)]
        clear: bool,
        /// plan/sync/effective 的显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// plan/sync/effective 的作用域相对路径
        #[arg(long)]
        scope: Option<String>,
        /// select 的显式仓库/Worktree 根（不用 profile 键序猜目标仓库）
        #[arg(long)]
        repo: Option<std::path::PathBuf>,
    },
    /// 资源合集：list | preview | apply | check | remove（添加/更新不启用资源）
    Collection {
        #[arg(long, default_value = "list", value_parser = ["list", "preview", "apply", "check", "remove"])]
        action: String,
        /// preview：合集显示名（带 --source 更新时可省略，沿用登记值）
        #[arg(long)]
        name: Option<String>,
        /// preview：合集仓库地址（带 --source 更新时可省略，沿用登记值）
        #[arg(long)]
        url: Option<String>,
        /// preview：分支或标签，默认远端默认分支
        #[arg(long = "ref")]
        ref_: Option<String>,
        /// 更新已有合集的 source ID
        #[arg(long)]
        source: Option<String>,
        /// apply 使用之前预览返回的 ID
        #[arg(long)]
        preview_id: Option<String>,
        /// remove 默认仅预览；传此参数才移除来源登记（保留历史实体）
        #[arg(long)]
        execute: bool,
    },
    /// 前台运行网页控制台（调试用；日常请用 ailoom web）
    #[command(hide = true)]
    Console {
        /// 监听端口（占用时自动向后寻找可用端口）
        #[arg(long, default_value_t = crate::console::DEFAULT_PORT)]
        port: u16,
        /// 不自动打开浏览器
        #[arg(long)]
        no_open: bool,
    },
    /// 打开网页；自动启动或复用独立的后台服务
    Web {
        /// 端口（默认 47831）；已有服务在运行时沿用其端口
        #[arg(long)]
        port: Option<u16>,
        /// 只输出访问地址，不打开浏览器
        #[arg(long)]
        no_open: bool,
    },
    /// 管理网页后台（普通 CLI 命令不依赖此服务）
    Service {
        #[command(subcommand)]
        action: ServiceCommand,
    },
    /// 工作区体检
    Doctor {
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// 有失败项时以退出码 10 结束（供 CI / 脚本做门禁）；报告照常输出
        #[arg(long)]
        strict: bool,
    },
    /// 按托管清单安全卸载
    Uninstall {
        /// 显式工作区根
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// 预览后确认执行
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServiceCommand {
    /// 启动后台服务，已运行则复用
    Start {
        /// 端口（默认 47831）；已有服务在运行时沿用其端口
        #[arg(long)]
        port: Option<u16>,
    },
    /// 等待现有任务完成后停止服务，不关闭登录自启动
    Stop,
    /// 查看运行状态与登录自启动设置
    Status,
    /// 开启下次登录自启动；不改变当前运行状态
    Enable {
        #[arg(long, default_value_t = crate::console::DEFAULT_PORT)]
        port: u16,
    },
    /// 关闭登录自启动；不停止当前服务
    Disable,
    /// 前台运行，供系统服务管理器调用
    #[command(hide = true)]
    Run {
        #[arg(long, default_value_t = crate::console::DEFAULT_PORT)]
        port: u16,
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

/// 正数参数（如 rotate 阈值）：拒绝 0、负数与非数字。
fn positive_f64(raw: &str) -> Result<f64, String> {
    match raw.parse::<f64>() {
        Ok(v) if v > 0.0 && v.is_finite() => Ok(v),
        _ => Err(format!("需要大于 0 的数字，实际为 {raw}")),
    }
}
