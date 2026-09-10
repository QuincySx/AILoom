---
name: ailoom-share-learning
description: 总结会话中的可复用经验并生成可审阅的贡献草稿（AILoom 内置）
---

## 何时使用

会话中出现了：被验证的修复方案、踩坑记录、团队约定；且用户表达了"记住/分享/沉淀"意图时。

## 工作流程

1. 从会话中提炼：标题（一句话）、正文（问题→根因→解决步骤，≤300 字）、出处引用（如有）。
2. **先生成草稿文件**（如 `.ailoom/drafts/learning-<主题>.md`，frontmatter 含 title），展示给用户审阅。
3. 用户确认后，运行：
   `ailoom contribute --file .ailoom/drafts/learning-<主题>.md`
   - 单项目工作区会自动归属该项目；多项目时命令会要求显式 `--project <id>` 或 `--shared`。
4. 报告贡献分支与审核方式。绝不自动推送主分支，绝不提交未经用户审阅的内容。

## 边界

- 不上传完整会话记录；只提炼经验要点。
- 秘密、凭据、机器路径不得写入经验正文。
- 本 Skill 是软约束；手动入口：手写 Markdown 后运行 `ailoom contribute --file <文档>`。
