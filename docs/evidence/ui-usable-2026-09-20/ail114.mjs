// AIL-114：引用写入/移除/范围/应用结果正确性 —— F04~F08；自带清理，可重复执行。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const A = `${S}/知识库 甲`, C = `${S}/项目 丙`;
const EV = `${S}/evidence`;
const b = await connect('http://127.0.0.1:8646/');
const log = (...a) => console.log('[AIL-114]', ...a);
const shot = n => b.screenshot(`${EV}/ail114-${n}.png`);
const sh = cmd => execSync(cmd).toString();
const api = p => JSON.parse(sh(`curl -s "http://127.0.0.1:8646${p}"`));
const TOK = () => sh(`grep "/?token=" ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
const POST = (p, body) => sh(`curl -s -X POST http://127.0.0.1:8646${p} -H "X-AILoom-Session: ${TOK()}" -H 'Content-Type: application/json' -d '${body}'`);
let failures = [];
const expect = (c, n) => { if (c) log('PASS', n); else { failures.push(n); log('FAIL', n); } };
// 渲染频繁：点击「添加」后若弹窗未开（节点被重渲染替换）则重试
async function openPicker(retries = 6) {
  for (let i = 0; i < retries; i++) {
    await b.evaluate(`document.querySelector('[data-add]')?.click()`);
    try { await b.waitFor(`[...document.querySelectorAll('[data-picker-list]')].some(el => el.closest('dialog')?.matches(':modal'))`, 2500); return; }
    catch (e) { /* maybe stale node; retry */ }
  }
  log('诊断 add 存在:', await b.evaluate(`!!document.querySelector('[data-add]')`));
  log('诊断 dialogs:', JSON.stringify(await b.evaluate(`[...document.querySelectorAll('dialog')].filter(d=>d.open).map(d=>d.querySelector('h2')?.textContent)`)));
  log('诊断 tab:', await b.evaluate(`[...document.querySelectorAll('[data-tab]')].findIndex(b=>b.classList.contains('on'))`));
  log('诊断 message:', await b.evaluate(`document.querySelector('[data-message]')?.textContent`));
  throw new Error('选择器始终未打开');
}

await b.waitFor('!!document.querySelector("#nav")');
const nongitId = sh(`ls ${S}/data/repos | grep nongit- | head -1`).trim();
const gitId = sh(`ls ${S}/data/repos | grep '^repo-' | head -1`).trim();
for (const p of ['/private/tmp/ailoom-usable/知识库 甲', '/private/tmp/ailoom-usable/知识库 乙', '/private/tmp/ailoom-usable/项目 丙']) {
  POST('/api/fs/approve', JSON.stringify({ path: p }));
}

// ---------- 清理前奏（真实服务调用）：清除引用/副本并应用一次，让流程从已知状态开始 ----------
for (const res of ['retrieval-tips', 'term-table', 'meeting-notes']) {
  POST('/api/profile/select', JSON.stringify({ root: A, resource: `personal/skill/personal/${res}`, state: 'inherit' }));
  POST('/api/profile/select', JSON.stringify({ root: C, resource: `personal/skill/personal/${res}`, state: 'inherit' }));
  // 层级模型后残留可能位于工作树/子目录层：逐层复位
  POST('/api/profile/select', JSON.stringify({ root: C, resource: `personal/skill/personal/${res}`, state: 'inherit', worktree: true }));
}
POST('/api/profile/select', JSON.stringify({ root: A, host: 'codex', state: 'inherit' })); // 复位宿主，保证 Part 2 的不一致场景
for (const name of ['term-table', 'meeting-notes']) {
  POST('/api/library/delete', JSON.stringify({ id: `personal/skill/personal/${name}`, execute: true }));
}
async function apiApply(root) {
  const plan = JSON.parse(POST('/api/jobs/plan', JSON.stringify({ root })));
  const done = await new Promise(resolve => {
    const t0 = Date.now();
    const poll = () => {
      const j = api(`/api/jobs/${plan.job_id}`);
      if (j.status === 'success' || j.status === 'failed') resolve(j); else if (Date.now() - t0 > 30000) resolve(j); else setTimeout(poll, 300);
    };
    poll();
  });
  if (plan.job_id) {
    const apply = JSON.parse(POST('/api/jobs/apply', JSON.stringify({ plan_job_id: plan.job_id })));
    await new Promise(resolve => {
      const t0 = Date.now();
      const poll = () => {
        const j = api(`/api/jobs/${apply.job_id}`);
        if (j.status === 'success' || j.status === 'failed' || j.status === 'stale-plan') resolve(j); else if (Date.now() - t0 > 30000) resolve(j); else setTimeout(poll, 300);
      };
      poll();
    });
  }
}
await apiApply('/private/tmp/ailoom-usable/知识库 甲');
await apiApply('/private/tmp/ailoom-usable/项目 丙'); // 丙同样从已知状态开始（上轮可能留下 meeting-notes）
sh(`rm -rf "${A}/.claude/skills" "${C}/.claude/skills" "${C}/web/.claude" 2>/dev/null`); // apply 后手工兜底清理测试实体（仅沙盒夹具）
expect(!sh(`find "${A}/.claude/skills" -maxdepth 1 -mindepth 1 2>/dev/null | head -1`).trim(), '清理后甲目录无托管 Skill 实体');

// ---------- Part 1 · F07：应用成功必须有明确结果（UI 重新添加引用 → 预览 → 应用）----------
await b.goto('#/projects/' + encodeURIComponent(nongitId));
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-add]")');
await openPicker();
await b.evaluate(`(function(){ const box=[...document.querySelectorAll('[data-picker-item]')].find(b=>b.value.includes('retrieval-tips')); box.checked=true; box.dispatchEvent(new Event('change')); })()`);
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存 1 条引用')`, 15000);
await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='生成预览')");
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='生成预览').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)", 20000);
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用' && !b.disabled).click()`);
await b.waitFor("[...document.querySelectorAll('dialog:modal .dialog-actions button')].some(x=>x.textContent==='确认应用')");
await b.evaluate(`[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(x=>x.textContent==='确认应用').click()`);
await b.waitFor(`document.querySelector('[data-content]').textContent.includes('应用完成')`, 20000);
await shot('01-apply-success-panel');
const panelText = await b.evaluate(`document.querySelector('[data-content]').textContent`);
expect(panelText.includes('应用完成：写入'), 'F07：应用完成在面板明确呈现');
expect(panelText.includes('retrieval-tips'), 'F07：显示写入路径');
expect(panelText.includes('操作记录') && panelText.includes('新会话'), 'F07：给出撤销入口与待验证说明');
const skillLink = sh(`find "${A}/.claude/skills" -maxdepth 1 -mindepth 1 2>/dev/null | head -1`).trim();
expect(!!skillLink, `apply 后磁盘出现托管实体：${skillLink}`);

