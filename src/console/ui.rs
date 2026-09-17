//! UI 静态资产（AIL-080）：固定映射的嵌入式模块表，include_str! 编译期嵌入，
//! 按 MIME 返回。禁止把任意磁盘路径映射成静态文件——只服务本表列出的资产。

pub const TOKENS_CSS: &str = include_str!("ui/tokens.css");
pub const BASE_CSS: &str = include_str!("ui/base.css");

pub const API_JS: &str = include_str!("ui/services/api.js");
pub const STORE_JS: &str = include_str!("ui/state/store.js");
pub const TARGET_JS: &str = include_str!("ui/state/target.js");
pub const DRAFT_JS: &str = include_str!("ui/state/draft.js");
pub const JOBS_JS: &str = include_str!("ui/state/jobs.js");

pub const BUTTON_JS: &str = include_str!("ui/components/button.js");
pub const FIELD_JS: &str = include_str!("ui/components/field.js");
pub const TRI_STATE_JS: &str = include_str!("ui/components/triState.js");
pub const BADGE_JS: &str = include_str!("ui/components/badge.js");
pub const TABLE_JS: &str = include_str!("ui/components/dataTable.js");
pub const DIALOG_JS: &str = include_str!("ui/components/dialog.js");
pub const EDITOR_JS: &str = include_str!("ui/components/editor.js");
pub const DIFF_JS: &str = include_str!("ui/components/diffView.js");
pub const CONFLICT_JS: &str = include_str!("ui/components/conflictPanel.js");

pub const SCOPE_PICKER_JS: &str = include_str!("ui/features/scopePicker.js");
pub const CAPABILITY_MATRIX_JS: &str = include_str!("ui/features/capabilityMatrix.js");
pub const COLLECTIONS_PANEL_JS: &str = include_str!("ui/features/collectionsPanel.js");
pub const IMPORT_PREVIEW_JS: &str = include_str!("ui/features/importPreview.js");
pub const WORKFLOW_LIST_JS: &str = include_str!("ui/features/workflowStageList.js");
pub const PLAN_PREVIEW_JS: &str = include_str!("ui/features/planPreview.js");
pub const INSTRUCTIONS_JS: &str = include_str!("ui/features/instructionsPanel.js");

pub const APP_JS: &str = include_str!("ui/app.js");
pub const PAGE_ONBOARDING_JS: &str = include_str!("ui/pages/onboarding.js");
pub const PAGE_SCOPES_JS: &str = include_str!("ui/pages/scopes.js");
pub const PAGE_LIBRARY_JS: &str = include_str!("ui/pages/library.js");
pub const PAGE_SOURCES_JS: &str = include_str!("ui/pages/sources.js");
pub const PAGE_WORKFLOWS_JS: &str = include_str!("ui/pages/workflows.js");
pub const PAGE_TASKS_JS: &str = include_str!("ui/pages/tasks.js");
pub const PAGE_INSTRUCTIONS_JS: &str = include_str!("ui/pages/instructions.js");
pub const PAGE_OVERVIEW_JS: &str = include_str!("ui/pages/overview.js");
pub const PAGE_SAMPLES_JS: &str = include_str!("ui/pages/samples.js");

/// 固定资产表：路径 → (内容, MIME)。
pub fn lookup(path: &str) -> Option<(&'static str, &'static str)> {
    let assets: &[(&str, (&str, &str))] = &[
        (
            "/ui/pages/projects.js",
            (
                include_str!("ui/pages/projects.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/theme.css",
            (include_str!("ui/theme.css"), "text/css; charset=utf-8"),
        ),
        (
            "/ui/theme.js",
            (
                include_str!("ui/theme.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/collectionsPanel.js",
            (COLLECTIONS_PANEL_JS, "text/javascript; charset=utf-8"),
        ),
        ("/ui/tokens.css", (TOKENS_CSS, "text/css; charset=utf-8")),
        ("/ui/base.css", (BASE_CSS, "text/css; charset=utf-8")),
        (
            "/ui/services/api.js",
            (API_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/state/store.js",
            (STORE_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/state/target.js",
            (TARGET_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/state/draft.js",
            (DRAFT_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/state/jobs.js",
            (JOBS_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/components/button.js",
            (BUTTON_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/components/field.js",
            (FIELD_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/components/triState.js",
            (TRI_STATE_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/components/badge.js",
            (BADGE_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/components/dataTable.js",
            (TABLE_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/components/dialog.js",
            (DIALOG_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/components/editor.js",
            (EDITOR_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/components/diffView.js",
            (DIFF_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/components/conflictPanel.js",
            (CONFLICT_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/features/scopePicker.js",
            (SCOPE_PICKER_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/features/capabilityMatrix.js",
            (CAPABILITY_MATRIX_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/features/importPreview.js",
            (IMPORT_PREVIEW_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/features/workflowStageList.js",
            (WORKFLOW_LIST_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/features/planPreview.js",
            (PLAN_PREVIEW_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/features/instructionsPanel.js",
            (INSTRUCTIONS_JS, "text/javascript; charset=utf-8"),
        ),
        ("/ui/app.js", (APP_JS, "text/javascript; charset=utf-8")),
        (
            "/ui/pages/onboarding.js",
            (PAGE_ONBOARDING_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/scopes.js",
            (PAGE_SCOPES_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/library.js",
            (PAGE_LIBRARY_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/sources.js",
            (PAGE_SOURCES_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/workflows.js",
            (PAGE_WORKFLOWS_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/tasks.js",
            (PAGE_TASKS_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/instructions.js",
            (PAGE_INSTRUCTIONS_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/overview.js",
            (PAGE_OVERVIEW_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/samples.js",
            (PAGE_SAMPLES_JS, "text/javascript; charset=utf-8"),
        ),
    ];
    assets.iter().find(|(p, _)| *p == path).map(|(_, v)| *v)
}
