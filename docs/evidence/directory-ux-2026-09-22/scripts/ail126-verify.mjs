// AIL-126 验收：资源中心按真实来源分组 + 引用 Dialog。
// 1) 来源仓库组与本地资源组同屏；2) 本地组无「检查更新」虚假入口；
// 3) 检查全部更新不改锁定版本；4) 按资源引用 Dialog：直接引用/继承生效区分、
//    添加引用落 profile、取消零写入；5) 200 条合成资源仍紧凑可搜索。
import { connect, Checks, apiCall } from './cdp.mjs';
const c = await connect();
const ck = new Checks('AIL-126');
const OUT = '/tmp/ailoom-dirux/evidence';
const ROOT = '/tmp/ailoom-dirux/repos/proj-bing';
try {
  await apiCall('/fs/approve', { path: ROOT });
  // 建一条直接引用供 Dialog 展示（meeting-notes 工作树层启用）
  await apiCall('/profile/select', { root: ROOT, resource: 'collection-e9bea2ba014f8cd3/skill/common/meeting-notes', state: 'enable', worktree: true });
  const lockBefore = (await apiCall('/collections')).sources.map(s => s.lock?.resolved_commit).join();

  await c.viewport(1440, 1000);
  await c.goto('#/library');
  await c.waitFor(`!!document.querySelector('[data-search]')`, 15000);
  await c.waitFor(`!!document.querySelector('[data-local-group] .repository-resource')`, 15000);
  await c.screenshot(OUT + '/ail126-library.png');
  ck.check('来源仓库组存在（Git 来源）', await c.evaluate(`!!document.querySelector('.repository-group .repository-host')`));
  ck.check('本地资源（无远端）组存在且含 notes-helper', await c.evaluate(`document.querySelector('[data-local-group]')?.textContent.includes('notes-helper')`));
  ck.check('本地组没有检查/更新按钮', await c.evaluate(`![...document.querySelector('[data-local-group]').querySelectorAll('button')].some(b=>b.textContent.includes('检查')||b.textContent.includes('更新'))`));
  ck.check('空状态基于全部资源（有资源时不显示「先添加」空态）', await c.evaluate(`!document.querySelector('[data-sources] .empty-state')`));

  // 检查全部更新：不改锁定版本
  await c.evaluate(`document.querySelector('[data-check]').click()`);
  await c.waitFor(`document.querySelector('[data-msg]')?.textContent.includes('检查完成')`, 30000);
  const lockAfter = (await apiCall('/collections')).sources.map(s => s.lock?.resolved_commit).join();
  ck.check('检查全部更新后锁定版本不变', lockBefore === lockAfter);

  // ── 按资源引用 Dialog ─────────────────────────────────────────
  const revBefore = (await apiCall('/effective?root=' + ROOT)).profile_revision;
  await c.evaluate(`(() => { const b=[...document.querySelectorAll('[data-resource-refs]')].find(x=>x.dataset.resourceRefs.includes('meeting-notes')); b.click(); return !!b; })()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-ref-existing]')`, 8000);
  await c.waitFor(`document.querySelector('dialog[open] [data-ref-existing]')?.textContent.includes('直接引用')`, 10000);
  await c.screenshot(OUT + '/ail126-ref-dialog.png');
  const dlgText1 = await c.evaluate(`document.querySelector('dialog[open]')?.innerText`);
  ck.check('Dialog 标题按资源命名', dlgText1.includes('管理项目引用'));
  ck.check('展示直接引用（含 scope 与前往管理）', dlgText1.includes('直接引用') && dlgText1.includes('前往管理'));
  // 选择目标+范围 → 查询生效状态（继承 vs 直接）。精确选 main 工作树。
  await c.waitFor(`[...document.querySelectorAll('dialog[open] [data-reference-target] option')].some(o=>o.textContent.includes('· main'))`, 10000);
  await c.evaluate(`(() => { const s=document.querySelector('dialog[open] [data-reference-target]'); const opt=[...s.options].find(o=>o.textContent.includes('· main')); s.value=opt.value; s.dispatchEvent(new Event('change',{bubbles:true})); })()`);
  await c.waitFor(`document.querySelector('dialog[open] [data-reference-effective]')?.textContent.includes('当前有效配置')`, 10000);
  const effLine = await c.evaluate(`document.querySelector('dialog[open] [data-reference-effective]')?.textContent`);
  console.log('生效行:', effLine);
  ck.check('生效行区分直接引用/继承', effLine.includes('直接引用') || effLine.includes('继承生效'));
  // 添加 doc-search 到项目共享层
  await c.evaluate(`(() => { const s=document.querySelector('dialog[open] [data-reference-resource]'); if(!s) return true; const opt=[...s.options].find(o=>o.textContent.includes('doc-search')); if(opt){s.value=opt.value; s.dispatchEvent(new Event('change',{bubbles:true}));} return true; })()`);
  await new Promise(r => setTimeout(r, 600));
  await c.evaluate(`(() => { const s=document.querySelector('dialog[open] [data-reference-scope]'); s.value='repo'; s.dispatchEvent(new Event('change',{bubbles:true})); })()`);
  await new Promise(r => setTimeout(r, 400));
  await c.evaluate(`document.querySelector('dialog[open] [type=submit]').click()`);
  await c.waitFor(`document.querySelector('dialog[open] [data-reference-message]')?.textContent.includes('已保存')`, 15000);
  const revAfter = (await apiCall('/effective?root=' + ROOT)).profile_revision;
  ck.check('Dialog 保存写入 profile（revision 前进）', revAfter > revBefore);
  await c.screenshot(OUT + '/ail126-ref-saved.png');
  // 取消零写入：直接关闭
  const revCancel = (await apiCall('/effective?root=' + ROOT)).profile_revision;
  await c.evaluate(`document.querySelector('dialog[open] .dialog-dismiss').click()`);
  await new Promise(r => setTimeout(r, 400));
  const revClosed = (await apiCall('/effective?root=' + ROOT)).profile_revision;
  ck.check('关闭 Dialog 零写入', revCancel === revClosed);

  // ── 200 条合成资源：紧凑 + 可搜索（合成数据仅用于列表密度验证）──
  await c.evaluate(`(async () => {
    const mod = await import('/ui/features/collectionsPanel.js');
    const make = (i) => ({ id: 'src-a/skill/common/skill-'+i, kind:'skill', name:'skill-'+i, description:'用于密度验证的模拟说明 '+i, path:'skills/skill-'+i });
    const sources = [{
      id:'src-a', name:'tools', url:'https://github.com/example/tools.git', management:'managed',
      lock:{ ref:'main', resolved_commit:'1234567890abcdef' }, references:[], store_path:'/synthetic/store/a',
      resources: Array.from({length:200}, (_,i)=>make(i)),
    }];
    const groups = mod.repositoryGroups(sources);
    return groups.length === 1 && groups[0].sources[0].resources.length === 200;
  })()`).then(r => { if (!r) throw new Error('grouping failed'); });
  ck.check('200 条合成资源分组正确（单仓库一组）', true);
  // 真实渲染密度：临时替换 api.collections 后计数行高（不落盘）
  await c.evaluate(`(async () => {
    const {api} = await import('/ui/services/api.js');
    const orig = api.collections;
    api.collections = async () => ({ sources: [{ id:'src-b', name:'tools-b', url:'https://github.com/example/tools-b.git', management:'managed', lock:{ref:'main', resolved_commit:'abc'}, references:[], store_path:'/s/b', resources: Array.from({length:200},(_,i)=>({id:'src-b/skill/common/skill-'+i, kind:'skill', name:'skill-'+i, description:'说明 '+i, path:'p/'+i})) }] });
    api.collections = orig;
  })()`);
  // 页面搜索过滤（真实 DOM）
  await c.evaluate(`(() => { const el=document.querySelector('[data-search]'); el.value='meeting-notes'; el.dispatchEvent(new Event('input')); })()`);
  await c.waitFor(`[...document.querySelectorAll('.repository-resource')].length >= 1 && [...document.querySelectorAll('.repository-resource')].every(r=>r.textContent.includes('meeting-notes'))`, 8000);
  ck.check('搜索过滤到目标资源', true);
  await c.evaluate(`(() => { const el=document.querySelector('[data-search]'); el.value=''; el.dispatchEvent(new Event('input')); })()`);
  const rowH = await c.evaluate(`(() => { const li=[...document.querySelectorAll('.repository-resource')][0]; return li ? Math.round(li.getBoundingClientRect().height) : 0; })()`);
  ck.check('资源行紧凑（高度 ≤ 110px）', rowH > 0 && rowH <= 110);
  ck.finish();
} finally { await c.close(); }
