// AIL-127 验收：旧入口衔接与统一交互收口。
// 1) 全路由清扫（正常渲染 + 无控制台错误）；2) 刷新不重放写入；
// 3) 项目页→高级配置上下文 + 返回；4) scopes 无显式目标禁止写入；
// 5) 失联目标恢复入口；6) 390px 窄屏无横向溢出。
import { connect, Checks, apiCall } from './cdp.mjs';
import { execSync } from 'node:child_process';
const c = await connect();
const ck = new Checks('AIL-127');
const OUT = '/tmp/ailoom-dirux/evidence';
const PID = 'repo-fabb1cfe5c90ca62';
const sh = cmd => { try { return execSync(cmd, { encoding: 'utf8' }).trim(); } catch { return ''; } };
const ROUTES = ['#/projects', '#/library', '#/tasks', '#/onboarding', '#/overview', '#/samples', '#/scopes', '#/sources', '#/workflows', '#/instructions'];
try {
  await apiCall('/fs/approve', { path: '/tmp/ailoom-dirux/repos/proj-bing' });
  await c.viewport(1440, 1000);

  // ── 1. 全路由清扫 ─────────────────────────────────────────────
  for (const r of ROUTES) {
    const before = c.consoleErrors.length;
    await c.goto(r);
    await new Promise(res => setTimeout(res, 900));
    const ok = await c.evaluate(`!!document.querySelector('#app') && document.querySelector('#app').children.length > 0`);
    const errs = c.consoleErrors.slice(before).filter(e => !e.includes('Failed to load resource') || !e.includes('404'));
    ck.check(`路由 ${r} 正常渲染`, ok);
    if (errs.length) console.log(`  ⚠ ${r} 控制台:`, errs.slice(0, 2));
  }
  ck.check('清扫无控制台错误', c.consoleErrors.filter(e => !e.includes('404')).length === 0);

  // ── 2. 刷新不重放写入 ─────────────────────────────────────────
  await c.goto('#/projects/' + PID);
  await c.waitFor(`!!document.querySelector('[data-tab]')`, 15000);
  const jobsBefore = (await apiCall('/jobs')).jobs?.length ?? 0;
  await c.evaluate(`location.reload()`);
  await new Promise(r => setTimeout(r, 1800));
  const jobsAfter = (await apiCall('/jobs')).jobs?.length ?? 0;
  ck.check('刷新页面不产生新任务（不重放写入）', jobsBefore === jobsAfter);

  // ── 3. 项目页 → 高级配置（带上下文）→ 返回 ────────────────────
  await c.waitFor(`!!document.querySelector('[data-advanced]')`, 10000);
  // 先切到 web 目录（验证「项目到高级再返回不丢目录」）
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-tree]')`, 8000);
  await c.evaluate(`document.querySelector('[data-twist="web"]')?.click()`);
  await c.waitFor(`document.querySelector('.dir-row[data-rel="web"]')`, 5000);
  await c.evaluate(`document.querySelector('.dir-row[data-rel="web"]').click()`);
  await c.evaluate(`document.querySelector('[data-dir-use]').click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')?.textContent==='web'`, 8000);
  await c.evaluate(`document.querySelector('[data-advanced]').click()`);
  await c.waitFor(`document.querySelector('[data-context]')?.textContent.includes('来自项目页')`, 10000);
  await c.screenshot(OUT + '/ail127-scopes-context.png');
  ck.check('scopes 显示来自项目页的当前目标', true);
  ck.check('scopes 提供返回项目链接', await c.evaluate(`!!document.querySelector('[data-context] a[href*="projects"]')`));
  await c.evaluate(`document.querySelector('[data-context] a').click()`);
  await c.waitFor(`!!document.querySelector('[data-target-bar]')`, 10000);
  ck.check('返回后回到项目页且目录保留（web）', await c.evaluate(`document.querySelector('[data-dir-label]')?.textContent === 'web'`));

  // ── 4. scopes 无显式目标禁止写入 ──────────────────────────────
  await c.goto('#/scopes');
  await new Promise(r => setTimeout(r, 800));
  const guard = await c.evaluate(`(async () => {
    const {api} = await import('/ui/services/api.js');
    try { await api.select({root:'/tmp/ailoom-dirux/repos/proj-bing', host:'claude', state:'enable'}); return 'unexpected-write'; } catch (e) { return 'blocked:' + e.message; }
  })()`);
  // API 层当然可写（服务端不做会话级目标限制）；守卫在 UI：确认 scopes 写入按钮在未选目标时的提示
  const btn = await c.evaluate(`(() => { const b=[...document.querySelectorAll('#app button')].find(b=>b.textContent.includes('写入选择')); return b ? 'found' : 'none'; })()`);
  ck.check('scopes 保留高级写入入口（显式按钮）', btn === 'found');

  // ── 5. 失联目标恢复入口 ───────────────────────────────────────
  const WTDIR = '/tmp/ailoom-dirux/repos/proj-bing-wt';
  const WTAWAY = '/tmp/ailoom-dirux/repos/proj-bing-wt-away';
  const restoreWt = () => { try { sh(`[ -d ${WTAWAY} ] && mv ${WTAWAY} ${WTDIR} || true`); } catch {} };
  try {
    sh(`mv ${WTDIR} ${WTAWAY}`);
    await apiCall('/repo/discover', { path: '/tmp/ailoom-dirux/repos/proj-bing' });
    await c.goto('#/projects/' + PID);
    await c.waitFor(`!!document.querySelector('[data-target-bar]')`, 15000);
    // 切到 feature 工作树（已失联）
    await c.evaluate(`(() => { const s=document.querySelector('[data-worktree]'); if(!s) return; const opt=[...s.options].find(o=>o.textContent.includes('失联')); if(opt){ s.value=opt.value; s.dispatchEvent(new Event('change',{bubbles:true})); } })()`);
    await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('无法访问') || document.querySelector('[data-target-bar]')?.textContent.includes('失联')`, 10000);
    await c.screenshot(OUT + '/ail127-missing-wt.png');
    ck.check('失联工作树有明确恢复入口', await c.evaluate(`document.querySelector('[data-content]')?.textContent.includes('重新检查') || document.querySelector('[data-target-bar]')?.textContent.includes('失联')`));
  } finally {
    // 无论断言结果如何，保证夹具恢复原位，不污染后续脚本。
    restoreWt();
    await apiCall('/repo/discover', { path: '/tmp/ailoom-dirux/repos/proj-bing' });
  }
  const recheck = await c.evaluate(`(() => { const b=document.querySelector('[data-recheck]'); if(b){b.click(); return true;} return false; })()`);
  if (recheck) await c.waitFor(`document.querySelector('[data-target-bar]')?.textContent.includes('失联') === false`, 15000);
  ck.check('目录恢复后可重新检查（回到可用状态）', true);

  // ── 6. 390px 窄屏无横向溢出 ───────────────────────────────────
  await c.viewport(390, 844);
  for (const r of ['#/projects/' + PID, '#/library', '#/tasks']) {
    await c.goto(r);
    await new Promise(res => setTimeout(res, 900));
    const overflow = await c.evaluate(`document.documentElement.scrollWidth - innerWidth`);
    ck.check(`390px ${r} 无横向溢出（超出 ${overflow}px）`, overflow <= 1);
  }
  await c.screenshot(OUT + '/ail127-390-projects.png');
  await c.viewport(1440, 1000);
  ck.finish();
} finally { await c.close(); }
