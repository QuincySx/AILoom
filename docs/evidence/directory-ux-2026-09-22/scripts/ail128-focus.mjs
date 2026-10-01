// AIL-128 门禁：Dialog 焦点管理（进入/Esc/焦点还原）。
import { connect, Checks, apiCall } from './cdp.mjs';
const c = await connect();
const ck = new Checks('AIL-128-focus');
const OUT = '/tmp/ailoom-dirux/evidence';
try {
  await apiCall('/fs/approve', { path: '/tmp/ailoom-dirux/repos/proj-bing' });
  await c.viewport(1440, 1000);
  await c.goto('#/projects/repo-fabb1cfe5c90ca62');
  await c.waitFor(`!!document.querySelector('[data-switch-dir]')`, 15000);
  // 记录触发器并打开 Dialog
  await c.evaluate(`document.querySelector('[data-switch-dir]').focus()`);
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-search]')`, 8000);
  ck.check('Dialog 打开后焦点进入搜索框', await c.evaluate(`document.activeElement === document.querySelector('dialog[open] [data-dir-search]')`));
  // Tab 移动焦点（焦点圈未被劫持）
  await c.evaluate(`document.activeElement.dispatchEvent(new KeyboardEvent('keydown',{key:'Tab',bubbles:true}))`);
  await c.evaluate(`document.querySelector('dialog[open] [data-dir-search]').focus(); const ev=new KeyboardEvent('keydown',{key:'Tab',bubbles:true}); document.activeElement.dispatchEvent(ev)`);
  // Esc 关闭（对 dialog 触发 cancel）
  await c.evaluate(`(() => { const d=document.querySelector('dialog[open]'); d.dispatchEvent(new KeyboardEvent('cancel',{bubbles:true,cancelable:true})); })()`);
  await c.waitFor(`!document.querySelector('dialog[open]')`, 5000);
  ck.check('Esc 关闭 Dialog', true);
  ck.check('关闭后焦点还原到触发按钮', await c.evaluate(`document.activeElement?.dataset?.switchDir !== undefined || document.activeElement?.textContent?.includes('切换目录')`));
  await c.screenshot(OUT + '/ail128-focus-restore.png');
  ck.finish();
} finally { await c.close(); }