// ---------- Part 2 · F08：双宿主状态不一致要如实显示 ----------
await b.evaluate(`document.querySelector('[data-tab="0"]').click()`);
await b.waitFor('!!document.querySelector("[data-entries]")');
await b.evaluate(`(async()=>{ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.querySelector('.row-head strong')?.textContent==='Codex CLI');
  row.querySelector('select').value='enable'; [...row.querySelectorAll('button')].find(x=>x.textContent==='保存').click(); })()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存')`);
// 等保存后的重渲染彻底结束（codex 行的本层设置为 enable）
await b.waitFor(`(function(){ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.querySelector('.row-head strong')?.textContent==='Codex CLI'); return row && row.querySelector('select').value==='enable'; })()`, 15000);
await new Promise(r => setTimeout(r, 600));
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('部署不一致')`, 15000);
await b.waitFor(`!!document.querySelector('[data-entries]')`);
const rowText1 = await b.evaluate(`document.querySelector('[data-entries]')?.textContent ?? ''`);
expect(rowText1.includes('部署不一致') && rowText1.includes('codex'), 'F08：claude 最新+codex 未部署 → 显示不一致，不称「已部署一致」');
await shot('02-mixed-hosts-status');

// ---------- Part 3 · F08：状态查询失败显示未知+重试 ----------
await b.evaluate(`(function(){ window.__origFetch = window.fetch; window.fetch = function(u,o){
  if (String(u).includes('/api/deploy-status')) return Promise.resolve(new Response(JSON.stringify({error:'注入故障：deploy-status 不可用'}), {status:500, headers:{'Content-Type':'application/json'}}));
  return window.__origFetch.apply(this, arguments); }; })()`);
await b.evaluate(`document.querySelector('[data-tab="0"]').click()`);
await b.waitFor('!!document.querySelector("[data-entries]")');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('未知')`, 15000);
const unknownText = await b.evaluate(`document.querySelector('[data-entries]')?.textContent ?? ''`);
expect(unknownText.includes('磁盘状态未知'), 'F08：查询失败显示未知而非「未部署」');
expect(!!await b.evaluate(`document.querySelector('[data-deploy-retry]')`), 'F08：未知状态提供重试');
await b.evaluate(`window.fetch = window.__origFetch`);
await b.evaluate(`document.querySelector('[data-deploy-retry]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('磁盘：')`, 15000);
await new Promise(r => setTimeout(r, 300));
expect(!await b.evaluate(`document.querySelector('[data-entries]')?.textContent.includes('未知') ?? true`), 'F08：重试后恢复真实状态');
await shot('03-unknown-recovered');

