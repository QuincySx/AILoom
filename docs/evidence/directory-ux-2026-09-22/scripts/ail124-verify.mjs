// AIL-124 验收：项目共享设置与 Markdown 指令范围分离。
// 1) 目录页 → 共享设置独立路由；2) Markdown 只在共享页编辑且明确项目级；
// 3) 返回/浏览器后退恢复原目录+页签；4) 草稿保护；5) 共享改动后目录视图来源正确、工作树覆盖仍生效。
import { connect, Checks, apiCall } from './cdp.mjs';
const c = await connect();
const ck = new Checks('AIL-124');
const OUT = '/tmp/ailoom-dirux/evidence';
const ROOT = '/tmp/ailoom-dirux/repos/proj-bing';
const PID = 'repo-fabb1cfe5c90ca62';
const MN = 'collection-e9bea2ba014f8cd3/skill/common/meeting-notes';
try {
  await apiCall('/fs/approve', { path: ROOT });
  await apiCall('/profile/select', { root: ROOT, resource: MN, state: 'disable' });
  await apiCall('/profile/select', { root: ROOT, resource: MN, state: 'enable', worktree: true });

  await c.viewport(1440, 1000);
  await c.goto('#/projects/' + PID);
  await c.waitFor(`!!document.querySelector('[data-tab]')`, 15000);
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].some(r=>r.textContent.includes('meeting-notes'))`, 15000);
  // 切到 web 目录（为了验证返回恢复）
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-tree]')`, 8000);
  await c.evaluate(`document.querySelector('[data-twist="web"]')?.click()`);
  await c.waitFor(`document.querySelector('.dir-row[data-rel="web"]')`, 5000);
  await c.evaluate(`document.querySelector('.dir-row[data-rel="web"]').click()`);
  await c.evaluate(`document.querySelector('[data-dir-use]').click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')?.textContent==='web'`, 8000);

  // ── Markdown 指令页签：目录页只显示范围说明与前往链接 ──────────
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Markdown 指令').click()`);
  await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('项目级配置')`, 8000);
  ck.check('目录页 Markdown 页签不含编辑器', await c.evaluate(`!document.querySelector('[data-content] textarea') && !document.querySelector('[data-content] [contenteditable]')`));
  ck.check('目录页 Markdown 页签提供前往共享设置链接', await c.evaluate(`!!document.querySelector('[data-content] a[href$="/instructions"]')`));
  await c.screenshot(OUT + '/ail124-dir-instructions.png');

  // ── 进入项目共享设置 ──────────────────────────────────────────
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].some(r=>r.textContent.includes('meeting-notes'))`, 10000);
  await c.evaluate(`document.querySelector('[data-shared]').click()`);
  await c.waitFor(`document.querySelector('#app h1')?.textContent.includes('共享设置')`, 10000);
  await c.screenshot(OUT + '/ail124-settings.png');
  ck.check('共享设置页说明作用范围=整个项目', await c.evaluate(`document.querySelector('#app')?.textContent.includes('作用范围：整个项目')`));
  ck.check('共享设置页没有目录选择器', await c.evaluate(`!document.querySelector('[data-target-bar] [data-switch-dir]')`));
  ck.check('共享设置页有共享资源/Markdown/项目资料三个页签', await c.evaluate(`[...document.querySelectorAll('[data-tab]')].map(b=>b.textContent).join() === '共享资源,Markdown 指令,项目资料'`));

  // ── Markdown 指令：项目级编辑 + 范围文案 ──────────────────────
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Markdown 指令').click()`);
  await c.waitFor(`!!document.querySelector('[data-content] textarea, [data-content] [contenteditable]') || document.querySelector('[data-content]')?.textContent.includes('Markdown')`, 10000);
  await new Promise(r => setTimeout(r, 800));
  const scopeLine = await c.evaluate(`document.querySelector('[data-content] [data-scope-line]')?.textContent ?? ''`);
  ck.check('编辑器范围文案=整个项目', scopeLine.includes('整个项目'));
  // 编辑产生草稿
  await c.evaluate(`(() => { const ta=document.querySelector('[data-content] textarea'); if(ta){ta.value='- 个人：回答用中文（AIL-124 验收草稿）'; ta.dispatchEvent(new Event('input',{bubbles:true}));} else { const ed=document.querySelector('[data-content] [contenteditable]'); if(ed){ed.innerText='- 个人：回答用中文（AIL-124 验收草稿）'; ed.dispatchEvent(new Event('input',{bubbles:true}));} } })()`);
  await new Promise(r => setTimeout(r, 300));
  // 保存（共享层配置；待应用）
  await c.evaluate(`[...document.querySelectorAll('[data-content] button')].find(b=>b.textContent.includes('保存个人指令'))?.click()`);
  await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('已保存') || document.querySelector('[data-content]')?.textContent.includes('待应用')`, 10000);
  await c.screenshot(OUT + '/ail124-instructions-saved.png');
  ck.check('Markdown 保存提示为项目级（待应用）', await c.evaluate(`document.querySelector('[data-content]')?.textContent.includes('应用')`));

  // ── 草稿保护：再编辑 → 返回链接触发确认 → 取消留在本页 ─────────
  await c.evaluate(`(() => { const ta=document.querySelector('[data-content] textarea'); if(ta){ta.value=ta.value+'\\n第二行未保存'; ta.dispatchEvent(new Event('input',{bubbles:true}));} })()`);
  await new Promise(r => setTimeout(r, 300));
  await c.evaluate(`document.querySelector('[data-back]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] .confirmation-message')`, 5000);
  ck.check('脏稿返回弹出确认（草稿保护）', true);
  await c.screenshot(OUT + '/ail124-dirty-guard.png');
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes('取消'))?.click()`);
  await new Promise(r => setTimeout(r, 400));
  ck.check('取消后仍留在共享设置页', await c.evaluate(`location.hash.endsWith('/settings')`));
  // 放弃并返回
  await c.evaluate(`document.querySelector('[data-back]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] .confirmation-message')`, 5000);
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes('放弃并离开'))?.click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')`, 10000);
  await new Promise(r => setTimeout(r, 600));
  ck.check('返回后目录恢复为 web', await c.evaluate(`document.querySelector('[data-dir-label]')?.textContent==='web'`));
  ck.check('返回后页签恢复为 Skill', await c.evaluate(`document.querySelector('[data-tab].on')?.textContent==='Skill'`));
  await c.screenshot(OUT + '/ail124-back-restore.png');

  // 浏览器后退路径：再次进共享设置 → history.back 恢复
  await c.evaluate(`document.querySelector('[data-shared]').click()`);
  await c.waitFor(`document.querySelector('#app h1')?.textContent.includes('共享设置')`, 10000);
  await c.evaluate(`history.back()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')`, 10000);
  await new Promise(r => setTimeout(r, 600));
  ck.check('浏览器后退同样恢复 web 目录', await c.evaluate(`document.querySelector('[data-dir-label]')?.textContent==='web'`));

  // ── 共享资源改动：共享层停用 MN → 工作树覆盖仍生效 ─────────────
  // 当前：共享=disable、工作树=enable → web/根目录都应显示启用
  await apiCall('/fs/approve', { path: ROOT });
  const effWeb = await apiCall(`/effective?root=${ROOT}&scope=web`);
  ck.check('共享停用+工作树启用：web 仍启用（覆盖生效）', effWeb.resources?.[MN]?.deployed === true);
  const dirRow = await c.evaluate(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('meeting-notes'))?.textContent`);
  ck.check('目录视图来源说明=当前目录配置（工作树层）', dirRow?.includes('当前目录配置'));
  ck.finish();
} finally { await c.close(); }
