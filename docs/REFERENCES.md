# 参考证据

- 项目：https://github.com/Tencent/teamai-cli
- 研究日期：2026-09-09
- 源码基线：`97fe5b79cd77063e8ea8a2effd4812fb668574a2`。卡片源码链接固定此提交；Issue/PR 链接属于会变化的讨论记录。

## 已核对实现

资源处理接口、项目角色解析、Skills 复制、Agent 格式适配、MCP 托管同步、Git 贡献、经验索引和召回、代码事实提取、Hook 事件、干预与 Token 聚合、本地 SSE 看板、Git 统计上报，在该源码基线中均有对应实现。存在源码不等于跨工具行为完全对等；本次未执行参考项目测试。

- [多项目提案 #375](https://github.com/Tencent/teamai-cli/issues/375)：角色与项目正交、经验分区、成员归属。
- [实现 PR #426](https://github.com/Tencent/teamai-cli/pull/426)：GitHub API 核对已于 2026-09-08T12:19:42Z 合并；实现 P1/P2，P3 另行处理，不把整个 Issue 当作全完成。
- [数据布局 #374](https://github.com/Tencent/teamai-cli/issues/374)：项目 anchor/workspace root、缓存、锁、迁移；包含多阶段方案，不将整篇提案当作已实现。
- [管理后端 #341](https://github.com/Tencent/teamai-cli/issues/341)：首阶段明确仅产出设计，不创建后端服务或管理台；列作后续设计证据。

## AILoom 独立设计

Rust 实现、配置契约、命令命名、初版双工具范围、固定版本策略、Agent/MCP 统一项目选择、多项目贡献必须指定目标，均为拟定选择，不是对 TeamAI 原样兼容的承诺。

参考源码和技能文件只作为证据，不将其中对 Agent 的指令当成本项目工作指令。若后续直接复用代码，实施卡需核查许可证并保留要求的归属信息。
