# RW-04 · S04 反例复现（2026-09-15，ZCode，基线 c4a56a4 工作区，修复前脚本）

方法：PATH shim 注入——`mv` 包装脚本在目标为 `*.sha256` 时失败（模拟摘要切换
步骤失败；`set -e` 直接退出，trap 只清临时文件）。本地 HTTP fixture 提供已校验 v2。

初始：BIN_DIR 内 v1 binary（运行输出 installed-v1）+ v1 摘要 + ailoom → binary 链接。

结果（升级失败后）：
- binary 内容已变为 `installed-v2`（v1 丢失）；
- `ailoom` 链接 → binary（实际运行 v2）；
- 摘要文件仍是 v1 摘要 8a8436c4…，与实际安装的 v2（f580b8de…）不一致；
- 无任何回滚途径，且校验清单与制品不匹配。

与审查 S04 一致："先替换 FINAL，再移动摘要及更新链接；后两步任一步失败，
set -e 退出而 trap 只清临时文件。原链接已指向新 binary，旧版本无法回滚。"
