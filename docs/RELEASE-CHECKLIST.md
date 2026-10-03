# 发布检查单（RELEASE-CHECKLIST）

> 每次发版前逐项核对。

## 0. 版本

- [ ] `Cargo.toml` 的 `version` 已更新（`Cargo.lock` 只应改这一行，锁文件格式不要跟着新 cargo 升级）
- [ ] `CHANGELOG.md` 有该版本的段落，写用户能感知的新增、修复与已知限制；发布流水线以它作为 Release 说明，缺失时拒绝发布
- [ ] 打签名 tag：`git tag -s v<版本> -m "v<版本>"`
- [ ] 发布流水线（`.github/workflows/release.yml`，手动触发）：默认把 `ailoom-cli`（内含 5 个平台的二进制）发布到 npm（不发 GitHub Release）；取消勾选 `publish` 只构建。npm 上已有的版本会跳过，重跑不会重复发布
- [ ] `packaging/npm/package.json` 的 `version` 与 `Cargo.toml` 一致（`tests/installer.rs` 断言；流水线发布时也会写入）

## 1. 质量门

- [ ] `cargo fmt --all -- --check` 通过
- [ ] `cargo clippy --all-targets -- -D warnings` 通过
- [ ] `cargo test` 全部通过（含 tests/e2e.rs 端到端验收）
- [ ] 端到端验收（AIL-023）在目标平台跑通并记录输出摘要
- [ ] 浏览器验收：`scripts/ui-browser.sh` 全部模式 PASS（CI `browser` job；本机运行需要 Chrome），日志与截图在 `target/ui-browser/`
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
- [ ] 升级安装：网页服务在运行时，安装后自动重启为新版本（`AILOOM_INSTALL_RESTART=0` 可关闭）；`tests/installer.rs` 覆盖
- [ ] macOS 二进制签名与公证：需要 Apple Developer ID 证书与公证账号（尚未配置）；未签名的下载版会被 Gatekeeper 拦截，源码安装不受影响
- [ ] 制品无开发机路径/秘密；hash 校验清单随包发布
- [ ] npm 一次性配置：npmjs.com → `ailoom-cli` → Settings → Trusted Publisher 选 GitHub Actions，填 `QuincySx` / `AILoom` / `release.yml`；发布不需要令牌
- [ ] 发布后验证：`npm i -g ailoom-cli && ailoom version` 输出新版本

## 5. 文档

- [ ] `docs/guide/INSTALL.md` 的安装、升级、数据位置、卸载与诊断说明与实际一致
- [ ] 仓库根目录有 `LICENSE`；第三方 Skill 不提交进仓库（见 `.ailoom/team/THIRD_PARTY.md`）

- [ ] docs/QUICKSTART.md、docs/SUPPORT.md 与实际命令行一致（help 输出比对）
- [ ] docs/CONTRACTS.md 变更记录已更新；受影响卡片已同步
- [ ] README.md 入口链接有效
