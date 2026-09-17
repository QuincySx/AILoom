//! 控制台前端入口（AIL-080）：只负责安全 bootstrap —— HTML 壳 + 会话令牌注入 +
//! ES module 应用加载。令牌仅进入内存边界（window.__AILOOM），不写 localStorage、
//! 不进日志。具体页面/组件/状态在 ui/ 模块内（见 console/ui.rs 资产表）。

pub fn index_html(token: &str) -> String {
    include_str!("ui/shell.html").replace("__SESSION_TOKEN__", &format!("{token:?}"))
}