// ---------- Part 4 · F05：批量部分失败如实报告，重试只补失败项 ----------
const B2 = `${S}/知识库 乙/skills/术语表`, M = `${S}/知识库 甲/skills/会议纪要`;
POST('/api/library/import', JSON.stringify({ dir: B2, name: 'term-table', execute: true }));
POST('/api/library/import', JSON.stringify({ dir: M, name: 'meeting-notes', execute: true }));
expect((api('/api/resources').entries || []).filter(e => ['term-table','meeting-notes'].includes(e.name)).length === 2, '夹具：两个新 Skill 已真实入库');

// API 导入发生在页面外：切换页签强制重渲染，让选择器拿到最新资源列表
await b.evaluate(`document.querySelector('[data-tab="0"]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('Claude Code')`, 15000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('retrieval-tips')`, 15000);

await openPicker();
await b.evaluate(`(function(){ for (const box of document.querySelectorAll('[data-picker-item]')) {
    if (box.value.includes('term-table') || box.value.includes('meeting-notes')) { box.checked = true; box.dispatchEvent(new Event('change')); }
  } })()`);
await b.evaluate(`(function(){ window.fetch = function(u,o){
  try { if (String(u).includes('/api/profile/select') && String(o?.body).includes('meeting-notes')) return Promise.resolve(new Response(JSON.stringify({error:'注入故障：第二项保存失败'}), {status:500, headers:{'Content-Type':'application/json'}})); } catch(e) {}
  return window.__origFetch.apply(this, arguments); }; })()`);
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
try {
  await b.waitFor(`document.querySelector('[data-picker-error]')?.textContent.includes('已保存 1 项；1 项失败')`, 15000);
} catch (e) {
  log('诊断 picker-error:', await b.evaluate(`document.querySelector('[data-picker-error]')?.textContent`));
  log('诊断 items:', JSON.stringify(await b.evaluate(`[...document.querySelectorAll('[data-picker-item]')].map(b=>({v:b.value,checked:b.checked,disabled:b.disabled}))`)));
  log('诊断 dialogs:', JSON.stringify(await b.evaluate(`[...document.querySelectorAll('dialog')].filter(d=>d.open).map(d=>d.querySelector('h2')?.textContent)`)));
  log('诊断 message:', await b.evaluate(`document.querySelector('[data-message]')?.textContent`));
  throw e;
}
await shot('04-partial-failure-reported');
const errText = await b.evaluate(`document.querySelector('[data-picker-error]').textContent`);
expect(errText.includes('meeting-notes'), 'F05：失败项单独列出');
const stillChecked = await b.evaluate(`document.querySelector('[data-picker-item]:checked')?.value || ''`);
expect(stillChecked.includes('meeting-notes'), 'F05：仅失败项保留勾选');
const prof1 = sh(`cat ${S}/data/profile/profile.toml`);
expect(prof1.includes('term-table') && !prof1.includes('meeting-notes'), 'F05：成功项已写入，失败项零写入（非全批零写入）');
await b.evaluate(`window.fetch = window.__origFetch`);
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('meeting-notes')`, 20000);
expect(true, 'F05：重试仅补失败项后全部完成');
const prof2 = sh(`cat ${S}/data/profile/profile.toml`);
expect(prof2.includes('meeting-notes'), 'F05：重试后失败项写入');

