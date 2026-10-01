// AIL-115 门禁：两个非 Git 知识库 + 一个 Git 项目的完整演示。
// A 为主对象：扫描可见 → 空库内联导入 → 添加 → 选宿主 → 预览 → 应用 → 文件与状态 → 移除 → 再应用。
// B 交叉隔离；全程真实 UI/API/磁盘；不手改任何配置文件。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const A = `${S}/知识库 甲`, B = `${S}/知识库 乙`, C = `${S}/项目 丙`;
const IMPORT_DIR = `${S}/导入源/检索技巧`;
const EV = `${S}/evidence`;
const b = await connect('http://127.0.0.1:8646/');
const log = (...a) => console.log('[AIL-115]', ...a);
const shot = n => b.screenshot(`${EV}/ail115-${n}.png`);
const sh = cmd => execSync(cmd).toString();
const api = p => { const r = sh(`curl -s "http://127.0.0.1:8646${p}"`); try { return JSON.parse(r); } catch { throw new Error('API 非 JSON: ' + p + ' → ' + r.slice(0, 120)); } };
const TOK = () => sh(`grep '/?token=' ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
const POST = (p, body) => sh(`curl -s -X POST http://127.0.0.1:8646${p} -H "X-AILoom-Session: ${TOK()}" -H 'Content-Type: application/json' -d '${body}'`);
let failures = [];
const expect = (c, n) => { if (c) log('PASS', n); else { failures.push(n); log('FAIL', n); } };
const kbHashes = () => sh(`find "${A}" "${B}" -type f -not -path '*/.git/*' -not -path '*/.claude/*' -not -path '*/.agents/*' -not -name 'AGENTS.override.md' -exec shasum -a 256 {} \\; | sort`);
const canon = p => sh(`cd ${JSON.stringify(p)} && pwd -P`).trim();

async function openPicker() {
  for (let i = 0; i < 8; i++) {
    await b.evaluate(`document.querySelector('[data-add]')?.click()`);
    try { await b.waitFor(`[...document.querySelectorAll('[data-picker-list]')].some(el => el.closest('dialog')?.matches(':modal'))`, 2500); return; }
    catch (e) { /* 重试 */ }
  }
  throw new Error('选择器始终未打开');
}
async function pickerCheck(nameFragment) {
  await b.evaluate(`(function(){ const box=[...document.querySelectorAll('[data-picker-item]')].find(b=>b.value.includes(${JSON.stringify(nameFragment)}) && !b.disabled); box.checked=true; box.dispatchEvent(new Event('change')); })()`);
}
async function planApplyApply(tag) {
  await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
  await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='生成预览')");
  await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='生成预览').click()`);
  await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)", 20000);
  await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用' && !b.disabled).click()`);
  await b.waitFor("[...document.querySelectorAll('dialog:modal .dialog-actions button')].some(x=>x.textContent==='确认应用')");
  await b.evaluate(`[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(x=>x.textContent==='确认应用').click()`);
  await b.waitFor(`document.querySelector('[data-content]')?.textContent.includes('应用完成')`, 25000);
  await shot(tag);
}

await b.waitFor('!!document.querySelector("#nav")');

// ---------- 登记（真实新建项目 Dialog）----------
async function addProject(path, name, category) {
  await b.goto('#/projects');
  await b.waitFor('!!document.querySelector("[data-new]")');
  await b.evaluate(`(async()=>{ document.querySelector('[data-new]').click(); await new Promise(r=>setTimeout(r,150));
    document.querySelector('[data-path]').value=${JSON.stringify(path)};
    document.querySelector('[data-name]').value=${JSON.stringify(name)};
    document.querySelector('[data-category]').value=${JSON.stringify(category)};
    document.querySelector('[data-create]').requestSubmit(); })()`);
  await b.waitFor(`document.querySelector('[data-header-path]')?.textContent === ${JSON.stringify(canon(path))}`, 20000);
}
await addProject(A, '知识库甲', '知识库');
await addProject(B, '知识库乙', '知识库');
await addProject(C, '项目丙', '工作');
await b.goto('#/projects');
await b.waitFor('!!document.querySelector("[data-list]")');
await shot('01-registered-3-projects');
expect(api('/api/state').repos.length === 3, '甲/乙/丙 全部登记（甲乙为普通文件夹，无 Git）');

