# RW-10 · R07 反例复现与修复验证（2026-09-15，ZCode）

## 修复前（基线 c4a56a4 代码，静态+单测确认）

`mod a { fn helper() {} } mod b { fn helper() {} }` 同处 src/lib.rs 时，
两个符号均生成 `fn:helper@src/lib.rs`（旧 id 不含作用域）——索引互覆盖、
查询只出一个、file:line 混淆。与审查 R07 一致。
（旧实现下同场景 build 后 query helper 仅返回 1 个符号；此处由旧 id 规则
`fn:{name}@{rel}` 直接可证，单元矩阵同构。）

## 修复后（真实 CLI）

工作区绑定 + 业务源 src/lib.rs 含 a/b 两个模块各一个 helper：
- `ailoom code --action build`：symbols=4（mod a/b + 2 helper），parsed_ok=1。
- `ailoom code --action query --query helper` 返回两个独立符号：
  - fn:a::helper@src/lib.rs  src/lib.rs:3
  - fn:b::helper@src/lib.rs  src/lib.rs:7

## 机制与迁移行为

- 符号身份加入嵌套 mod 链：`fn:<scope::name>@<rel>`（struct/enum/trait/mod 同规则）；
  contains 边的 from 指向内层 mod 身份。
- 限定调用解析：调用点 mod 链由内向外查同名 fn，命中 → to 为完整身份、
  confidence=ast-scoped（召回权重 0.9）；未命中保持名字层
  （name-based-project / name-based 低置信），歧义不静默选错。
- GRAPH_SCHEMA_VERSION 2→3：旧图 schema 不符 → build 全量重建（新身份），
  增量/删除行为（按文件剔除+覆盖，旧边随文件事实替换清除）由既有
  incremental_update_equivalent_to_full_and_no_dangling_edges 用例继续守护。

回归：cargo test --test code_graph → 6 passed（含新增
same_file_nested_module_symbols_get_scoped_identities）。