// ---------- Part 5 · F04：本层显式停用不应被选择器当作「已添加」禁选 ----------
await b.waitFor(`!!document.querySelector('[data-entries]')`);
await b.evaluate(`(async()=>{ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.textContent.includes('term-table'));
  row.querySelector('select').value='disable'; [...row.querySelectorAll('button')].find(x=>x.textContent==='保存').click(); })()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存')`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('term-table')`, 15000);
await openPicker();
const termBox = await b.evaluate(`(function(){ const box=[...document.querySelectorAll('[data-picker-item]')].find(b=>b.value.includes('term-table')); return {disabled: box.disabled, label: box.closest('label').textContent.includes('已添加')}; })()`);
expect(!termBox.disabled && !termBox.label, 'F04：本层「停用」不被误判为已添加禁选');
await b.evaluate(`(function(){ const box=[...document.querySelectorAll('[data-picker-item]')].find(b=>b.value.includes('term-table')); box.checked=true; box.dispatchEvent(new Event('change')); })()`);
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存 1 条引用')`, 15000);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('retrieval-tips')`, 15000);
expect(true, 'F04：通过选择器把停用项改回启用');

// ---------- Part 6 · F04：移除=两种语义；停用+apply 清理磁盘 ----------
await b.waitFor(`(function(){ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.textContent.includes('term-table')); return !!row && [...row.querySelectorAll('button')].some(x=>x.textContent==='从本项目移除…'); })()`, 15000);
await b.evaluate(`(function(){ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.textContent.includes('term-table'));
  [...row.querySelectorAll('button')].find(x=>x.textContent==='从本项目移除…').click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('清除本层设置'))`);
await shot('05-remove-dialog-choices');
const dlgText = await b.evaluate(`[...document.querySelectorAll('dialog[open]')].map(d=>d.textContent).join('')`);
expect(dlgText.includes('清除后没有任何层表态'), 'F04：无上层表态时如实说明「应用后清理」');
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='在本范围停用').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已在本范围停用')`, 15000);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('term-table')`, 15000);
log('Part6 disable 后 profile:', JSON.stringify((sh(`cat ${S}/data/profile/profile.toml`).match(/term-table" = "\w+/) || ['(none)'])[0]));
log('Part6 disable 后 message:', await b.evaluate(`document.querySelector('[data-message]')?.textContent`));
await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='生成预览')");
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='生成预览').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)", 20000);
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用' && !b.disabled).click()`);
await b.waitFor("[...document.querySelectorAll('dialog:modal .dialog-actions button')].some(x=>x.textContent==='确认应用')");
await b.evaluate(`[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(x=>x.textContent==='确认应用').click()`);
await b.waitFor(`document.querySelector('[data-content]').textContent.includes('应用完成')`, 20000);
const termLink = sh(`find "${A}/.claude/skills" -maxdepth 1 -mindepth 1 -name 'term-table' 2>/dev/null`).trim();
expect(!termLink, 'F04：移除（停用语义）+apply 后托管文件被清理');

