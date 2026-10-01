// AIL-121 复现（修复前基线）：项目默认停用 → main 工作树启用 → web 无覆盖。
// 预期（服务端真实语义）：web 视图显示「已启用」，来源 = 工作树。
// 缺陷预期（旧 diffOf）：web 视图错误显示「已停用 / 跟随项目默认」。
import { connect, Checks } from './cdp.mjs';
const c = await connect();
const ck = new Checks('AIL-121-repro');
const OUT = '/tmp/ailoom-dirux/evidence/ail121';
const PID = 'repo-fabb1cfe5c90ca62';
try {
  await c.viewport(1440, 1000);
  await c.goto('#/projects/' + PID);
  await c.waitFor(`!!document.querySelector('[data-tab]')`, 15000);
  // Skill 页签
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('meeting-notes') || document.querySelector('[data-content]')?.textContent.includes('meeting-notes')`, 15000);
  const rowText = () => c.evaluate(`[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('meeting-notes'))?.innerText ?? '(无行)'`);
  // 工作树视角（默认）
  console.log('--- 工作树视角 ---');
  console.log(await rowText());
  await c.screenshot(OUT + '-before-worktree-view.png');
  // 切到「当前工作树 · 指定子目录」+ 输入 web
  await c.evaluate(`(() => { const s=[...document.querySelectorAll('select')].find(x=>x.getAttribute('aria-label')?.includes('正在编辑')); s.value='wt-sub'; s.dispatchEvent(new Event('change',{bubbles:true})); })()`);
  await c.waitFor(`!!document.querySelector('[data-sub-scope]') && !document.querySelector('[data-sub-wrap]')?.hidden`, 5000);
  await c.evaluate(`(() => { const i=document.querySelector('[data-sub-scope]'); i.value='web'; i.dispatchEvent(new Event('change',{bubbles:true})); })()`);
  await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('meeting-notes') || document.querySelector('[data-content]')?.innerText.includes('没有') || true`, 10000);
  await new Promise(r => setTimeout(r, 800));
  console.log('--- web 子目录视角（缺陷现场）---');
  const webRow = await rowText();
  console.log(webRow);
  await c.screenshot(OUT + '-before-web-view.png');
  ck.check('web 视图行应显示已启用（服务端 deployed=true）', !webRow.includes('已停用') && (webRow.includes('已启用')));
  ck.check('web 视图应说明来源为工作树', webRow.includes('工作树') || webRow.includes('当前工作树'));
  ck.finish();
} finally { await c.close(); }
