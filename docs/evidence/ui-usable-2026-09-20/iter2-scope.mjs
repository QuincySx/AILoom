// UX 成熟度第 2 轮验收：四档作用域（项目默认 / 项目默认·子目录 / 工作树 / 工作树·子目录）
// 真实 UI + API + 磁盘。主对象：Git 项目丙 + 子目录 web（继承后单独微调场景）。
// 前置：reset115.sh && node ail115.mjs（保证 retrieval-tips 已入库、甲乙丙已登记）。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const C = `${S}/项目 丙`, A = `${S}/知识库 甲`;
const EV = `${S}/evidence`;
const b = await connect('http://127.0.0.1:8646/');
const log = (...a) => console.log('[iter2-scope]', ...a);
const shot = n => b.screenshot(`${EV}/ux-iter/iter2-${n}.png`);
const sh = cmd => execSync(cmd).toString();
let failures = [];
const expect = (c, n) => { if (c) log('PASS', n); else { failures.push(n); log('FAIL', n); } };
const api = p => JSON.parse(sh(`curl -s "http://127.0.0.1:8646${p}"`));
const cid = api('/api/state').repos.find(r => (r.common_dir || '').includes('项目 丙')).repo_id;
execSync(`mkdir -p "${C}/web"`);
// 可重复执行：清掉上次运行留在丙/web 子目录层的设置与部署产物
const TOK = () => sh(`grep '/?token=' ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
const POST = (p, body) => sh(`curl -s -X POST http://127.0.0.1:8646${p} -H "X-AILoom-Session: ${TOK()}" -H 'Content-Type: application/json' -d '${body}'`);
POST('/api/profile/select', JSON.stringify({resource:'personal/skill/personal/retrieval-tips', state:'inherit', root:C, worktree:true, subproject:'web'}));
POST('/api/profile/select', JSON.stringify({host:'claude', state:'inherit', root:C, worktree:true, subproject:'web'}));
POST('/api/profile/select', JSON.stringify({host:'claude', state:'inherit', root:C, worktree:true}));
execSync(`rm -rf "${C}/web/.claude" "${C}/.claude"`);

await b.waitFor('!!document.querySelector("#nav")');
await b.goto('#/projects/' + encodeURIComponent(cid));
await b.waitFor('!!document.querySelector("[data-scope]")', 15000);

// ---- 1. 四档下拉与层级说明行 ----
const opts = await b.evaluate(`[...document.querySelectorAll('[data-scope] option')].map(o=>o.value).join(',')`);
expect(opts === 'repo,worktree,wt-sub', '视角下拉三档：项目默认/当前工作树/工作树·子目录');
const hint0 = await b.evaluate(`document.querySelector('[data-scope-hint]')?.textContent || ''`);
expect(hint0.includes('项目默认') && hint0.includes('继承'), '说明行：默认层解释继承关系');

// ---- 1b. 项目默认层启用 claude 宿主（repo 视角：select+保存）----
await b.evaluate(`document.querySelector('[data-tab="0"]').click()`);
await b.waitFor('!!document.querySelector("[data-entries] .project-row")', 15000);
await b.evaluate(`(async()=>{ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.querySelector('.row-head strong')?.textContent==='Claude Code');
  row.querySelector('select').value='enable'; [...row.querySelectorAll('button')].find(x=>x.textContent==='保存').click(); })()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存')`);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-add]")', 15000);

// ---- 2. 切到「仅当前工作树 · 指定子目录」并填 web ----
await b.evaluate(`(function(){ const sel=document.querySelector('[data-scope]'); sel.value='wt-sub'; sel.dispatchEvent(new Event('change')); })()`);
await new Promise(r => setTimeout(r, 400));
const subVisible = await b.evaluate(`!document.querySelector('[data-sub-wrap]')?.hidden`);
expect(subVisible, '选择子目录档后出现子目录输入');
await b.evaluate(`(function(){ const i=document.querySelector('[data-sub-scope]'); i.value='web'; i.dispatchEvent(new Event('input')); i.dispatchEvent(new Event('change')); })()`);
await new Promise(r => setTimeout(r, 500));
const hint1 = await b.evaluate(`document.querySelector('[data-scope-hint]')?.textContent || ''`);
expect(hint1.includes('子目录 web') && hint1.includes('只影响这个子目录'), '说明行：子目录层含义与继承链');
await shot('01-subscope-ui');

