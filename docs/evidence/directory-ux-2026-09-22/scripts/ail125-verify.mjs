// AIL-125 验收：当前目标预览应用与结果闭环（UI 主路径）。
// 1) 成功应用+重新查询；2) noop 不可应用；3) 旧计划失效：预览后改配置 →
//    UI 应用显示「计划已过期」且零写入；4) 切目标后旧计划不可应用；
//    5) 撤销遇用户后改 → 冲突保留不覆盖。
import { connect, Checks, apiCall } from './cdp.mjs';
import { execSync } from 'node:child_process';
const c = await connect();
const ck = new Checks('AIL-125');
const OUT = '/tmp/ailoom-dirux/evidence';
const ROOT = '/tmp/ailoom-dirux/repos/proj-bing';
const PID = 'repo-fabb1cfe5c90ca62';
const DS = 'collection-e9bea2ba014f8cd3/skill/common/doc-search';
const DOC = ROOT + '/web/.claude/skills/doc-search/SKILL.md';
const sh = cmd => { try { return execSync(cmd, { encoding: 'utf8' }).trim(); } catch { return '(absent)'; } };
const confirmTop = async label => {
  await c.waitFor(`[...document.querySelectorAll('dialog[open] button')].some(b=>b.textContent.includes(${JSON.stringify(label)}))`, 5000);
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes(${JSON.stringify(label)})).click()`);
};
const switchToWeb = async () => {
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-tree]')`, 8000);
  await c.evaluate(`document.querySelector('[data-twist="web"]')?.click()`);
  await c.waitFor(`document.querySelector('.dir-row[data-rel="web"]')`, 5000);
  await c.evaluate(`document.querySelector('.dir-row[data-rel="web"]').click()`);
  await c.evaluate(`document.querySelector('[data-dir-use]').click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')?.textContent==='web'`, 8000);
};
const gotoPreview = async () => {
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='预览与应用').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app button')].some(b=>b.textContent.includes('生成预览'))`, 8000);
};
const preview = async () => {
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent.includes('生成预览')).click()`);
  await new Promise(r => setTimeout(r, 1600));
};
const applyBtnState = () => c.evaluate(`(() => { const b=[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用'); return b ? (b.disabled?'disabled':'enabled') : 'none'; })()`);
try {
  await apiCall('/fs/approve', { path: ROOT });
  await apiCall('/profile/select', { root: ROOT, host: 'claude', state: 'enable' });
  await apiCall('/profile/select', { root: ROOT, resource: DS, state: 'enable', worktree: true, subproject: 'web' });

  await c.viewport(1440, 1000);
  await c.goto('#/projects/' + PID);
  await c.waitFor(`!!document.querySelector('[data-tab]')`, 15000);
  await switchToWeb();
  await gotoPreview();

  // ── 1. 成功应用（含磁盘与状态重查）────────────────────────────
  await preview();
  ck.check('预览绑定作用域 web', await c.evaluate(`document.querySelector('[data-content]')?.textContent.includes('作用域 web')`));
  ck.check('非 noop 后应用按钮可用', (await applyBtnState()) === 'enabled');
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用').click()`);
  await confirmTop('确认应用');
  await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('应用完成')`, 30000);
  await c.screenshot(OUT + '/ail125-applied.png');
  ck.check('磁盘已写入 doc-search', sh(`ls ${ROOT}/web/.claude/skills`).includes('doc-search'));

  // ── 2. noop 不可应用 ──────────────────────────────────────────
  await preview();
  ck.check('noop 预览提示无改动', await c.evaluate(`document.querySelector('[data-content]')?.textContent.includes('无改动')`));
  ck.check('noop 后应用按钮禁用', (await applyBtnState()) === 'disabled');
  await c.screenshot(OUT + '/ail125-noop.png');

  // ── 3. 旧计划失效（UI 主路径）：预览出清理动作 → 应用前改配置 → 过期 ──
  await apiCall('/profile/select', { root: ROOT, resource: DS, state: 'inherit', worktree: true, subproject: 'web' });
  await preview(); // 计划出现清理 doc-search 的动作
  ck.check('清理计划使应用可用', (await applyBtnState()) === 'enabled');
  const docBefore = sh(`cat "${DOC}"`);
  await apiCall('/profile/select', { root: ROOT, host: 'codex', state: 'enable' }); // 预览后的配置变化
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用').click()`);
  await confirmTop('确认应用');
  await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('计划已过期') || document.querySelector('[data-content]')?.textContent.includes('应用失败')`, 20000);
  await c.screenshot(OUT + '/ail125-stale.png');
  ck.check('UI 明确提示「计划已过期」', await c.evaluate(`document.querySelector('[data-content]')?.textContent.includes('计划已过期')`));
  ck.check('过期计划零写入：doc-search 文件仍在', sh(`cat "${DOC}"`) === docBefore);
  ck.check('过期提示要求重新预览', await c.evaluate(`document.querySelector('[data-content]')?.textContent.includes('重新生成预览')`));

  // ── 4. 切目标后旧计划不可应用 ─────────────────────────────────
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`!!document.querySelector('[data-switch-dir]')`, 8000);
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-tree]')`, 8000);
  await c.evaluate(`document.querySelector('.dir-row[data-rel=""]').click()`);
  await c.evaluate(`document.querySelector('[data-dir-use]').click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')?.textContent==='根目录'`, 5000);
  await gotoPreview();
  ck.check('切换目标后应用按钮不可用（旧计划作废）', (await applyBtnState()) !== 'enabled');

  // ── 5. 撤销遇用户后改 → 冲突保留 ──────────────────────────────
  // 恢复 DS 引用，切回 web，重新预览并应用，然后手改文件，再撤销。
  await apiCall('/profile/select', { root: ROOT, resource: DS, state: 'enable', worktree: true, subproject: 'web' });
  await switchToWeb();
  await gotoPreview();
  await preview();
  ck.check('应用按钮可用（新计划）', (await applyBtnState()) === 'enabled');
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用').click()`);
  await confirmTop('确认应用');
  await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('应用完成')`, 30000);
  sh(`printf '\\n<!-- 用户手动补充的说明 -->' >> "${DOC}"`);
  ck.check('用户后改已写入部署文件', sh(`cat "${DOC}"`).includes('用户手动补充的说明'));
  await c.goto('#/tasks');
  await c.waitFor(`!!document.querySelector('[data-undo]')`, 10000);
  // 选中最新一条成功的应用记录（撤销按钮在选中后才可用）
  await c.evaluate(`(() => {
    const rows=[...document.querySelectorAll('[data-table] tbody tr')];
    const row=rows.find(r=>r.textContent.includes('成功'));
    if (row) row.click();
    return !!row;
  })()`);
  await c.waitFor(`!!document.querySelector('[data-undo]') && !document.querySelector('[data-undo]').disabled`, 8000);
  await c.evaluate(`document.querySelector('[data-undo]').click()`);
  await confirmTop('确认撤销');
  await new Promise(r => setTimeout(r, 2000));
  const afterUndo = sh(`cat "${DOC}" 2>/dev/null || echo GONE`);
  ck.check('撤销遇用户后改：保留用户修改（冲突不覆盖）', afterUndo.includes('用户手动补充的说明'));
  await c.screenshot(OUT + '/ail125-undo-conflict.png');
  ck.finish();
} finally { await c.close(); }
