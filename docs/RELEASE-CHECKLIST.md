# 发布检查单（RELEASE-CHECKLIST）

> 初版未发布（AIL-029 决定分发方式）。本清单供未来发布前逐项勾选。

## 1. 质量门

- [ ] `cargo fmt --all -- --check` 通过
- [ ] `cargo clippy --all-targets -- -D warnings` 通过
- [ ] `cargo test` 全部通过（含 tests/e2e.rs 端到端验收）
- [ ] 端到端验收（AIL-023）在目标平台跑通并记录输出摘要
- [ ] 浏览器验收：`scripts/ui-browser.sh` 全部模式 PASS（需要本机 Chrome；CI 暂不运行），日志与截图在 `target/ui-browser/`
- [ ] `python3 scripts/docs_check.py` 与 `node --experimental-vm-modules --test tests/frontend_components.mjs` 通过（CI `frontend-and-docs` job）
- [ ] MSRV（1.85，CI `msrv` job）下 `cargo check --locked --all-targets` 通过（若变更依赖，同步更新 Cargo.toml `rust-version` 与 CI）

## 2. 宿主能力复核

- [ ] 按 docs/capabilities/ 逐行核对官方文档是否有变动（发现路径/格式/键名）
- [ ] 真实宿主（Claude Code + Codex 最新版）做一次技能/规则/Agent 加载验收，更新"实际验证"列
- [ ] 未验证条目保持标注，不得改标 supported

## 3. 内容与隐私

- [ ] 内置资源文本（src/adapters/res/）不含机器路径/秘密/未审阅承诺
- [ ] 默认不上传：事件不含 prompt 全文；共享记录仅白名单字段（tests/session_metrics.rs 断言）
- [ ] 上报默认关闭语义未回归（report push 未开启时拒绝）

## 4. 打包（AIL-029 落地后）

- [ ] 干净环境安装→version 一致→卸载不删用户配置
- [ ] 制品无开发机路径/秘密；hash 校验清单随包发布
- [ ] npm 包名可用性重新核实；未经明确发布指令不实际 `npm publish` / 推公开仓库

## 5. 文档

- [ ] docs/QUICKSTART.md、docs/SUPPORT.md 与实际命令行一致（help 输出比对）
- [ ] docs/CONTRACTS.md 变更记录已更新；受影响卡片已同步
- [ ] README.md 入口链接有效
