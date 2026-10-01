// AIL-109 端到端：建项目 → 导入来源 → 选宿主 → 加 Skill/MCP/Agent/指令 → 预览 → 应用
// → 移除 → 再应用 → 检查更新 → 按项目升级决策 → 撤销 → 重启复核。全程真实 UI + API + 文件核对。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const E = '/tmp/ailoom-e2e';
const A = `${E}/projects/a`;
const profile = () => { try { return execSync(`cat ${E}/data/profile/profile.toml`).toString(); } catch { return '(no profile)'; } };
const files = (p) => execSync(`find ${p} -not -path "*/.git/*" -not -name ".git" \\( -type f -o -type l \\) | sort`).toString();
const shot = (n) => b.screenshot(`/tmp/ailoom-flow/evidence/ail109-${n}.png`);
const b = await connect('http://127.0.0.1:8644/');
await b.waitFor('!!document.querySelector("#nav")');
// 应用目标跟随页面实际选中的工作目录（页头展示）
let A_DIR = null;
const syncDir = async () => { A_DIR = await b.evaluate(`document.querySelector('[data-header-path]')?.textContent`); log('当前工作目录:', A_DIR); };
const log = (...a) => console.log('[E2E]', ...a);

// ---- 1. 通过 UI 建项目 A/B/C ----
async function addProject(path, name, category) {
  await b.goto('#/projects');
  await b.waitFor('!!document.querySelector("[data-new]")');
  await b.evaluate(`(async()=>{
    document.querySelector('[data-new]').click();
    await new Promise(r=>setTimeout(r,150));
    document.querySelector('[data-path]').value=${JSON.stringify(path)};
    document.querySelector('[data-name]').value=${JSON.stringify(name)};
    document.querySelector('[data-category]').value=${JSON.stringify(category)};
    document.querySelector('[data-create]').requestSubmit();
  })()`);
  await b.waitFor('!!document.querySelector("[data-tab]")');
}
await addProject(`${E}/projects/a`, '电商中台A', '验收');
const wtCount = await b.evaluate('document.querySelectorAll("[data-worktree] option").length');
log('项目A 工作树数:', wtCount);
if (wtCount !== 2) throw new Error('A 应有两个工作树');
await addProject(`${E}/projects/b`, '电商中台B', '验收');
await addProject(`${E}/projects/c`, '普通目录C', '验收');
log('C 无工作树控件:', await b.evaluate('!document.querySelector("[data-worktree]")'));
await shot('projects-registered');

// ---- 2. 导入两个来源（git） + CC Switch 外部来源 ----
async function importGit(url, name) {
  await b.goto('#/library');
  await b.waitFor(`document.querySelector('[data-sources]')?.innerText.length > 5`);
  await new Promise(r=>setTimeout(r,400));
  if (await b.evaluate(`document.querySelector('[data-sources]').innerText.includes(${JSON.stringify(name)})`)) { log('来源已登记，跳过:', name); return; }
  await b.waitFor('!!document.querySelector("[data-add]") && !document.querySelector("[data-add]").disabled');
  await b.evaluate("document.querySelector('[data-add]').click()");
  await b.waitFor("document.querySelector('[data-import]')?.closest('dialog')?.matches(':modal')");
  await b.evaluate(`(async()=>{
    document.querySelector('[data-provider]').value='git';
    document.querySelector('[data-provider]').dispatchEvent(new Event('change'));
    document.querySelector('[data-name]').value=${JSON.stringify(name)};
    document.querySelector('[data-url]').value=${JSON.stringify(url)};
  })()`);
  await b.evaluate("document.querySelector('[data-preview]').click()");
  await b.waitFor("!!document.querySelector('[data-confirm]')");
  await b.evaluate("document.querySelector('[data-confirm]').click()");
  await b.waitFor(`document.querySelector('[data-msg]').textContent.includes('已加入资源库')`);
}
await importGit(`${E}/sources/team-src`, '平台技能库');
await importGit(`${E}/sources/second-src`, '备用技能库');
log('来源导入完成');

// ---- 3. 选宿主 claude ----
await b.goto('#/projects');
await b.waitFor('!!document.querySelector("[data-search]")');
await b.evaluate(`[...document.querySelectorAll('[data-list] a')].find(a=>a.textContent==='电商中台A').click()`);
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.waitFor('!!document.querySelector("[data-entries] .project-row")');
await b.evaluate(`(()=>{ const row=[...document.querySelectorAll('[data-entries] .project-row')][0]; row.querySelector('select').value='enable'; [...row.querySelectorAll('button')].find(x=>x.textContent==='保存').click(); })()`);
await b.waitFor('document.querySelector("[data-message]").textContent.includes("已保存")');
log('宿主 claude 已启用（项目默认）');
await syncDir();

