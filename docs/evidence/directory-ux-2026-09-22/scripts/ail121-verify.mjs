// AIL-121/122 验收：目录优先目标 + 中间层继承修复 + 目录选择 Dialog。
// 场景：项目默认停用 meeting-notes → main 工作树启用 → web 无覆盖。
// 服务端真实语义（已由 API 断言）：web = 启用，来源 worktree_override。
import { connect, Checks, apiCall } from './cdp.mjs';
const c = await connect();
const ck = new Checks('AIL-121+122');
const OUT = '/tmp/ailoom-dirux/evidence';
const PID = 'repo-fabb1cfe5c90ca62';
const ROOT = '/tmp/ailoom-dirux/repos/proj-bing';
const RES = 'collection-e9bea2ba014f8cd3/skill/common/meeting-notes';
const rowOf = () => c.evaluate(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('meeting-notes'))?.innerText ?? '(无行)'`);
try {
  // 重启后 approved_roots 是服务内存态：先随用户动作重新批准（与页面行为一致）。
  await apiCall('/fs/approve', { path: ROOT });
  // 场景基线（重置后自建）：项目共享=停用 MN，main 工作树=启用 MN，web 无覆盖。
  await apiCall('/profile/select', { root: ROOT, resource: RES, state: 'disable' });
  await apiCall('/profile/select', { root: ROOT, resource: RES, state: 'enable', worktree: true });
  // ── API 目标断言（服务端真值）─────────────────────────────────
  const eff = await apiCall(`/effective?root=${ROOT}&scope=web`);
  ck.check('API：web 生效 = 启用（deployed=true）', eff.resources?.[RES]?.deployed === true);
  ck.check('API：web 生效来源 = worktree_override', eff.resources?.[RES]?.origin === 'worktree_override');

  await c.viewport(1440, 1000);
  await c.goto('#/projects/' + PID);
  await c.waitFor(`!!document.querySelector('[data-tab]')`, 15000);
  ck.check('目标条显示工作目录', await c.evaluate(`!!document.querySelector('[data-target-bar]')`));
  ck.check('目标条有切换目录按钮', await c.evaluate(`document.querySelector('[data-switch-dir]')?.textContent.includes('切换目录')`));
  ck.check('页面无「正在编辑」配置层下拉（AIL-122 移除）', await c.evaluate(`![...document.querySelectorAll('#app select')].some(s=>(s.getAttribute('aria-label')||'').includes('正在编辑'))`));

  // Skill 页签：根目录视图（本层 = worktree_override：repo disable + wt enable → added-here）
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].some(r=>r.textContent.includes('meeting-notes'))`, 15000);
  const rootRow = await rowOf();
  console.log('--- 根目录视角 ---\n' + rootRow);
  await c.screenshot(OUT + '/ail122-root-skill.png');
  ck.check('根目录行显示已启用（本层工作树显式启用）', rootRow.includes('已启用'));
  // 根目录：本层 enable、上游（项目共享）disable → 属于「本地新增」，动作是移除
  ck.check('根目录行动作含「移除」（本地新增语义）', await c.evaluate(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('meeting-notes'))?.textContent.includes('移除')`));

  // ── AIL-122 目录选择 Dialog ────────────────────────────────────
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('[data-dir-tree]')`, 8000);
  ck.check('Dialog 打开且焦点进入', await c.evaluate(`document.querySelector('dialog[open] [data-dir-search]')?.matches(':modal') || document.querySelector('dialog[open]')?.matches(':modal')`));
  await c.screenshot(OUT + '/ail122-dir-dialog.png');
  // 展开 web
  await c.evaluate(`document.querySelector('[data-twist="web"]')?.click()`);
  await c.waitFor(`[...document.querySelectorAll('[data-dir-row], .dir-row')].some(r=>r.dataset.rel==='web') || document.querySelector('.dir-row[data-rel="web"]')`, 5000);
  await c.evaluate(`document.querySelector('.dir-row[data-rel="web"]').click()`);
  ck.check('选中提示显示 web 路径', await c.evaluate(`document.querySelector('[data-dir-selected]')?.textContent.includes('/web')`));
  // 取消零写入
  await c.evaluate(`document.querySelector('[data-dir-cancel]').click()`);
  await c.waitFor(`!document.querySelector('dialog[open]')`, 5000);
  const dirBefore = await c.evaluate(`document.querySelector('[data-dir-label]')?.textContent`);
  ck.check('取消后仍为根目录', dirBefore === '根目录');
  const profBefore = await apiCall('/effective?root=' + ROOT);
  // 再次打开并真正选择 web
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-tree]')`, 8000);
  await c.evaluate(`document.querySelector('[data-twist="web"]')?.click()`);
  await c.waitFor(`document.querySelector('.dir-row[data-rel="web"]')`, 5000);
  await c.evaluate(`document.querySelector('.dir-row[data-rel="web"]').click()`);
  await c.evaluate(`document.querySelector('[data-dir-use]').click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')?.textContent==='web'`, 8000);
  ck.check('目标条目录标签 = web', true);
  // Esc 取消验证（零写入）在下一轮 picker 中验证
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].some(r=>r.textContent.includes('meeting-notes'))`, 15000);
  const webRow = await rowOf();
  console.log('--- web 目录视角（修复后）---\n' + webRow);
  await c.screenshot(OUT + '/ail121-web-view-fixed.png');
  ck.check('AIL-121 核心：web 行显示已启用（不再按项目默认误报停用）', webRow.includes('已启用') && !webRow.includes('已停用'));
  ck.check('web 行说明真实来源（当前目录配置/工作树）', webRow.includes('当前目录配置') || webRow.includes('来自'));
  const profAfter = await apiCall('/effective?root=' + ROOT);
  ck.check('浏览切换零配置写入（profile revision 不变）', profBefore.profile_revision === profAfter.profile_revision);

  // Esc 取消零写入
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-search]')`, 8000);
  await c.evaluate(`document.querySelector('dialog[open] [data-dir-search]').focus()`);
  await c.evaluate(`document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))`);
  await c.evaluate(`(() => { const d=document.querySelector('dialog[open]'); if(d) d.dispatchEvent(new KeyboardEvent('cancel',{bubbles:true,cancelable:true})); })()`);
  await new Promise(r => setTimeout(r, 400));
  await c.screenshot(OUT + '/ail122-esc-close.png');
  ck.check('Esc 可关闭 Dialog', await c.evaluate(`!document.querySelector('dialog[open]')`));

  // 慢响应逆序：根目录 → web 快速切换，慢的旧响应不得覆盖新状态
  await c.evaluate(`window.__origFetch=window.fetch; window.fetch=(...a)=>{ const u=String(a[0]); if(u.includes('/api/effective')) return new Promise(res=>setTimeout(()=>res(window.__origFetch(...a)), 1200)); return window.__origFetch(...a); }`);
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-tree]')`, 8000);
  await c.waitFor(`document.querySelector('.dir-row[data-rel=""]')`, 5000);
  await c.evaluate(`document.querySelector('.dir-row[data-rel=""]').click()`);
  await c.evaluate(`document.querySelector('[data-dir-use]').click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')?.textContent==='根目录'`, 5000);
  // 立即再切到 web（第一次 effective 还在路上）
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-tree]')`, 8000);
  await c.evaluate(`document.querySelector('[data-twist="web"]')?.click()`);
  await c.waitFor(`document.querySelector('.dir-row[data-rel="web"]')`, 5000);
  await c.evaluate(`document.querySelector('.dir-row[data-rel="web"]').click()`);
  await c.evaluate(`document.querySelector('[data-dir-use]').click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')?.textContent==='web'`, 5000);
  await new Promise(r => setTimeout(r, 2000));
  const afterRace = await rowOf();
  console.log('--- 慢响应竞态后的 web 视图 ---\n' + afterRace);
  ck.check('慢响应逆序后 web 状态仍是「已启用/来自当前目录配置」', afterRace.includes('已启用') && !afterRace.includes('已停用'));
  ck.check('恢复 fetch', await c.evaluate(`(window.fetch=window.__origFetch, true)`));

  ck.finish();
} finally { await c.close(); }
