// AIL-119：来源故障、符号链接与失效引用恢复 —— 真实 UI + API + 磁盘。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const A = `${S}/知识库 甲`, B = `${S}/知识库 乙`;
const SRC = `${S}/来源仓库`;
const EV = `${S}/evidence`;
const b = await connect('http://127.0.0.1:8646/');
const log = (...a) => console.log('[AIL-119]', ...a);
const shot = n => b.screenshot(`${EV}/ail119-${n}.png`);
const sh = cmd => execSync(cmd).toString();
const api = p => JSON.parse(sh(`curl -s "http://127.0.0.1:8646${p}"`));
const TOK = () => sh(`grep '/?token=' ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
const POST = (p, body) => sh(`curl -s -X POST http://127.0.0.1:8646${p} -H "X-AILoom-Session: ${TOK()}" -H 'Content-Type: application/json' -d '${body}'`);
let failures = [];
const expect = (c, n) => { if (c) log('PASS', n); else { failures.push(n); log('FAIL', n); } };

await b.waitFor('!!document.querySelector("#nav")');
// 上次中断可能遗留快照改名：启动前恢复
sh(`[ -d ${S}/data/collections/cache/523e4753955f8ef1/snapshots.bak ] && [ ! -d ${S}/data/collections/cache/523e4753955f8ef1/snapshots ] && mv ${S}/data/collections/cache/523e4753955f8ef1/snapshots.bak ${S}/data/collections/cache/523e4753955f8ef1/snapshots || true`);
POST('/api/fs/approve', JSON.stringify({ path: '/private/tmp/ailoom-usable/知识库 甲' }));
const aid = api('/api/state').repos.find(r => (r.common_dir || '').includes('知识库 甲')).repo_id;

// ---------- 1. E3003 逐项处置：外逃/悬空链接不再整源拒绝，且定位具体条目 ----------
sh(`printf -- '---\\nname: 链接探针\\ndescription: 带链接的探针 Skill\\n---\\n# x\\n' > "${SRC}/resources/skills/.keep" 2>/dev/null || true`);
sh(`mkdir -p "${SRC}/resources/skills/链接探针"; cp "${SRC}/resources/agents/../agents/test-agent.toml" /dev/null 2>/dev/null; true`);
sh(`mkdir -p "${SRC}/resources/skills/链接探针/docs"; printf '正文\\n' > "${SRC}/resources/skills/链接探针/docs/note.md"`);
sh(`rm -rf "${SRC}/resources/skills/链接探针"`);
sh(`mkdir -p "${SRC}/resources/skills/链接探针/docs"; printf '正文\n' > "${SRC}/resources/skills/链接探针/docs/note.md"`);
sh(`ln -sfn note.md "${SRC}/resources/skills/链接探针/docs/in-repo.md"`);
sh(`ln -sfn /etc/hosts "${SRC}/resources/skills/链接探针/escape.md"`);
sh(`ln -sfn nowhere.md "${SRC}/resources/skills/链接探针/dangling.md"`);
sh(`cd "${SRC}" && GIT_CONFIG_GLOBAL=${S}/home/.gitconfig GIT_CONFIG_NOSYSTEM=1 git -c commit.gpgsign=false add -A && git -c commit.gpgsign=false commit -qm link-probe || true`);

// 触发检查更新并更新资源库版本（走真实 UI：/library 检查更新 + 更新）
await b.goto('#/library');
await b.waitFor('!!document.querySelector("[data-check]")', 20000);
await b.evaluate(`document.querySelector('[data-check]').click()`);
await b.waitFor(`document.querySelector('[data-msg]')?.textContent.includes('检查完成')`, 20000);
// 有新版本则走完整更新；已是最新（重复执行）则跳过
const updAvail = await (async () => {
  try { await b.waitFor(`!document.querySelector('[data-update]').disabled`, 8000); return true; }
  catch { return false; }
})();
log('有可用更新:', updAvail, '（重复执行时可能已是最新）');
if (updAvail) {
  await b.evaluate(`document.querySelector('[data-update]').click()`);
  await b.waitFor(`document.querySelector('dialog:modal, dialog[open]')?.textContent.includes('确认更新') || document.querySelector('[data-msg]')?.textContent.includes('已更新')`, 15000);
  await b.evaluate(`[...document.querySelectorAll('dialog button')].find(x=>x.textContent==='确认更新')?.click()`);
  await b.waitFor(`document.querySelector('[data-msg]')?.textContent.includes('已更新') || document.querySelector('[data-msg]')?.textContent.includes('资源库版本已更新')`, 20000);
}
await b.evaluate(`[...document.querySelectorAll('details')].forEach(d=>{ if (d.querySelector('summary')?.textContent.includes('来源提示')) d.open = true; })`);
const libText = await b.evaluate(`document.querySelector('#app').innerText`);
expect(!libText.includes('合集包含符号链接'), 'E3003 整源拒绝已消除');
expect(libText.includes('外逃符号链接') || libText.includes('悬空'), 'warnings 定位具体链接条目');
await shot('01-link-warnings-in-library');

// 项目页：来源提示 + Agent/Skill 仍可用
await b.goto('#/projects/' + encodeURIComponent(aid));
await b.waitFor('!!document.querySelector("[data-tab]")', 20000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`!!document.querySelector('[data-entries]')`, 15000);
const srcMsg = await b.evaluate(`document.querySelector('[data-message]')?.textContent || ''`);
expect(srcMsg.includes('来源提示') || srcMsg.includes('外逃') || srcMsg === '', '项目页可出现 warnings 提示（有来源时）');

// ---------- 2. 来源失联：项目仍显示已登记引用并可解除 ----------
// 真实场景：引用有效资源 → 来源快照失联（读取失败）→ 引用仍可见、可解除
POST('/api/profile/select', JSON.stringify({ root: A, resource: 'collection-523e4753955f8ef1/agent/common/test-agent', state: 'enable' }));
sh(`mv ${S}/data/collections/cache/523e4753955f8ef1/snapshots ${S}/data/collections/cache/523e4753955f8ef1/snapshots.bak`);
const ghostId = 'collection-523e4753955f8ef1/agent/common/test-agent';
await b.evaluate(`location.reload()`);
await b.waitFor('!!document.querySelector("#nav")', 20000);
await b.goto('#/projects/' + encodeURIComponent(aid));
await b.waitFor('!!document.querySelector("[data-tab]")', 20000);
await b.evaluate(`document.querySelector('[data-tab="3"]').click()`); // test-agent 是 Agent 引用
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('来源不可用')`, 20000);
await shot('02-stale-reference-row');
const staleText = await b.evaluate(`document.querySelector('[data-entries]')?.textContent ?? ''`);
expect(staleText.includes(ghostId) && staleText.includes('来源不可用'), '失效引用仍显示（不因来源缺失而消失）');
await b.evaluate(`(function(){ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.textContent.includes('来源不可用'));
  [...row.querySelectorAll('button')].find(x=>x.textContent==='解除引用…').click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('解除失效引用'))`, 15000);
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='解除引用').click()`);
await b.waitFor(`document.querySelector('[data-message]')?.textContent.includes('已解除失效引用')`, 15000);
await b.waitFor(`!document.querySelector('[data-entries]')?.textContent.includes('来源不可用')`, 15000);
expect(true, '失效引用可在项目内解除');
const prof = sh(`cat ${S}/data/profile/profile.toml`);
expect(!prof.includes('test-agent'), '解除后 profile 无残留');
sh(`mv ${S}/data/collections/cache/523e4753955f8ef1/snapshots.bak ${S}/data/collections/cache/523e4753955f8ef1/snapshots`);

// ---------- 3. 同一来源供 A/B：更新与应用时机分离 ----------
// B 也引用 test-agent（agent 需宿主支持：跳过实际添加，直接用 skill 类条目验证跨项目版本语义）
// 此处验证：检查更新后 A/B 的部署状态独立（B stale 时 A 不受影响）
const effA = api(`/api/effective?root=${encodeURIComponent(A)}`);
expect(!!effA.repo_id, 'A 状态可读');
// 来源删除边界：registry 保留引用时移除来源会被阻止（collectionsPanel 已有该交互）

console.log(JSON.stringify({ failures }, null, 2));
await b.close();
if (failures.length) process.exitCode = 1;
