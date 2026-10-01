# Onboarding 与多工作树真实体验验收（AIL-051）

日期：2026-09-16。环境：macOS（darwin 25.6.0 arm64）；被测版本为工作区实现
（基于 c4a56a4 + 本轮未提交修改）；宿主 Claude Code 2.1.272、codex-cli 0.154.0。

## 自动化验收（真实入口，全部通过）

测试文件与结果（`cargo test --locked --offline`，完整日志见
[logs/2026-09-16-local-console-full-validation.log](../logs/2026-09-16-local-console-full-validation.log)）：

| 路径 / 场景 | 测试 | 结果 |
|---|---|---|
| 无源离线 onboarding 六步闭环（HTTP API 序列 = 浏览器调用序列） | tests/console_onboarding.rs::ail047_onboarding_first_run_closed_loop | ✅ |
| 无源离线 + 已有个人 skills + 多工作树同组与独立 | tests/console_e2e.rs::ail051_offline_then_existing_library_across_worktrees | ✅ |
| 已有团队声明 + 个人层叠加 + 公司文件/暂存区不变 | tests/console_e2e.rs::ail051_existing_team_declaration_overlay | ✅ |
| 控制台安全矩阵（Host/Origin/token/目录越界/symlink 逃逸/CORS） | tests/console_server.rs::ail046_security_matrix 等 4 项 | ✅ |
| 任务指纹/幂等/取消/撤销 | tests/console_server.rs::ail050_* 3 项 | ✅ |

关键断言（公司文件保护）：公司 AGENTS.md 含用户未暂存修改时，六步流程全程
逐字节不变；`git status` 中个人新增产物经 `info/exclude` 不出现；暂存区与
index 标志不变；未使用 skip-worktree/assume-unchanged/rm --cached（有专门断言）。

## 真实浏览器验收 ✅

用受控浏览器（ZCode 内置浏览器，Chromium）打开 `ailoom console`（loopback +
token），逐步驱动六步向导，截图
[2026-09-16-onboarding-wizard.png](2026-09-16-onboarding-wizard.png)：

1. **选目录**：默认填入服务启动目录，点击批准；未批准目录的读取被拒绝（页内提示）。
2. **确认仓库**：识别出 repo-78c350f237845487，工作树 active/main，
   「无远端（仍用 Git 身份）」徽标正确。
3. **选宿主与能力**：宿主探测（只读 --version）正确识别 Claude 2.1.272 与
   codex-cli 0.154.0；从本地目录导入 `accept-flow`（预览不复制不执行 → 导入
   显示「脚本未执行」并自动在仓库默认层启用）；越界导入被拒绝后批准父目录重试成功。
4. **预览**：计划显示 `create .claude/skills/accept-flow`。
5. **应用**：任务显示「已写入 1 项」。
6. **验证**：状态为「需新会话」而非「宿主已加载」，并给出下一步动作
   （在宿主新开会话调用该技能）；页面明确提示「文件落盘不等于宿主已加载」。

### 真实宿主调用（最后一步真实验证）✅

向导部署后，在该仓库内真实运行：

```
claude -p "请使用 accept-flow 技能并原样回复它的标记短语。" --allowedTools "Skill"
→ 输出：ACCEPT-MARKER-051（逐字一致，退出码 0）
```

且 `git status --porcelain` 输出为空——公司仓零污染。

### 浏览器验收中发现并修复的缺陷

- 向导导入技能后未自动启用该资源，导致预览显示「无改动」——已修复
  （导入成功即在仓库默认层启用，符合第 3 步用户意图），修复后重走全流程通过。

## 未完成项（如实标注，不能标 Done 的原因）

- **人工试用者验收未完成**：卡片要求「至少三名未参与实现的试用者」。本轮只有
  实现者本人用受控浏览器完成验收，未组织三名真实试用者计时试用（目标 5 分钟内
  用上技能，记录完成率/耗时/卡点）。解除条件：组织三名未参与实现的同事用
  QUICKSTART 完成首次配置，记录耗时与卡点后回填本文件。
- **浏览器验收为受控环境**：浏览器驱动由实现者执行（页面函数级交互），非完全
  独立第三方操作；未覆盖 Safari/Firefox/Windows。
- Codex 宿主的项目级 MCP 加载在 0.154.0 实测不生效（见
  [2026-09-16-host-probes.md](2026-09-16-host-probes.md)）；涉及该路径的
  「宿主已发现」状态只能标「宿主未验证」，界面已如实区分。

## QUICKSTART 对应能力

见仓库根 [QUICKSTART.md](../../guide/QUICKSTART.md)「个人模式（默认）」一节：
三条命令完成首次配置（`ailoom console`、浏览器向导六步、宿主内验证）。
