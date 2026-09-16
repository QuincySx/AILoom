# RW-07 · R03 修复与验证（2026-09-15，ZCode）

session_state 旧实现忽略传入 events、仅按 stop_count>0 判 idle——Stop 后继续
输入仍永久 idle（与审查 R03 一致）。

修复：状态按该会话（完整身份 workspace+device+tool+session，RW-06）最新生命
周期事件判定：stop→idle、prompt/session-start→running、无→unknown。
aggregate::parse_time 改 pub 供排序复用。

验证（tests/dashboard_reporting.rs 新增 dashboard_state_follows_latest_
lifecycle_event，真实 CLI hook 注入 + /api/state）：
- s-board：session-start→stop→UserPromptSubmit → state=running（不再永久 idle）；
- s2：仅 Stop → idle；
- s3：仅 SessionStart → running。
既有 dashboard 用例（session-start→stop → idle）继续通过：13 passed。

## 补充：SSE 与重连（卡面第 1/3 项）

新增 dashboard_sse_pushes_state_transitions_without_page_refresh（socket 级）：
- 初始快照（无论 Last-Event-ID）→ sse1=idle；
- 独立进程 hook 注入 UserPromptSubmit → SSE 500ms 轮询指纹变化后推送 running
  （页面不刷新）；再注入 Stop → 推送 idle；
- 断线重连：新连接立即获得全量快照（最新状态），服务器正常回收旧连接线程。
浏览器端交互证据（EventSource 页面行为）属上一轮真机验收记录（快照归档），
本轮以 socket 级测试验证传输与状态语义；两者分开记录。

事件重复说明：Stop 事件重复投递（同内容 payload）经 event_id 去重不追加、
不触发推送（去重语义正确）；测试注入以内容 hint 区分真实不同事件。