// ---------- Part 7 · F04：上层 enable + 本层 enable → 移除本层不清文件（Git 项目丙）----------
await b.goto('#/projects/' + encodeURIComponent(gitId));
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.waitFor('!!document.querySelector("[data-entries] .project-row")', 15000);
await b.evaluate(`(async()=>{ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.querySelector('.row-head strong')?.textContent==='Claude Code');
  row.querySelector('select').value='enable'; [...row.querySelectorAll('button')].find(x=>x.textContent==='保存').click(); })()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存')`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('已启用')`, 15000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-add]")');
await openPicker();
await b.evaluate(`(function(){ const box=[...document.querySelectorAll('[data-picker-item]')].find(b=>b.value.includes('meeting-notes')); box.checked=true; box.dispatchEvent(new Event('change')); })()`);
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存 1 条引用')`, 15000);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('meeting-notes')`, 15000);
await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='生成预览')");
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='生成预览').click()`);
// 重复执行时磁盘可能已与期望一致（noop）：视为已部署，直接进入双层移除场景
await b.waitFor(`document.querySelector('[data-content]').textContent.includes('无改动') || [...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)`, 20000);
if (!await b.evaluate(`document.querySelector('[data-content]').textContent.includes('无改动')`)) {
  await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用' && !b.disabled).click()`);
  await b.waitFor("[...document.querySelectorAll('dialog:modal .dialog-actions button')].some(x=>x.textContent==='确认应用')");
  await b.evaluate(`[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(x=>x.textContent==='确认应用').click()`);
  await b.waitFor(`document.querySelector('[data-content]').textContent.includes('应用完成')`, 20000);
}
expect(sh(`find "${C}/.claude/skills" "${C}/.agents/skills" -maxdepth 1 -name 'meeting-notes' 2>/dev/null | head -1`).trim() !== '', '丙磁盘已有 meeting-notes 部署（本层移除场景前置）');
await b.evaluate(`document.querySelector('[data-scope]').value='worktree'; document.querySelector('[data-scope]').dispatchEvent(new Event('change'))`);
await b.waitFor(`document.querySelector('[data-scope-hint]')?.textContent.includes('正在编辑')`, 15000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('meeting-notes')`, 15000);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('跟随项目默认')`, 15000);
expect(true, 'F04：工作树视角显示「跟随项目默认」的生效视图');
await b.evaluate(`(function(){ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.textContent.includes('meeting-notes'));
  [...row.querySelectorAll('button')].find(x=>x.textContent==='在此工作树移除…').click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('项目默认仍然启用'))`, 15000);
