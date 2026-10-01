// AIL-123 验收：目录资源列表与添加/停用/恢复/移除语义。
// 链路：web 继承启用 → 在此停用（仅 web）→ 恢复继承（预览=真实 effective）→
// 添加 doc-search（仅 web）→ 预览 → 应用 → 磁盘断言（web 有、main 根/知识库乙无）→
// 移除 → 预览 → 应用 → 磁盘清理。全程 UI 真实点击 + API/文件双断言。
import { connect, Checks, apiCall } from './cdp.mjs';
import { execSync } from 'node:child_process';
const c = await connect();
const ck = new Checks('AIL-123');
const OUT = '/tmp/ailoom-dirux/evidence';
const ROOT = '/tmp/ailoom-dirux/repos/proj-bing';
const PID = 'repo-fabb1cfe5c90ca62';
const MN = 'collection-e9bea2ba014f8cd3/skill/common/meeting-notes';
const DS = 'collection-e9bea2ba014f8cd3/skill/common/doc-search';
const sh = cmd => { try { return execSync(cmd, { encoding: 'utf8' }).trim(); } catch { return '(absent)'; } };
const hashDir = p => sh(`find ${p} -type f 2>/dev/null -exec shasum {} \\; | shasum | cut -d' ' -f1`);
const rowOf = () => c.evaluate(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('meeting-notes'))?.innerText ?? '(无行)'`);
const rowOfDs = () => c.evaluate(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('doc-search'))?.innerText ?? '(无行)'`);
const clickRowAction = async (needle, label) => {
  const ok = await c.evaluate(`(() => {
    const row=[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes(${JSON.stringify(needle)}));
    const btn=[...(row?.querySelectorAll('button')??[])].find(b=>b.textContent.includes(${JSON.stringify(label)}));
    if(!btn) return false; btn.click(); return true; })()`);
  if (!ok) throw new Error(`找不到动作按钮：${needle} / ${label}`);
};
const confirmTop = async label => {
  await c.waitFor(`[...document.querySelectorAll('dialog[open] button')].some(b=>b.textContent.includes(${JSON.stringify(label)}))`, 5000);
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes(${JSON.stringify(label)})).click()`);
};
try {
  await apiCall('/fs/approve', { path: ROOT });
  await apiCall('/fs/approve', { path: '/tmp/ailoom-dirux/repos/kb-yi' });
  const baselineKbYi = hashDir('/tmp/ailoom-dirux/repos/kb-yi');
  const baselineMainRootFiles = sh(`ls ${ROOT}/.claude/skills 2>/dev/null || echo none`);
  const baselineWtB = hashDir('/private/tmp/ailoom-dirux/repos/proj-bing-wt');
  // 场景基线：项目共享=停用 MN，main 工作树=启用 MN（AIL-121 场景）；web 无本层覆盖
  await apiCall('/profile/select', { root: ROOT, resource: MN, state: 'disable' });
  await apiCall('/profile/select', { root: ROOT, resource: MN, state: 'enable', worktree: true });
  // 确保 web 无本层覆盖起点（若上轮遗留则清除）
  await apiCall('/profile/select', { root: ROOT, resource: MN, state: 'inherit', worktree: true, subproject: 'web' });
  await apiCall('/profile/select', { root: ROOT, resource: DS, state: 'inherit', worktree: true, subproject: 'web' });
  // 宿主是部署目标的前提：项目共享层启用 claude（无宿主 → 计划无产物，属正确行为）
  await apiCall('/profile/select', { root: ROOT, host: 'claude', state: 'enable' });
  const revBefore = (await apiCall('/effective?root=' + ROOT)).profile_revision;

  await c.viewport(1440, 1000);
  await c.goto('#/projects/' + PID);
  await c.waitFor(`!!document.querySelector('[data-tab]')`, 15000);
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].some(r=>r.textContent.includes('meeting-notes'))`, 15000);
  // 切到 web 目录
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-tree]')`, 8000);
  await c.evaluate(`document.querySelector('[data-twist="web"]')?.click()`);
  await c.waitFor(`document.querySelector('.dir-row[data-rel="web"]')`, 5000);
  await c.evaluate(`document.querySelector('.dir-row[data-rel="web"]').click()`);
  await c.evaluate(`document.querySelector('[data-dir-use]').click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')?.textContent==='web'`, 8000);
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].some(r=>r.textContent.includes('meeting-notes'))`, 15000);
  // 本地 Skill 扫描区应发现未托管 old-notes（真实路径）
  await c.waitFor(`[...document.querySelectorAll('#app .project-row li, #app [data-skills-scan] li')].some(li=>li.textContent.includes('old-notes')) || document.querySelector('[data-scan-status]')?.textContent.includes('1 项')`, 15000);
  const scanText = await c.evaluate(`document.querySelector('[data-skills-scan]')?.innerText`);
  ck.check('本地 Skill 区发现未托管 old-notes（真实路径）', scanText.includes('old-notes') && scanText.includes('未托管'));
  ck.check('未托管条目只有「接管说明/删除」，无更新按钮', !scanText.includes('检查更新') && scanText.includes('删除'));
  await c.screenshot(OUT + '/ail123-web-scan.png');

  // ── 在此停用（follow-on → Dialog 确认）────────────────────────
  console.log('--- web 行（继承启用）---\n' + await rowOf());
  const revPreDisable = (await apiCall('/effective?root=' + ROOT)).profile_revision;
  await clickRowAction('meeting-notes', '在此停用');
  await c.waitFor(`[...document.querySelectorAll('dialog[open] p')].some(p=>p.textContent.includes('不删除全局资源'))`, 5000);
  await c.screenshot(OUT + '/ail123-stop-dialog.png');
  ck.check('停用确认窗说明只改 web 配置', await c.evaluate(`[...document.querySelectorAll('dialog[open] p')].some(p=>p.textContent.includes('不删除全局资源，不修改'))`));
  await confirmTop('在此停用');
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('meeting-notes'))?.textContent.includes('已在此移除')`, 10000);
  console.log('--- web 行（停用后）---\n' + await rowOf());
  await c.screenshot(OUT + '/ail123-stopped.png');
  const effWeb = await apiCall(`/effective?root=${ROOT}&scope=web`);
  ck.check('API：web 停用生效（deployed=false，来源 web 层）', effWeb.resources?.[MN]?.deployed === false);
  ck.check('API：停用写在 worktree_subproject web', JSON.stringify(effWeb.resources?.[MN]?.origin) === JSON.stringify({ worktree_subproject: { path: 'web' } }));
  ck.check('API：main 根目录仍启用（不受 web 停用影响）', (await apiCall('/effective?root=' + ROOT)).resources?.[MN]?.deployed === true);
  // 注：停用“尚未部署过”的资源不产生磁盘动作，pending 横幅正确地不出现；
  // 待应用横幅在「添加 doc-search」（有真实部署动作）之后断言。

  // ── 恢复继承（预测结果 = 真实 effective：回到工作树启用）──────
  await clickRowAction('meeting-notes', '恢复继承');
  await c.waitFor(`!!document.querySelector('dialog[open] .confirmation-message')`, 5000);
  const confirmText = await c.evaluate(`document.querySelector('dialog[open] .confirmation-message')?.textContent`);
  ck.check('恢复继承确认窗预测上游=当前目录配置启用', confirmText.includes('当前目录配置') && confirmText.includes('继续参与部署'));
  await c.screenshot(OUT + '/ail123-restore-confirm.png');
  await confirmTop('恢复继承');
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('meeting-notes'))?.textContent.includes('已启用')`, 10000);
  const effWeb2 = await apiCall(`/effective?root=${ROOT}&scope=web`);
  ck.check('API：恢复继承后 web 回到启用（worktree_override）', effWeb2.resources?.[MN]?.deployed === true && effWeb2.resources?.[MN]?.origin === 'worktree_override');

  // ── 添加 doc-search 到 web（Picker Dialog）────────────────────
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent.trim()==='添加Skill')?.click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-picker-search]')`, 8000);
  await c.evaluate(`const i=document.querySelector('[data-picker-search]'); i.value='doc-search'; i.dispatchEvent(new Event('input'))`);
  await c.waitFor(`document.querySelector('[data-picker-item]')`, 5000);
  await c.screenshot(OUT + '/ail123-add-picker.png');
  await c.evaluate(`document.querySelector('[data-picker-item]').click()`);
  await c.evaluate(`document.querySelector('[data-picker-submit]').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].some(r=>r.textContent.includes('doc-search'))`, 10000);
  console.log('--- doc-search 行（添加后）---\n' + await rowOfDs());
  await c.screenshot(OUT + '/ail123-added.png');
  ck.check('doc-search 显示已启用（在此添加）', (await rowOfDs()).includes('已启用'));
  const effWeb3 = await apiCall(`/effective?root=${ROOT}&scope=web`);
  ck.check('API：web 层新增 doc-search', effWeb3.resources?.[DS]?.origin && JSON.stringify(effWeb3.resources?.[DS]?.origin) === JSON.stringify({ worktree_subproject: { path: 'web' } }));
  ck.check('添加仅保存配置：磁盘尚未出现文件', sh(`ls ${ROOT}/web/.claude/skills 2>/dev/null || echo none`).includes('old-notes') && !sh(`ls ${ROOT}/web/.claude/skills 2>/dev/null`).includes('doc-search'));
  ck.check('待应用横幅出现（有真实待部署动作）', await c.evaluate(`document.querySelector('[data-pending]')?.textContent.includes('未生效')`));

  // ── 预览 → 应用 → 磁盘断言 ───────────────────────────────────
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='预览与应用').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app button')].some(b=>b.textContent.includes('生成预览'))`, 8000);
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent.includes('生成预览')).click()`);
  await c.waitFor(`[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)`, 20000);
  await c.screenshot(OUT + '/ail123-preview.png');
  const previewText = await c.evaluate(`document.querySelector('[data-content]')?.innerText`);
  ck.check('预览列出 web 实际落盘路径', previewText.includes('web/'));
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用').click()`);
  await confirmTop('确认应用');
  await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('应用完成')`, 30000);
  await c.screenshot(OUT + '/ail123-applied.png');
  ck.check('磁盘：web/.claude/skills/doc-search 已写入', sh(`ls ${ROOT}/web/.claude/skills`).includes('doc-search'));
  ck.check('磁盘：main 根目录 .claude/skills 不受影响', sh(`ls ${ROOT}/.claude/skills 2>/dev/null || echo none`) === (baselineMainRootFiles === 'none' ? 'none' : baselineMainRootFiles) || !sh(`ls ${ROOT}/.claude/skills 2>/dev/null`).includes('doc-search'));
  ck.check('磁盘：另一工作树零变化', hashDir('/private/tmp/ailoom-dirux/repos/proj-bing-wt') === baselineWtB);
  ck.check('磁盘：另一知识库（乙）哈希不变', hashDir('/tmp/ailoom-dirux/repos/kb-yi') === baselineKbYi);
  // 成功后状态重查：Skill 行磁盘状态应显示已部署
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].some(r=>r.textContent.includes('doc-search'))`, 15000);
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('doc-search'))?.textContent.includes('已部署') || [...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('doc-search'))?.textContent.includes('未知')`, 10000);
  const dsRowAfter = await rowOfDs();
  console.log('--- doc-search 行（应用后）---\n' + dsRowAfter);
  ck.check('应用后重新查询：doc-search 行显示磁盘已部署', dsRowAfter.includes('已部署') && !dsRowAfter.includes('未部署'));

  // ── 移除 doc-search（added-here → 移除 → 预览 → 应用 → 清理）──
  await clickRowAction('doc-search', '移除');
  await confirmTop('移除');
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('doc-search'))?.textContent.includes('未启用') || true`, 10000);
  await new Promise(r => setTimeout(r, 600));
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='预览与应用').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app button')].some(b=>b.textContent.includes('生成预览'))`, 8000);
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent.includes('生成预览')).click()`);
  await c.waitFor(`[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)`, 20000);
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用').click()`);
  await confirmTop('确认应用');
  await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('应用完成')`, 30000);
  ck.check('磁盘：doc-search 已从 web 清理', !sh(`ls ${ROOT}/web/.claude/skills`).includes('doc-search'));
  ck.check('磁盘：old-notes（用户自建）未被误删', sh(`ls ${ROOT}/web/.claude/skills`).includes('old-notes'));
  ck.check('磁盘：知识库乙哈希仍不变', hashDir('/tmp/ailoom-dirux/repos/kb-yi') === baselineKbYi);
  const revAfter = (await apiCall('/effective?root=' + ROOT)).profile_revision;
  ck.check('profile revision 已推进（有写入发生）', revAfter > revPreDisable);
  ck.finish();
} finally { await c.close(); }