// ---------- A：扫描未托管 Skill（F01 能力）----------
const aid = api('/api/state').repos.find(r => r.common_dir?.includes('知识库 甲')).repo_id;
await b.goto('#/projects/' + encodeURIComponent(aid));
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`!!document.querySelector('[data-scan]')`, 15000);
await b.evaluate(`document.querySelector('[data-scan-sub]').value='skills'; document.querySelector('[data-scan]').click()`);
await b.waitFor(`document.querySelector('[data-scan-results]')?.textContent.includes('会议纪要')`, 15000);
await shot('02-a-scan-unmanaged');
const scanText = await b.evaluate(`document.querySelector('[data-skills-scan]').textContent`);
expect(scanText.includes('会议纪要') && scanText.includes('项目自有（未托管）'), 'A 未托管 Skill 扫描可见');
expect(scanText.includes('周报生成'), 'A 嵌套 Skill 扫描可见');
expect(scanText.includes('损坏技能') && scanText.includes('缺少 frontmatter'), 'A 损坏条目独立报错');
expect(scanText.includes('链接到乙') && scanText.includes('未解析其内容'), 'A 越界链接不读取内容');

// ---------- A：空资源库内联导入 + 添加（重复执行时若已引用则跳过）----------
const alreadyAdded = await b.evaluate(`document.querySelector('[data-entries]')?.textContent.includes('retrieval-tips')`);
if (!alreadyAdded) {
await openPicker();
await b.evaluate(`document.querySelector('[data-picker-import]').click()`);
await b.waitFor(`document.querySelector('[data-provider]')?.closest('dialog')?.matches(':modal')`, 15000);
await b.evaluate(`document.querySelector('[data-provider]').value='local'; document.querySelector('[data-provider]').dispatchEvent(new Event('change'))`);
await b.evaluate(`document.querySelector('[data-url]').value=${JSON.stringify(IMPORT_DIR)}; document.querySelector('[data-name]').value='retrieval-tips'`);
await b.evaluate(`document.querySelector('[data-form]').requestSubmit()`);
await b.waitFor(`document.querySelector('[data-candidate]')?.textContent.includes('确认导入内容')`, 15000);
await shot('03-inline-import-preview');
await b.evaluate(`document.querySelector('[data-confirm]').click()`);
await b.waitFor(`[...document.querySelectorAll('[data-picker-list]')].some(el => el.textContent.includes('retrieval-tips'))`, 15000);
await b.evaluate(`(function(){ const box=[...document.querySelectorAll('[data-picker-item]')].find(x=>x.value.includes('retrieval-tips')&&!x.disabled); box.checked=true; box.dispatchEvent(new Event('change')); })()`);
await shot('04-imported-and-selected');
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
await b.waitFor(`document.querySelector('[data-message]')?.textContent.includes('已保存 1 条引用')`, 15000);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('retrieval-tips')`, 15000);
expect(true, 'A 内联导入并添加引用（全程未离开项目页）');
} else {
  log('retrieval-tips 已引用（重复执行），跳过内联导入');
}

// ---------- A：选宿主 ----------
await b.evaluate(`document.querySelector('[data-tab="0"]').click()`);
await b.waitFor('!!document.querySelector("[data-entries] .project-row")', 15000);
await b.evaluate(`(async()=>{ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.querySelector('.row-head strong')?.textContent==='Claude Code');
  row.querySelector('select').value='enable'; [...row.querySelectorAll('button')].find(x=>x.textContent==='保存').click(); })()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存')`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('已启用')`, 15000);
expect(true, 'A 选择 Claude Code 宿主');

// ---------- A：预览 → 应用 → 文件与状态 ----------
await planApplyApply('05-a-applied');
expect(sh(`find "${A}/.claude/skills" -maxdepth 1 -mindepth 1 -name 'retrieval-tips' 2>/dev/null | head -1`).trim() !== '', 'A 磁盘出现托管 Skill（.claude/skills/retrieval-tips）');
const dsA = api(`/api/deploy-status?root=${encodeURIComponent(A)}`);
expect(dsA.items.some(i => i.resource_id.includes('retrieval-tips') && i.state === 'current'), 'A deploy-status：current（文件部署通过；宿主真实加载待验证）');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('磁盘：已部署')`, 15000);
await shot('06-a-status-current');
const hashAfterApply = kbHashes();

// ---------- A：移除 → 应用（清理）→ 再应用（恢复）----------
await b.waitFor('!!document.querySelector("[data-entries] .project-row")', 15000);
await b.evaluate(`(function(){ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.textContent.includes('retrieval-tips'));
  [...row.querySelectorAll('button')].find(x=>x.textContent==='从本项目移除…').click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('清除本层设置'))`, 15000);
await shot('07-a-remove-dialog');
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='清除本层设置').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已移除本层设置')`, 15000);
await b.waitFor(`!document.querySelector('[data-entries]')?.textContent.includes('retrieval-tips')`, 15000);
await planApplyApply('08-a-applied-after-remove');
expect(sh(`find "${A}/.claude/skills" -maxdepth 1 -mindepth 1 2>/dev/null | head -1`).trim() === '', 'A 移除+应用后托管文件被清理');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-add]")', 15000);
await openPicker();
await pickerCheck('retrieval-tips');
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存 1 条引用')`, 15000);
await planApplyApply('09-a-reapplied');
expect(sh(`find "${A}/.claude/skills" -maxdepth 1 -mindepth 1 -name 'retrieval-tips' 2>/dev/null | head -1`).trim() !== '', 'A 再应用后托管文件恢复');

