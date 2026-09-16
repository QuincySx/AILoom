//! 控制台前端入口（AIL-080）：只负责安全 bootstrap —— HTML 壳 + 会话令牌注入 +
//! ES module 应用加载。令牌仅进入内存边界（window.__AILOOM），不写 localStorage、
//! 不进日志。具体页面/组件/状态在 ui/ 模块内（见 console/ui.rs 资产表）。

pub fn index_html(token: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="zh"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>AILoom 本地控制台</title>
<link rel="stylesheet" href="/ui/tokens.css">
<link rel="stylesheet" href="/ui/base.css">
</head><body>
<h1>AILoom 本地控制台 <span class="muted">仅本机可访问</span></h1>
<div id="conflictBar"></div>
<div id="toast"></div>
<script>window.__AILOOM = {{ token: {token:?} }};</script>
<script type="module" src="/ui/app.js"></script>
</body></html>"#,
        token = token
    )
}