await shot('06-remove-dialog-upper-enabled');
const dlg2 = await b.evaluate(`[...document.querySelectorAll('dialog[open]')].map(d=>d.textContent).join('')`);
expect(dlg2.includes('其他工作树不受影响'), 'F04：确认窗如实说明只影响当前工作树、上层仍启用');
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='移除').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已在此处移除')`, 15000);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('已在此移除')`, 15000);
expect(await b.evaluate(`document.querySelector('[data-entries]')?.textContent.includes('项目默认仍启用')`), 'F04：行内如实说明「项目默认仍启用」');
// 新语义：「在此工作树移除」= 本工作树显式停用 → 本工作树不再部署；项目默认层不动
const effC = api(`/api/effective?root=${encodeURIComponent(C)}`);
expect(effC.resources?.['personal/skill/personal/meeting-notes']?.deployed === false, 'F04：移除后本工作树不再部署');
// 预览应产生清理动作 → 应用 → 本工作树文件被清理（其他范围不动）
await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='生成预览')");
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='生成预览').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled) || document.querySelector('[data-content]').textContent.includes('无改动')", 20000);
if (await b.evaluate(`document.querySelector('[data-content]').textContent.includes('无改动')`)) { failures.push('F04：移除后预览应为清理动作'); log('FAIL', 'F04：移除后预览应为清理动作'); }
else {
  await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用' && !b.disabled).click()`);
  await b.waitFor("[...document.querySelectorAll('dialog:modal .dialog-actions button')].some(x=>x.textContent==='确认应用')");
  await b.evaluate(`[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(x=>x.textContent==='确认应用').click()`);
  await b.waitFor(`document.querySelector('[data-content]').textContent.includes('应用完成')`, 20000);
}
expect(sh(`find "${C}/.claude/skills" -maxdepth 1 -name 'meeting-notes' 2>/dev/null | head -1`).trim() === '', 'F04：移除+应用后本工作树文件被清理');
// 恢复跟随默认（差异视图动作）→ 项目默认仍启用 → 再应用 → 文件恢复
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`[...document.querySelectorAll('[data-entries] .project-row')].some(r=>r.textContent.includes('meeting-notes'))`, 15000);
await b.evaluate(`(function(){ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.textContent.includes('meeting-notes'));
  [...row.querySelectorAll('button')].find(x=>x.textContent==='恢复跟随默认').click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('恢复跟随项目默认'))`, 15000);
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='恢复跟随默认').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已恢复跟随默认')`, 15000);
const effC2 = api(`/api/effective?root=${encodeURIComponent(C)}`);
expect(effC2.resources?.['personal/skill/personal/meeting-notes']?.deployed === true, 'F04：恢复跟随默认后回到项目默认的启用');

// ---------- Part 8 · F06：指令脏状态保护 ----------
await b.evaluate(`document.querySelector('[data-scope]').value='repo'; document.querySelector('[data-scope]').dispatchEvent(new Event('change'))`);
await b.waitFor(`document.querySelector('[data-scope-hint]')?.textContent.includes('正在编辑')`, 15000);
await b.evaluate(`document.querySelector('[data-tab="4"]').click()`);
await b.waitFor(`!!document.querySelector('[data-save]') && !document.querySelector('[data-save]').disabled`);
const scopeState = await b.evaluate(`document.querySelector('[data-scope]') ? document.querySelector('[data-scope]').disabled : 'no-select'`);
expect(scopeState === true, 'F06：指令页签禁用无效的范围切换控件');
await b.evaluate(`document.querySelector('textarea').value='丙项目的未保存草稿'; document.querySelector('textarea').dispatchEvent(new Event('input'))`);
await b.evaluate(`document.querySelector('[data-tab="0"]').click()`);
await b.waitFor(`[...document.querySelectorAll('dialog:modal')].some(d=>d.textContent.includes('放弃'))`, 15000);
await b.evaluate(`[...document.querySelectorAll('dialog:modal button')].find(x=>x.textContent==='取消').click()`);
await new Promise(r => setTimeout(r, 300));
await b.evaluate(`document.querySelector('[data-tab="4"]').click()`);
await b.waitFor(`!!document.querySelector('textarea')`);
expect(await b.evaluate(`document.querySelector('textarea').value`) === '丙项目的未保存草稿', 'F06：取消切换后草稿保留');
await shot('07-draft-protected');

// 知识库内容依旧不变
expect(sh(`find "${A}" "${S}/知识库 乙" -type f -not -path '*/.git/*' -not -path '*/.claude/*' -not -name 'AGENTS.override.md' -exec shasum -a 256 {} \\; | sort`) === sh(`cat ${EV}/pre-scan-hashes.txt`), '全程知识库文件哈希不变');

console.log(JSON.stringify({ failures }, null, 2));
await b.close();
if (failures.length) process.exitCode = 1;
