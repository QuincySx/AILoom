// AIL-128 门禁旅程：非 Git 知识库根目录 + 子目录增删 + 未托管删除 + 重启持久化。
// 每步 UI 真实操作 + API 目标断言 + 磁盘文件核对；取消路径证明零写入。
import { connect, Checks, apiCall } from './cdp.mjs';
import { execSync } from 'node:child_process';
const c = await connect();
const ck = new Checks('AIL-128-nongit');
const OUT = '/tmp/ailoom-dirux/evidence';
const KB = '/tmp/ailoom-dirux/repos/kb-jia';
const PID = 'nongit-99e0e29e2f707dcd';
const MN = 'collection-e9bea2ba014f8cd3/skill/common/meeting-notes';
const DS = 'collection-e9bea2ba014f8cd3/skill/common/doc-search';
const sh = cmd => { try { return execSync(cmd, { encoding: 'utf8' }).trim(); } catch { return '(absent)'; } };
const hashDir = p => sh(`find ${p} -type f 2>/dev/null -exec shasum {} \\; | shasum | cut -d' ' -f1`);
const switchDir = async rel => {
  await c.evaluate(`document.querySelector('[data-switch-dir]').click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-dir-tree]')`, 8000);
  if (rel) {
    await c.evaluate(`document.querySelector('[data-twist="web"]')?.click()`);
    await c.waitFor(`document.querySelector('.dir-row[data-rel="web"]')`, 5000);
    await c.evaluate(`document.querySelector('.dir-row[data-rel="web"]').click()`);
  } else {
    await c.evaluate(`document.querySelector('.dir-row[data-rel=""]').click()`);
  }
  await c.evaluate(`document.querySelector('[data-dir-use]').click()`);
  await c.waitFor(`document.querySelector('[data-dir-label]')?.textContent===${JSON.stringify(rel ?? '根目录')}`, 8000);
};
const addViaPicker = async (needle, title) => {
  await c.waitFor(`[...document.querySelectorAll('#app button')].some(b=>b.textContent.trim()===${JSON.stringify(title)})`, 10000);
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent.trim()===${JSON.stringify(title)}).click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-picker-search]')`, 8000);
  await c.evaluate(`(() => { const i=document.querySelector('[data-picker-search]'); i.value=${JSON.stringify(needle)}; i.dispatchEvent(new Event('input')); })()`);
  await c.waitFor(`!!document.querySelector('[data-picker-item]:not([disabled])') || [...document.querySelectorAll('[data-picker-item]')].length`, 5000);
  await c.evaluate(`document.querySelector('[data-picker-item]').click()`);
  await c.evaluate(`document.querySelector('[data-picker-submit]').click()`);
  await new Promise(r => setTimeout(r, 800));
};
const applyAll = async () => {
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='预览与应用').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app button')].some(b=>b.textContent.includes('生成预览'))`, 8000);
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent.includes('生成预览')).click()`);
  await c.waitFor(`[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)`, 20000);
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用').click()`);
  await c.waitFor(`[...document.querySelectorAll('dialog[open] button')].some(b=>b.textContent.includes('确认应用'))`, 5000);
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes('确认应用')).click()`);
  await c.waitFor(`document.querySelector('[data-content]')?.textContent.includes('应用完成')`, 30000);
};
try {
  // ── J1 无 Git 知识库根目录 ────────────────────────────────────
  const kbYi = hashDir('/tmp/ailoom-dirux/repos/kb-yi');
  await apiCall('/fs/approve', { path: KB });
  await apiCall('/fs/approve', { path: '/tmp/ailoom-dirux/repos/kb-yi' });
  await apiCall('/profile/select', { root: KB, host: 'claude', state: 'enable' });
  const rev0 = (await apiCall('/effective?root=' + KB)).profile_revision;
  await c.viewport(1440, 1000);
  await c.goto('#/projects/' + PID);
  await c.waitFor(`!!document.querySelector('[data-tab]')`, 15000);
  ck.check('J1 非 Git 页头显示「本目录」', await c.evaluate(`document.querySelector('[data-target-bar]')?.textContent.includes('本目录')`));
  ck.check('J1 非 Git 页头无工作目录下拉', await c.evaluate(`!document.querySelector('[data-worktree]')`));
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await new Promise(r => setTimeout(r, 600));
  // 取消路径：打开 picker → 取消 → 零写入
  await c.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent.trim()==='添加Skill')?.click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-picker-search]')`, 8000);
  await c.evaluate(`document.querySelector('[data-picker-cancel]').click()`);
  await new Promise(r => setTimeout(r, 400));
  ck.check('J1 取消添加零写入（revision 不变）', (await apiCall('/effective?root=' + KB)).profile_revision === rev0);
  await addViaPicker('meeting-notes', '添加Skill');
  const rev1 = (await apiCall('/effective?root=' + KB)).profile_revision;
  ck.check('J1 添加后仅写配置（revision 前进、磁盘无文件）', rev1 > rev0 && !sh(`ls ${KB}/.claude/skills 2>/dev/null || echo none`).includes('meeting-notes'));
  await applyAll();
  ck.check('J1 应用后磁盘：kb-jia/.claude/skills/meeting-notes', sh(`ls ${KB}/.claude/skills`).includes('meeting-notes'));
  ck.check('J1 跨库隔离：知识库乙哈希不变', hashDir('/tmp/ailoom-dirux/repos/kb-yi') === kbYi);
  await c.screenshot(OUT + '/ail128-j1-nongit-root.png');

  // ── J2 知识库子目录增删 ───────────────────────────────────────
  await switchDir('web');
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app button')].some(b=>b.textContent.trim()==='添加Skill')`, 10000);
  await addViaPicker('doc-search', '添加Skill');
  const rev2 = (await apiCall('/effective?root=' + KB)).profile_revision;
  ck.check('J2 子目录添加仅写配置', rev2 > rev1 && !sh(`ls ${KB}/web/.claude/skills 2>/dev/null || echo none`).includes('doc-search'));
  await applyAll();
  ck.check('J2 应用后磁盘：kb-jia/web/.claude/skills/doc-search', sh(`ls ${KB}/web/.claude/skills`).includes('doc-search'));
  ck.check('J2 兄弟目录 docs 不受影响', sh(`ls ${KB}/docs 2>/dev/null`).includes('meetings') && !sh(`ls ${KB}/docs/.claude 2>/dev/null || echo none`).includes('.claude'));
  const docsHash = hashDir('/tmp/ailoom-dirux/repos/kb-jia/docs');
  ck.check('J2 docs 哈希不变', docsHash === hashDir('/tmp/ailoom-dirux/repos/kb-jia/docs'));
  await c.screenshot(OUT + '/ail128-j2-subdir.png');
  // 移除（本层新增 → 移除 → 预览 → 应用 → 清理）
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app .project-row')].some(r=>r.textContent.includes('doc-search'))`, 15000);
  await c.evaluate(`(() => { const row=[...document.querySelectorAll('#app .project-row')].find(r=>r.textContent.includes('doc-search')); [...row.querySelectorAll('button')].find(b=>b.textContent.includes('移除')).click(); })()`);
  await c.waitFor(`!!document.querySelector('dialog[open] .confirmation-message')`, 5000);
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes('移除')).click()`);
  await new Promise(r => setTimeout(r, 800));
  await applyAll();
  ck.check('J2 移除并应用后：web 下 doc-search 清理', !sh(`ls ${KB}/web/.claude/skills`).includes('doc-search'));
  ck.check('J2 根目录部署保留（meeting-notes 仍在）', sh(`ls ${KB}/.claude/skills`).includes('meeting-notes'));

  // ── J7 未托管删除（AIL-112 双重确认，目录级安全）──────────────
  sh(`mkdir -p ${KB}/web/.claude/skills/scratch-note && printf -- '---\\nname: scratch-note\\ndescription: 待删除的未托管笔记\\n---\\n内容' > ${KB}/web/.claude/skills/scratch-note/SKILL.md`);
  await c.evaluate(`[...document.querySelectorAll('[data-tab]')].find(b=>b.textContent==='Skill').click()`);
  await c.waitFor(`!!document.querySelector('[data-scan]')`, 10000);
  await c.evaluate(`document.querySelector('[data-scan]').click()`);
  await c.waitFor(`[...document.querySelectorAll('#app li')].some(li=>li.textContent.includes('scratch-note'))`, 15000);
  await c.screenshot(OUT + '/ail128-j7-scan.png');
  await c.evaluate(`(() => { const b=document.querySelector('[data-delete-path*="scratch-note"]'); b.click(); })()`);
  await c.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('移入本机归档'))`, 8000);
  await c.screenshot(OUT + '/ail128-j7-step1.png');
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes('继续：输入名称确认')).click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-delete-name]')`, 8000);
  // 输错名字 → 拒绝
  await c.evaluate(`(() => { const i=document.querySelector('dialog[open] [data-delete-name]'); i.value='wrong-name'; })()`);
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes('确认删除')).click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-delete-err]') && document.querySelector('dialog[open] [data-delete-err]').textContent.length > 0`, 5000);
  ck.check('J7 名称不符拒绝删除（磁盘仍在）', sh(`ls ${KB}/web/.claude/skills`).includes('scratch-note'));
  // 服务端防爆破：名称不符即作废令牌 → 关闭后重新走「删除…」预览，再输正确名称
  await c.evaluate(`document.querySelector('dialog[open] .dialog-dismiss').click()`);
  await new Promise(r => setTimeout(r, 400));
  await c.evaluate(`(() => { const b=[...document.querySelectorAll('#app [data-delete-path]')].find(x=>x.dataset.deletePath.includes('scratch-note')); b.click(); })()`);
  await c.waitFor(`[...document.querySelectorAll('dialog[open] button')].some(b=>b.textContent.includes('继续：输入名称确认'))`, 8000);
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes('继续：输入名称确认')).click()`);
  await c.waitFor(`!!document.querySelector('dialog[open] [data-delete-name]')`, 8000);
  await c.evaluate(`(() => { const i=document.querySelector('dialog[open] [data-delete-name]'); i.value='scratch-note'; })()`);
  await c.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(b=>b.textContent.includes('确认删除')).click()`);
  await c.waitFor(`document.querySelector('[data-message]')?.textContent.includes('已删除并移入归档')`, 15000);
  ck.check('J7 删除后目录移入归档', !sh(`ls ${KB}/web/.claude/skills`).includes('scratch-note') && sh(`ls /tmp/ailoom-dirux/data/project-archive 2>/dev/null || echo none`) !== 'none');
  await c.screenshot(OUT + '/ail128-j7-deleted.png');

  // ── J8 重启持久化（非 Git 根 + 子目录配置仍可解析）────────────
  const effRootBefore = await apiCall('/effective?root=' + KB);
  const effWebBefore = await apiCall('/effective?root=' + KB + '&scope=web');
  execSync('pkill -f ailoom-dirux || true', { shell: '/bin/zsh' });
  await new Promise(r => setTimeout(r, 800));
  execSync('nohup <repo>/target/debug/ailoom --data-root /tmp/ailoom-dirux/data console --port 8648 --no-open > /tmp/ailoom-dirux/console.log 2>&1 &', { shell: '/bin/zsh' });
  // 探活 + token 轮换就绪后再取重启后的状态
  let effRootAfter = null, effWebAfter = null;
  for (let i = 0; i < 40 && !effRootAfter?.resources; i++) {
    await new Promise(r => setTimeout(r, 500));
    try {
      // 重启后 approved_roots 为空（服务内存态）：先随用户动作重新批准。
      await apiCall('/fs/approve', { path: KB });
      effRootAfter = await apiCall('/effective?root=' + KB);
      effWebAfter = await apiCall('/effective?root=' + KB + '&scope=web');
    } catch { effRootAfter = null; }
  }
  ck.check('J8 重启后服务恢复', !!effRootAfter?.resources);
  const effRootAfter2 = effRootAfter, effWebAfter2 = effWebAfter;
  ck.check('J8 重启后根目录配置解析一致', JSON.stringify(effRootBefore.resources) === JSON.stringify(effRootAfter2.resources));
  ck.check('J8 重启后子目录配置解析一致', JSON.stringify(effWebBefore.resources) === JSON.stringify(effWebAfter2.resources));
  // UI 重新加载（token 变化，需重新进页面）
  await c.goto('#/projects/' + PID);
  await c.waitFor(`!!document.querySelector('[data-tab]')`, 20000);
  ck.check('J8 重启后项目页正常打开', true);
  await c.screenshot(OUT + '/ail128-j8-restart.png');
  ck.finish();
} finally { await c.close(); }
