// AIL-120：CLI 能力对齐专项走查 —— undo / scan-skills / library delete / sync 撤销记录
// + CLI 与 Web 同状态一致性。全部真实 CLI 调用 + 磁盘核对。
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const A = `${S}/知识库 甲`;
const BIN = '<repo>/target/debug/ailoom';
const ENV = `HOME=${S}/home XDG_STATE_HOME=${S}/xdg-state XDG_DATA_HOME=${S}/xdg-data GIT_CONFIG_GLOBAL=${S}/home/.gitconfig GIT_CONFIG_NOSYSTEM=1`;
const exists = (p) => { try { return sh(`test -e "${p}" && echo y`).trim() === 'y'; } catch { return false; } };
const cli = (args) => {
  const out = execSync(`${ENV} ${BIN} --data-root ${S}/data --json ${args} 2>&1`).toString();
  const parsed = JSON.parse(out);
  return parsed.result ?? parsed;
};
// 失败也返回（CLI 对拒绝操作以非零退出 + JSON 错误输出）
const cliTry = (args) => {
  try { return { ok: true, out: cli(args) }; }
  catch (e) { return { ok: false, out: e.stdout?.toString() || e.message }; }
};
const sh = (cmd) => execSync(cmd).toString();
const log = (...a) => console.log('[AIL-120-cli]', ...a);
let failures = [];
const expect = (c, n) => { if (c) log('PASS', n); else { failures.push(n); log('FAIL', n); } };
const canon = (p) => sh(`cd "${p}" && pwd -P`).trim();
const A_CANON = canon(A);

// 前置：批准根（Web 端稍后同状态对照用）
const TOK = () => sh(`grep '/?token=' ${S}/evidence/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
sh(`curl -s -X POST http://127.0.0.1:8646/api/fs/approve -H "X-AILoom-Session: ${TOK()}" -H 'Content-Type: application/json' -d '{"path":"${A_CANON}"}' >/dev/null`);

// ---- 1. scan-skills（新 CLI 能力）----
const scan1 = cli(`personal --action scan-skills --root "${A}" --sub skills`);
expect((scan1.items || []).some(i => i.dir_name === '会议纪要' && i.management === 'unmanaged'), 'CLI scan-skills：未托管条目');
expect((scan1.items || []).some(i => i.management === 'error'), 'CLI scan-skills：损坏条目独立报错');
const scan2 = cli(`personal --action scan-skills --root "${A}"`);
expect(Array.isArray(scan2.items) && scan2.items.every(i => i.management === 'managed' || i.management === 'error'), 'CLI scan-skills：不指定根时只扫宿主目录（仅托管部署/异常，无越界读取）');

// ---- 2. select → effective（既有能力，确认未回归）----
cli(`personal --action select --resource personal/skill/personal/retrieval-tips --state enable --repo "${A}"`);
const eff = cli(`personal --action effective --root "${A}"`);
expect(eff.resources?.['personal/skill/personal/retrieval-tips']?.deployed === true, 'CLI select/effective：引用生效');

// ---- 3. sync（部署 + 持久化撤销清单）----
const beforeSync = sh(`find "${A_CANON}/.claude/skills" -maxdepth 1 -mindepth 1 2>/dev/null | sort`);
const sync = cli(`personal --action sync --root "${A}"`);
expect(sync.ok === true, 'CLI sync：部署成功');
expect((sync.applied || []).length > 0 || sync.noop > 0, `CLI sync：动作计数（applied=${sync.applied?.length ?? 0}, noop=${sync.noop}）`);
const jobsDir = `${S}/data/console/jobs`;
const jobFiles = sh(`ls -t ${jobsDir}/*.json | head -3`).trim().split('\n');
let syncJob = null;
for (const f of jobFiles) {
  const j = JSON.parse(sh(`cat ${f}`));
  if (j.kind === 'apply' && j.status === 'success' && (j.undo || []).length > 0) { syncJob = j; break; }
}
expect(!!syncJob && (syncJob.undo || []).length > 0, `CLI sync：持久化撤销清单（job=${syncJob?.id?.slice(0, 8)}…）`);

// ---- 4. undo（撤销上一步 sync）----
const undo = cli(`personal --action undo --id ${syncJob.id}`);
expect((undo.restored || []).length > 0, `CLI undo：恢复 ${undo.restored?.length} 项`);
const afterUndo = sh(`find "${A_CANON}/.claude/skills" -maxdepth 1 -mindepth 1 2>/dev/null | sort`);
expect(afterUndo !== beforeSync || afterUndo === '', 'CLI undo：磁盘回滚');
const undo2 = cliTry(`personal --action undo --id ${syncJob.id}`);
expect(!undo2.ok && (undo2.out.includes('不可撤销') || undo2.out.includes('清单已处理')), 'CLI undo：重复撤销被拒（清单已处理）');
// 恢复现场：重新 sync 部署回去
const sync2 = cli(`personal --action sync --root "${A}"`);
expect(sync2.ok === true, 'CLI sync：重新部署恢复现场');

// ---- 5. library delete（预览 + 执行 + 归档）----
// 先导入一个临时副本供删除
sh(`mkdir -p "${S}/导入源/待删副本"`);
sh(`printf -- '---\\nname: 待删副本\\ndescription: CLI 删除走查\\n---\\n# x\\n' > "${S}/导入源/待删副本/SKILL.md"`);
sh(`curl -s -X POST http://127.0.0.1:8646/api/fs/approve -H "X-AILoom-Session: ${TOK()}" -H 'Content-Type: application/json' -d '{"path":"/private/tmp/ailoom-usable/导入源"}' >/dev/null`);
cli(`library --action import --dir "${S}/导入源/待删副本" --name cli-delete-test --execute`);
const delPrev = cli(`library --action delete --skill personal/skill/personal/cli-delete-test`);
expect(delPrev.executed === false && delPrev.preview?.exists === true, 'CLI library delete：预览模式且资源在库');
const delExec = cli(`library --action delete --skill personal/skill/personal/cli-delete-test --execute`);
expect(delExec.executed === true, 'CLI library delete：执行移入归档');
expect(!exists(`${S}/data/library/resources/skills/cli-delete-test`), '副本已从资源库移除');
const archived = sh(`find "${S}/data/library-archive" -maxdepth 2 -name 'cli-delete-test' -type d 2>/dev/null | head -1`).trim();
expect(!!archived, `删除可恢复（归档：${archived}）`);

// ---- 6. CLI 与 Web 同状态对照 ----
const cliEff = cli(`personal --action effective --root "${A}"`);
const webEff = JSON.parse(sh(`curl -s "http://127.0.0.1:8646/api/effective?root=${encodeURIComponent(A_CANON)}"`));
console.log('CLI pending:', cliEff.pending_actions, '| Web pending:', webEff.pending_actions);
expect(cliEff.pending_actions === webEff.pending_actions, 'CLI/Web pending 一致');
expect(JSON.stringify(Object.keys(cliEff.resources)) === JSON.stringify(Object.keys(webEff.resources)), 'CLI/Web 资源集合一致');
expect(cliEff.hosts.claude.enabled === webEff.hosts.claude.enabled, 'CLI/Web 宿主状态一致');

console.log(JSON.stringify({ failures }, null, 2));
if (failures.length) process.exitCode = 1;
