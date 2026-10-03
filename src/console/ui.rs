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

pub const COLLECTIONS_PANEL_JS: &str = include_str!("ui/features/collectionsPanel.js");
pub const INSTRUCTIONS_JS: &str = include_str!("ui/features/instructionsPanel.js");

pub const APP_JS: &str = include_str!("ui/app.js");
pub const PAGE_ONBOARDING_JS: &str = include_str!("ui/pages/onboarding.js");
pub const PAGE_LIBRARY_JS: &str = include_str!("ui/pages/library.js");
pub const PAGE_TASKS_JS: &str = include_str!("ui/pages/tasks.js");
pub const PAGE_SAMPLES_JS: &str = include_str!("ui/pages/samples.js");

/// 固定资产表：路径 → (内容, MIME)。
pub fn lookup(path: &str) -> Option<(&'static str, &'static str)> {
    let assets: &[(&str, (&str, &str))] = &[
        (
            "/ui/components/pathText.js",
            (
                include_str!("ui/components/pathText.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/projectDialog.js",
            (
                include_str!("ui/features/projectDialog.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/skillActions.js",
            (
                include_str!("ui/features/skillActions.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/nativeFiles.js",
            (
                include_str!("ui/features/nativeFiles.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/globalSkills.js",
            (
                include_str!("ui/features/globalSkills.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/pages/nativeFiles.js",
            (
                include_str!("ui/pages/nativeFiles.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/knowledgeRecovery.js",
            (
                include_str!("ui/features/knowledgeRecovery.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/knowledgePanel.js",
            (
                include_str!("ui/features/knowledgePanel.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/workspace.css",
            (include_str!("ui/workspace.css"), "text/css; charset=utf-8"),
        ),
        (
            "/ui/pages/workspace.js",
            (
                include_str!("ui/pages/workspace.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/resourceReferences.js",
            (
                include_str!("ui/features/resourceReferences.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/managedDefinition.js",
            (
                include_str!("ui/features/managedDefinition.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/resourcePicker.js",
            (
                include_str!("ui/features/resourcePicker.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/directoryPicker.js",
            (
                include_str!("ui/features/directoryPicker.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/pages/projectSettings.js",
            (
                include_str!("ui/pages/projectSettings.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/importDialog.js",
            (
                include_str!("ui/features/importDialog.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/features/ccSwitchImport.js",
            (
                include_str!("ui/features/ccSwitchImport.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/components/select.js",
            (
                include_str!("ui/components/select.js"),
                "text/javascript; charset=utf-8",
            ),
        ),
        (
            "/ui/components.css",
            (include_str!("ui/components.css"), "text/css; charset=utf-8"),
        ),
        (
            "/ui/pages/projects.js",
            (
                include_str!("ui/pages/projects.js"),
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
            "/ui/features/instructionsPanel.js",
            (INSTRUCTIONS_JS, "text/javascript; charset=utf-8"),
        ),
        ("/ui/app.js", (APP_JS, "text/javascript; charset=utf-8")),
        (
            "/ui/pages/onboarding.js",
            (PAGE_ONBOARDING_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/library.js",
            (PAGE_LIBRARY_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/tasks.js",
            (PAGE_TASKS_JS, "text/javascript; charset=utf-8"),
        ),
        (
            "/ui/pages/samples.js",
            (PAGE_SAMPLES_JS, "text/javascript; charset=utf-8"),
        ),
    ];
    assets.iter().find(|(p, _)| *p == path).map(|(_, v)| *v)
}
