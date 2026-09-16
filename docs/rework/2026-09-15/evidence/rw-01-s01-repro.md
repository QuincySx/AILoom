# RW-01 · S01 反例复现（2026-09-15，ZCode，基线 c4a56a4 工作区）

真实 CLI（target/debug/ailoom），场景：HOME=/tmp/rw01-repro/home；
阶段 1 以 AILOOM_DATA_ROOT=<home>/.ailoom、AILOOM_STORE_ROOT=<home>/.ailoom/store
模拟升级前旧版部署（XDG 未设）；wsA/wsB 两个工作区 init+sync（claude target）。

- 阶段 1 结果：wsA/.claude/skills/common-greet → `<legacy>/store/bG9j…/common-greet`
  绝对符号链接；SKILL.md（140 字节）与 references/checklist.md 可读；
  `<legacy>/device-id` = 5120ac7a-6dff-449c-8eb2-0186436c9b8c。
- 升级：清空 AILOOM_*/XDG_* 仅留 HOME，在 wsA 运行 `ailoom status`（只读）触发默认根迁移。
- 迁移后（修复前）与审查 S01 一致：
  1. 旧绝对链接断裂：`cat wsA/.claude/skills/common-greet/SKILL.md` → No such file or directory；
     status 输出自带 `! [skill-store-link] Skill 链接异常: .claude/skills/common-greet#symlink
     (store-target-missing:<legacy>/store/…)`。store 内容已出现在
     `~/.local/share/ailoom/store/`，但未重新 sync 的工作区链接不可达。
  2. device-id 不连续：`<legacy>/device-id`=5120ac7a…（原样留在旧根），
     新数据根生成新 id=73582ee9-36c6-4f84-94c9-a8fb13297a86 —— 设备身份断裂。
  3. bin/ 原地保留（该部分既有行为正确）。