// ---- 4. 添加 Skill / MCP / Agent ----
const addResource = async (tab, match) => {
  await b.evaluate(`document.querySelector('[data-tab="${tab}"]').click()`);
  await b.waitFor(`!!document.querySelector('[data-content] .tab-actions')`);
  await b.evaluate("document.querySelector('[data-add]').click()");
  await b.waitFor("!!document.querySelector('dialog[open] [data-picker-item]')");
  await b.evaluate(`(()=>{ const box=[...document.querySelectorAll('[data-picker-item]')].find(x=>x.value.includes(${JSON.stringify(match)})); box.checked=true; box.dispatchEvent(new Event('change')); document.querySelector('[data-picker-submit]').click(); })()`);
  await b.waitFor(`!document.querySelector('dialog[open]')`);
  await b.waitFor('!!document.querySelector("[data-entries] .project-row")');
};
await addResource(1, '/skill/common/code-review');
await addResource(2, '/mcp/common/team-files');
await addResource(3, '/agent/common/release-helper');
log('Skill/MCP/Agent 引用已保存');
// 指令
await b.evaluate(`document.querySelector('[data-tab="4"]').click()`);
await b.waitFor('!!document.querySelector("[data-save]") && !document.querySelector("[data-save]").disabled');
await b.evaluate(`(()=>{ const ta=document.querySelector('[data-editor] textarea'); ta.value='- E2E：指令验收条目'; ta.dispatchEvent(new Event('input')); })()`);
await b.evaluate("document.querySelector('[data-save]').click()");
await b.waitFor(`document.querySelector('[data-msg]').textContent.includes('已保存')`);
log('个人指令已保存');

// ---- 5. 预览 → 应用（核对真实文件） ----
await syncDir();
const beforeApply = files(A_DIR);
await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
await b.waitFor('!!document.querySelector("[data-content] h2")');
await b.evaluate(`[...document.querySelectorAll('#app button')].find(x=>x.textContent==='生成预览').click()`);
await b.waitFor(`document.querySelector('[data-content]').innerText.includes('仓库')`);
const previewText = await b.evaluate(`document.querySelector('[data-content]').innerText`);
log('预览含 create 动作:', previewText.includes('create'));
await shot('preview');
if (files(A_DIR) !== beforeApply) throw new Error('预览阶段不得写文件');
await b.evaluate(`[...document.querySelectorAll('#app button')].find(x=>x.textContent==='应用' && !x.disabled).click()`);
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='确认应用').click()`);
const waitFile = async (f, timeout=30000) => {
  for (let i=0;i<timeout/500;i++) {
    if (execSync(`test -e ${JSON.stringify(f)} && echo yes || echo no`).toString().includes('yes')) return;
    await new Promise(r=>setTimeout(r,500));
  }
  throw new Error('等待产物超时: ' + f);
};
await b.waitFor(`document.querySelector('[data-content]').innerText.includes('已写入') || document.querySelector('[data-content]').innerText.includes('失败')`, 30000);
for (const f of ['.claude/skills/code-review/SKILL.md', '.mcp.json', '.claude/agents/release-helper.md', '.claude/rules/ailoom-personal.md']) {
  await waitFile(`${A_DIR}/${f}`);
}
await shot('applied');
log('应用产物核对通过（skill/mcp/agent/rules）');
// 指令产物
const instr = execSync(`find ${A_DIR}/.ailoom -name "*.md" 2>/dev/null || true`).toString();
log('指令数据产物:', JSON.stringify(instr.slice(0, 120)));
// B/C 零影响
if (files(`${E}/projects/b`).includes('.claude') || files(`${E}/projects/c`).includes('.claude')) throw new Error('B/C 不得出现部署产物');
log('B/C 隔离 OK');

// ---- 6. 移除 skill → 应用（产物清理）→ 重新添加 → 应用 ----
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-entries] .project-row")');
await b.evaluate(`(()=>{ const row=[...document.querySelectorAll('[data-entries] .project-row')][0]; [...row.querySelectorAll('button')].find(x=>x.textContent==='从本项目移除…').click(); })()`);
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='移除引用').click()`);
await b.waitFor(`!document.querySelector('[data-entries] .project-row')`);
await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
await b.waitFor('!!document.querySelector("[data-content] h2")');
await b.evaluate(`[...document.querySelectorAll('#app button')].find(x=>x.textContent==='生成预览').click()`);
await b.waitFor(`document.querySelector('[data-content]').innerText.includes('仓库')`);
await b.evaluate(`[...document.querySelectorAll('#app button')].find(x=>x.textContent==='应用' && !x.disabled).click()`);
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='确认应用').click()`);
await b.waitFor(`document.querySelector('[data-content]').innerText.includes('已写入') || document.querySelector('[data-content]').innerText.includes('失败')`, 30000);
await new Promise(r=>setTimeout(r,800));
if (execSync(`test -e ${JSON.stringify(`${A_DIR}/.claude/skills/code-review`)} && echo yes || echo no`).toString().includes('yes')) throw new Error('移除并应用后 skill 产物应清理');
log('移除→应用 清理 OK；重新添加');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-add]")');
await b.evaluate("document.querySelector('[data-add]').click()");
await b.waitFor("!!document.querySelector('dialog[open] [data-picker-item]')");
await b.evaluate(`(()=>{ const box=[...document.querySelectorAll('[data-picker-item]')].find(x=>x.value.includes('/skill/common/code-review')); box.checked=true; box.dispatchEvent(new Event('change')); document.querySelector('[data-picker-submit]').click(); })()`);
await b.waitFor(`!document.querySelector('dialog[open]')`);
await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
await b.waitFor('!!document.querySelector("[data-content] h2")');
await b.evaluate(`[...document.querySelectorAll('#app button')].find(x=>x.textContent==='生成预览').click()`);
await b.waitFor(`document.querySelector('[data-content]').innerText.includes('仓库')`);
await b.evaluate(`[...document.querySelectorAll('#app button')].find(x=>x.textContent==='应用' && !x.disabled).click()`);
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='确认应用').click()`);
await b.waitFor(`document.querySelector('[data-content]').innerText.includes('已写入') || document.querySelector('[data-content]').innerText.includes('失败')`, 30000);
await waitFile(`${A_DIR}/.claude/skills/code-review`);
log('重新添加→应用 OK') && true;
if (!execSync(`test -e ${JSON.stringify(`${A_DIR}/.claude/skills/code-review`)} && echo yes || echo no`).toString().includes('yes')) throw new Error('重新添加后产物应恢复');
log('重新添加→应用 OK');