// ---- 3. 在子目录层添加 retrieval-tips（宿主已在夹具建立时于项目默认层启用）----
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-add]")', 15000);
for (let i = 0; i < 8; i++) {
  await b.evaluate(`document.querySelector('[data-add]')?.click()`);
  try { await b.waitFor(`[...document.querySelectorAll('[data-picker-list]')].some(el => el.closest('dialog')?.matches(':modal'))`, 2500); break; } catch (e) {}
}
await b.evaluate(`(function(){ const box=[...document.querySelectorAll('[data-picker-item]')].find(x=>x.value.includes('retrieval-tips') && !x.disabled); box.checked=true; box.dispatchEvent(new Event('change')); })()`);
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存 1 条引用')`, 15000);
const savedMsg = await b.evaluate(`document.querySelector('[data-message]').textContent`);
expect(savedMsg.includes('子目录 web'), '保存反馈标明子目录层');
await shot('02-saved-at-subscope');

// ---- 4. API 双读：父级未启用 / 子目录启用 ----
const effParent = api(`/api/effective?root=${encodeURIComponent(C)}`);
const effSub = api(`/api/effective?root=${encodeURIComponent(C)}&scope=web`);
const resParent = Object.entries(effParent.resources || {}).find(([id]) => id.includes('retrieval-tips'))?.[1];
const resSub = Object.entries(effSub.resources || {}).find(([id]) => id.includes('retrieval-tips'))?.[1];
expect(!resParent?.deployed || resParent?.enabled !== true, '父级（工作树根）不受子目录设置影响');
expect(resSub && (resSub.enabled === true || resSub.deployed === true), 'API effective（subproject=web）：子目录内启用');

// ---- 5. 预览并应用：文件只落在 web/ 下，不落在项目根 ----
await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='生成预览')");
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='生成预览').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)", 20000);
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用' && !b.disabled).click()`);
await b.waitFor("[...document.querySelectorAll('dialog:modal .dialog-actions button')].some(x=>x.textContent==='确认应用')");
await b.evaluate(`[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(x=>x.textContent==='确认应用').click()`);
await b.waitFor(`document.querySelector('[data-content]')?.textContent.includes('应用完成')`, 25000);
await shot('03-applied');
const diskSub = sh(`find "${C}/web/.claude/skills" -maxdepth 1 -mindepth 1 -name 'retrieval-tips' 2>/dev/null | head -1`).trim();
const diskRoot = sh(`find "${C}/.claude/skills" -maxdepth 1 -mindepth 1 2>/dev/null | head -1`).trim();
expect(diskSub !== '', '磁盘：web/ 子目录出现托管 Skill');
expect(diskRoot === '', '磁盘：项目根没有托管文件（微调不外溢）');

// ---- 6. 移除（子目录层）→ 应用 → 恢复继承 ----
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-entries] .project-row")', 15000);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('项目默认未启用')`, 15000);
expect(true, '工作树视角：子目录添加项标明「项目默认未启用」');
await b.evaluate(`(function(){ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.textContent.includes('retrieval-tips'));
  [...row.querySelectorAll('button')].find(x=>x.textContent==='恢复跟随默认').click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('恢复跟随项目默认'))`, 15000);
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='恢复跟随默认').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已恢复跟随默认')`, 15000);
await shot('04-removed');
const effSub2 = api(`/api/effective?root=${encodeURIComponent(C)}&scope=web`);
const resSub2 = Object.entries(effSub2.resources || {}).find(([id]) => id.includes('retrieval-tips'))?.[1];
expect(!resSub2 || (resSub2.enabled !== true && resSub2.deployed !== true), '移除后子目录恢复继承（不再启用）');

// ---- 7. 甲（文件夹项目）不受影响 ----
const effA = api(`/api/effective?root=${encodeURIComponent(A)}`);
expect(effA && effA.repo_id, '甲状态可读（隔离）');
expect(!sh(`find "${A}/web" -maxdepth 0 2>/dev/null | head -1`).trim(), '甲目录没有 web 泄漏');

console.log(JSON.stringify({ failures }));
process.exit(failures.length ? 1 : 0);