// ---------- B：添加同一库 Skill 并应用 ----------
const bid = api('/api/state').repos.find(r => r.common_dir?.includes('知识库 乙')).repo_id;
await b.goto('#/projects/' + encodeURIComponent(bid));
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.evaluate(`document.querySelector('[data-tab="0"]').click()`);
await b.waitFor('!!document.querySelector("[data-entries] .project-row")', 15000);
await b.evaluate(`(async()=>{ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.querySelector('.row-head strong')?.textContent==='Claude Code');
  row.querySelector('select').value='enable'; [...row.querySelectorAll('button')].find(x=>x.textContent==='保存').click(); })()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存')`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('已启用')`, 15000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-add]")', 15000);
await openPicker();
await pickerCheck('retrieval-tips');
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已保存 1 条引用')`, 15000);
await planApplyApply('10-b-applied');
expect(sh(`find "${B}/.claude/skills" -maxdepth 1 -mindepth 1 -name 'retrieval-tips' 2>/dev/null | head -1`).trim() !== '', 'B 部署同一 Skill 成功');

// ---------- A 再移除：不影响 B ----------
await b.goto('#/projects/' + encodeURIComponent(aid));
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`!!document.querySelector('[data-entries] .project-row')`, 15000);
await b.evaluate(`(function(){ const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.textContent.includes('retrieval-tips'));
  [...row.querySelectorAll('button')].find(x=>x.textContent==='从本项目移除…').click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('清除本层设置'))`, 15000);
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='清除本层设置').click()`);
await b.waitFor(`document.querySelector('[data-message]').textContent.includes('已移除本层设置')`, 15000);
await planApplyApply('11-a-removed-again');
expect(sh(`find "${A}/.claude/skills" -maxdepth 1 -mindepth 1 2>/dev/null | head -1`).trim() === '', 'A 再次移除后文件清理');
expect(sh(`find "${B}/.claude/skills" -maxdepth 1 -mindepth 1 -name 'retrieval-tips' 2>/dev/null | head -1`).trim() !== '', 'B 的部署完全不受 A 移除影响');
expect(api(`/api/deploy-status?root=${encodeURIComponent(B)}`).items.every(i => i.state === 'current'), 'B 状态仍 current');

// ---------- 丙（Git 项目）隔离 ----------
expect(api(`/api/effective?root=${encodeURIComponent(C)}`).hosts?.claude?.enabled !== true, '丙未启用任何宿主（隔离）');

// ---------- 重启持久化 ----------
sh(`curl -s -X POST http://127.0.0.1:8646/api/shutdown -H "X-AILoom-Session: ${TOK()}" -d '{}'`);
await new Promise(r => setTimeout(r, 1500));
execSync(`cd ${S} && HOME=${S}/home XDG_STATE_HOME=${S}/xdg-state XDG_DATA_HOME=${S}/xdg-data GIT_CONFIG_GLOBAL=${S}/home/.gitconfig GIT_CONFIG_NOSYSTEM=1 nohup <repo>/target/debug/ailoom --data-root ${S}/data console --port 8646 --no-open >> ${EV}/console.log 2>&1 &`, { shell: '/bin/zsh' });
for (let i = 0; i < 40; i++) { try { api('/api/state'); break; } catch { execSync('sleep 0.5'); } }
POST('/api/fs/approve', JSON.stringify({ path: canon(A) }));
POST('/api/fs/approve', JSON.stringify({ path: canon(B) }));
expect(api('/api/state').repos.length === 3, '重启后 3 项目仍在');
const libAfter = api('/api/resources').entries || [];
expect(libAfter.some(e => e.name === 'retrieval-tips'), '重启后资源库条目仍在');
const stB = api(`/api/deploy-status?root=${encodeURIComponent(B)}`);
expect(stB.items.length > 0 && stB.items.every(i => i.state === 'current'), '重启后 B 部署状态仍 current');
await b.evaluate(`location.reload()`);
await b.waitFor('!!document.querySelector("#nav")', 20000);
await b.goto('#/projects/' + encodeURIComponent(bid));
await b.waitFor(`document.querySelector('[data-tab]')`, 20000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('retrieval-tips')`, 20000);
await shot('12-after-restart');
expect(true, '重启后重新进入 B 仍见引用与状态');

// ---------- 知识库正文不变 ----------
expect(kbHashes() === hashAfterApply, '全程知识文档与未托管 Skill 文件哈希不变');
expect(!sh(`/bin/ls -a "${A}"`).includes('.git') && !sh(`/bin/ls -a "${B}"`).includes('.git'), '甲乙全程无 .git（未 init、未推远端）');

console.log(JSON.stringify({ failures }, null, 2));
await b.close();
if (failures.length) process.exitCode = 1;