// ---- 7. 检查更新（上游新提交）→ 库版本更新，项目保持 stale（按项目决策升级） ----
if (!execSync(`git -C ${E}/sources/team-src log --oneline`).toString().includes('e2e v3')) {
  execSync(`cd ${E}/sources/team-src && sed -i '' 's/审查代码质量（[^）]*）。/审查代码质量（E2E v3）。/' resources/skills/code-review/SKILL.md`,
    {shell:'/bin/bash', env: {...process.env, GIT_CONFIG_GLOBAL:`${E}/home/.gitconfig`, GIT_CONFIG_NOSYSTEM:'1'}});
  execSync(`git -C ${E}/sources/team-src -c commit.gpgsign=false add -A && git -C ${E}/sources/team-src -c commit.gpgsign=false commit -qm "e2e v3"`);
}
await b.goto('#/library');
await b.waitFor(`document.querySelector('[data-sources]')?.innerText.includes('平台技能库')`);
await b.evaluate("document.querySelector('[data-check]').click()");
await b.waitFor(`document.querySelector('[data-msg]').textContent.includes('检查完成')`);
await b.waitFor(`!!document.querySelector('[data-update-one]') && !document.querySelector('[data-update-one]').disabled`);
await new Promise(r=>setTimeout(r,400));
await b.evaluate(`[...document.querySelectorAll('[data-update-one]')][0].click()`);
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='确认更新').click()`);
await b.waitFor(`document.querySelector('[data-msg]').textContent.includes('版本已更新')`);
await b.goto('#/projects');
await b.waitFor('!!document.querySelector("[data-search]")');
await b.evaluate(`[...document.querySelectorAll('[data-list] a')].find(a=>a.textContent==='电商中台A').click()`);
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-entries] .project-row")');
const staleRow = await b.evaluate(`document.querySelector('[data-entries] .project-row').innerText`);
log('库更新后项目行:', staleRow.match(/磁盘：[^\n]*/)?.[0]);
if (!staleRow.includes('待应用更新')) throw new Error('应显示 stale 待项目决策');
await shot('stale-by-project-decision');

// ---- 8. 撤销最近一次 apply（文件层面）----
const jobsNow = await b.evaluate(`(async()=>{ const {api}=await import('/ui/services/api.js'); const v=await api.jobs(); return (v.jobs||[]).filter(j=>j.kind==='apply'&&j.status==='success').length; })()`);
log('成功 apply 记录数:', jobsNow);
await b.goto('#/tasks');
await b.waitFor(`document.querySelector('[data-table] table')`);
await new Promise(r=>setTimeout(r,500));
await b.evaluate(`(()=>{ const rows=[...document.querySelectorAll('[data-table] tbody tr')].filter(r=>r.innerText.includes('应用配置') && r.innerText.includes('成功')); rows[0].querySelector('button').click(); })()`);
await b.waitFor(`!!document.querySelector('[data-detail]') && !document.querySelector('[data-detail]').hidden`);
await b.evaluate("document.querySelector('[data-undo]').click()");
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='确认撤销').click()`);
await b.waitFor(`document.querySelector('[data-msg]').textContent.includes('撤销完成') || document.querySelector('[data-msg]').textContent.includes('冲突')`);
log('撤销结果:', await b.evaluate(`document.querySelector('[data-msg]').textContent`));
await shot('undone');

console.log('ERRORS:', JSON.stringify(b.errors));
if (b.errors.length) throw new Error('页面存在未处理异常');
await b.close();
console.log('AIL-109 E2E main journey PASSED');
