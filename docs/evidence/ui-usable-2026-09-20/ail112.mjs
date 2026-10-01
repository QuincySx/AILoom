// AIL-112：未托管 Skill 双重确认删除 —— 真实 UI 两步 Dialog + 服务端令牌/指纹/归档。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const A = `${S}/知识库 甲`;
const EV = `${S}/evidence`;
const b = await connect('http://127.0.0.1:8646/');
const log = (...a) => console.log('[AIL-112]', ...a);
const shot = n => b.screenshot(`${EV}/ail112-${n}.png`);
const sh = cmd => execSync(cmd).toString();
const exists = p => { try { return sh(`/bin/test -e "${p}" && echo exists`).trim() === 'exists'; } catch { return false; } };
const TOK = () => sh(`grep '/?token=' ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
const POST = (p, body) => sh(`curl -s -X POST http://127.0.0.1:8646${p} -H "X-AILoom-Session: ${TOK()}" -H 'Content-Type: application/json' -d '${body}'`);
let failures = [];
async function openDeleteFlow(name) {
  await b.evaluate(`(function(){ const btn=[...document.querySelectorAll('[data-delete-path]')].find(x=>x.dataset.deleteName===${JSON.stringify(name)}); btn.click(); })()`);
  await b.waitFor(`[...document.querySelectorAll('dialog[open] button')].some(x=>x.textContent==='继续：输入名称确认')`, 15000);
  await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='继续：输入名称确认').click()`);
  // 限定当前打开 dialog 内的输入框，并在点击前读回校验
  await b.waitFor(`[...document.querySelectorAll('dialog[open] [data-delete-name]')].length === 1`, 15000);
  await b.evaluate(`(function(){ const i=document.querySelector('dialog[open] [data-delete-name]'); i.value=${JSON.stringify(name)}; i.dispatchEvent(new Event('input')); })()`);
  const readback = await b.evaluate(`document.querySelector('dialog[open] [data-delete-name]')?.value`);
  if (readback !== name) throw new Error('输入未生效: ' + readback);
  await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='确认删除').click()`);
}
const expect = (c, n) => { if (c) log('PASS', n); else { failures.push(n); log('FAIL', n); } };

await b.waitFor('!!document.querySelector("#nav")');
const aid = sh(`curl -s http://127.0.0.1:8646/api/state | python3 -c "import json,sys; print([r['repo_id'] for r in json.load(sys.stdin)['repos'] if '知识库 甲' in (r.get('common_dir') or '')][0])"`).trim();
POST('/api/fs/approve', JSON.stringify({ path: '/private/tmp/ailoom-usable/知识库 甲' }));

// 准备独立删除对象（不影响其他夹具）：复制会议纪要为「待删除技能」
const TARGET = `${A}/skills/待删除技能`;
sh(`rm -rf "${TARGET}"; mkdir -p "${TARGET}/assets"; cp -c "${A}/skills/会议纪要/SKILL.md" "${TARGET}/SKILL.md"; echo img > "${TARGET}/assets/pic.txt"`);

await b.goto('#/projects/' + encodeURIComponent(aid));
await b.waitFor('!!document.querySelector("[data-tab]")', 20000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`!!document.querySelector('[data-scan]')`, 15000);
await b.evaluate(`document.querySelector('[data-scan-sub]').value='skills'; document.querySelector('[data-scan]').click()`);
await b.waitFor(`document.querySelector('[data-scan-results]')?.textContent.includes('待删除技能')`, 15000);

// ---- 两步 Dialog：第一层影响预览 ----
await b.evaluate(`(function(){ const btn=[...document.querySelectorAll('[data-delete-path]')].find(x=>x.dataset.deleteName==='待删除技能'); btn.click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open]')].some(d=>d.textContent.includes('删除未托管 Skill'))`, 15000);
const dlg1 = await b.evaluate(`[...document.querySelectorAll('dialog[open]')].map(d=>d.textContent).join('')`);
expect(dlg1.includes('2 个文件') && dlg1.includes('project-archive'), '第一层显示精确目录、文件数与归档恢复方式');
await shot('01-delete-step1-impact');

// ---- 取消零写入 ----
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='取消').click()`);
await new Promise(r => setTimeout(r, 200));
expect(sh(`/bin/test -d "${TARGET}" && echo exists`).trim() === 'exists', '第一层取消零写入');

// ---- 第二层：名称不符被拒；正确名称执行 ----
await b.evaluate(`(function(){ const btn=[...document.querySelectorAll('[data-delete-path]')].find(x=>x.dataset.deleteName==='待删除技能'); btn.click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open] button')].some(x=>x.textContent==='继续：输入名称确认')`, 15000);
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='继续：输入名称确认').click()`);
await b.waitFor(`!!document.querySelector('[data-delete-name]')`, 15000);
await b.evaluate(`document.querySelector('[data-delete-name]').value='错误名称'; document.querySelector('[data-delete-name]').dispatchEvent(new Event('input'))`);
await shot('02-delete-step2-confirm');
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='确认删除').click()`);
await b.waitFor(`document.querySelector('[data-delete-err]')?.textContent.includes('不一致')`, 15000);
expect(true, '第二层名称不符被拒绝（目录未删除）');
expect(sh(`/bin/test -d "${TARGET}" && echo exists`).trim() === 'exists', '名称错误时零写入');
// 名称错误已作废令牌（防爆破）：关闭对话框，重新走完整两步流程
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='取消').click()`);
await new Promise(r => setTimeout(r, 200));
await openDeleteFlow('待删除技能');
try {
  await b.waitFor(`document.querySelector('[data-message]')?.textContent.includes('已删除并移入归档')`, 15000);
} catch (e) {
  log('诊断 err:', await b.evaluate(`document.querySelector('[data-delete-err]')?.textContent`));
  log('诊断 dialogs:', JSON.stringify(await b.evaluate(`[...document.querySelectorAll('dialog')].filter(d=>d.open).map(d=>({t:d.querySelector('h2')?.textContent, err:d.querySelector('[data-delete-err]')?.textContent, name:d.querySelector('[data-delete-name]')?.value}))`)));
  log('诊断 msg:', await b.evaluate(`document.querySelector('[data-message]')?.textContent`));
  throw e;
}
await shot('03-deleted-archived');
expect(!exists(`${TARGET}`), '目录已从项目移除');
const archived = sh(`ls ${S}/data/project-archive/ | grep 待删除技能 | head -1`).trim();
expect(!!archived && exists(`${S}/data/project-archive/${archived}/SKILL.md`), `归档可恢复（${archived}，含 SKILL.md）`);

// ---- 双击/令牌重放保护：直接用旧 token 再执行 ----
// （从归档恢复目录到原位置，模拟重新出现）
sh(`rm -rf "${TARGET}"; mkdir -p "${TARGET}"; cp -c "${S}/data/project-archive/${archived}/SKILL.md" "${TARGET}/SKILL.md"`);
const tokOld = sh(`curl -s -X POST http://127.0.0.1:8646/api/project/delete-skill -H "X-AILoom-Session: $(grep '/?token=' ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/')" -H 'Content-Type: application/json' -d '{"path":"${TARGET}","execute":false}' | python3 -c "import json,sys; print(json.load(sys.stdin)['token'])"`).trim();
POST('/api/project/delete-skill', JSON.stringify({ token: tokOld, name: '待删除技能', execute: true }));
const r2 = sh(`curl -s -X POST http://127.0.0.1:8646/api/project/delete-skill -H "X-AILoom-Session: $(grep '/?token=' ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/')" -H 'Content-Type: application/json' -d '{"token":"${tokOld}","name":"待删除技能","execute":true}'`);
expect(r2.includes('已使用') || r2.includes('重新预览'), `令牌单次有效（双击/重放被拒）：${r2.slice(0, 60)}`);

// ---- 中途替换目录 → 指纹不一致拒绝 ----
sh(`rm -rf "${TARGET}"; mkdir -p "${TARGET}"; cp -c "${A}/skills/会议纪要/SKILL.md" "${TARGET}/SKILL.md"; echo extra > "${TARGET}/new.txt"`);
const tok3 = sh(`curl -s -X POST http://127.0.0.1:8646/api/project/delete-skill -H "X-AILoom-Session: $(grep '/?token=' ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/')" -H 'Content-Type: application/json' -d '{"path":"${TARGET}","execute":false}' | python3 -c "import json,sys; print(json.load(sys.stdin)['token'])"`).trim();
sh(`rm "${TARGET}/new.txt"`);
const r3 = POST('/api/project/delete-skill', JSON.stringify({ token: tok3, name: '待删除技能', execute: true }));
expect(r3.includes('内容已变化'), `目录被替换后拒绝执行：${r3.slice(0, 50)}`);

// ---- 路径穿越 / 项目根 / 无 SKILL.md 拒绝 ----
const r4 = POST('/api/project/delete-skill', JSON.stringify({ path: `${A}/skills/../skills`, execute: false }));
const r5 = POST('/api/project/delete-skill', JSON.stringify({ path: `${A}/skills/会议纪要/nested-nonexistent`, execute: false }));
const r6 = POST('/api/project/delete-skill', JSON.stringify({ path: `${A}`, execute: false }));
expect(r4.includes('Skill 目录') || r4.includes('边界') || r4.includes('拒绝'), `路径穿越被拒`);
expect(r5.includes('Skill 目录') || r5.includes('不可访问') || r5.includes('路径不可用'), `不存在目录被拒`);
expect(r6.includes('批准根本身') || r6.includes('Skill 目录'), `项目根被拒`);

// ---- 服务重启 → 令牌失效 ----
const tok7 = sh(`curl -s -X POST http://127.0.0.1:8646/api/project/delete-skill -H "X-AILoom-Session: $(grep '/?token=' ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/')" -H 'Content-Type: application/json' -d '{"path":"${TARGET}","execute":false}' | python3 -c "import json,sys; print(json.load(sys.stdin)['token'])"`).trim();
sh(`curl -s -X POST http://127.0.0.1:8646/api/shutdown -H "X-AILoom-Session: ${TOK()}" -d '{}' >/dev/null; sleep 1.2`);
execSync(`cd ${S} && HOME=${S}/home XDG_STATE_HOME=${S}/xdg-state XDG_DATA_HOME=${S}/xdg-data GIT_CONFIG_GLOBAL=${S}/home/.gitconfig GIT_CONFIG_NOSYSTEM=1 nohup <repo>/target/debug/ailoom --data-root ${S}/data console --port 8646 --no-open >> ${EV}/console.log 2>&1 &`, { shell: '/bin/zsh' });
for (let i = 0; i < 40; i++) { try { execSync('curl -s -o /dev/null http://127.0.0.1:8646/api/state'); break; } catch { execSync('sleep 0.5'); } }
const r7 = POST('/api/project/delete-skill', JSON.stringify({ token: tok7, name: '待删除技能', execute: true }));
expect(r7.includes('重新预览') || r7.includes('不存在'), `重启后旧确认失效：${r7.slice(0, 50)}`);

// ---- 符号链接：只摘除链接本身 ----
// （独立目标目录；与「链接到乙」同目标会被按真实身份去重——该去重行为已在 AIL-111 覆盖）
sh(`mkdir -p "${S}/知识库 乙/skills/删除链接目标"; printf -- '---\nname: 删除链接目标\ndescription: 链接目标探针\n---\n# x\n' > "${S}/知识库 乙/skills/删除链接目标/SKILL.md"` );
sh(`rm -rf "${TARGET}"; ln -s "${S}/知识库 乙/skills/删除链接目标" "${TARGET}"`);
await b.evaluate(`location.reload()`);
await b.waitFor('!!document.querySelector("#nav")', 20000);
await b.goto('#/projects/' + encodeURIComponent(aid));
await b.waitFor(`!!document.querySelector('[data-tab]')`, 20000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`!!document.querySelector('[data-scan]')`, 20000);
await b.evaluate(`document.querySelector('[data-scan-sub]').value='skills'; document.querySelector('[data-scan]').click()`);
await b.waitFor(`document.querySelector('[data-scan-results]')?.textContent.includes('待删除技能')`, 15000);
await b.evaluate(`(function(){ const btn=[...document.querySelectorAll('[data-delete-path]')].find(x=>x.dataset.deleteName==='待删除技能'); btn.click(); })()`);
await b.waitFor(`[...document.querySelectorAll('dialog[open] button')].some(x=>x.textContent==='继续：输入名称确认')`, 15000);
const symText = await b.evaluate(`[...document.querySelectorAll('dialog[open]')].map(d=>d.textContent).join('')`);
expect(symText.includes('只摘除链接本身'), '符号链接在预览中声明只摘除链接');
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='继续：输入名称确认').click()`);
await b.waitFor(`!!document.querySelector('[data-delete-name]')`, 15000);
// 名称错误已作废令牌（防爆破）：关闭对话框，重新走完整两步流程
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='取消').click()`);
await new Promise(r => setTimeout(r, 200));
await openDeleteFlow('待删除技能');
try {
  await b.waitFor(`document.querySelector('[data-message]')?.textContent.includes('已删除并移入归档')`, 15000);
} catch (e) {
  log('诊断 err:', await b.evaluate(`document.querySelector('[data-delete-err]')?.textContent`));
  log('诊断 dialogs:', JSON.stringify(await b.evaluate(`[...document.querySelectorAll('dialog')].filter(d=>d.open).map(d=>({t:d.querySelector('h2')?.textContent, err:d.querySelector('[data-delete-err]')?.textContent, name:d.querySelector('[data-delete-name]')?.value}))`)));
  log('诊断 msg:', await b.evaluate(`document.querySelector('[data-message]')?.textContent`));
  throw e;
}
expect(!exists(`${TARGET}`) && exists(`${S}/知识库 乙/skills/删除链接目标`), '链接被摘除且目标目录完好');
await shot('04-symlink-link-only');

// 清理：恢复原始状态（外部链接形式的链接到乙 仍在 skills/ 下，不影响其他卡）
sh(`rm -rf "${TARGET}" "${S}/知识库 乙/skills/删除链接目标"`);
await b.evaluate(`location.reload()`);
await b.waitFor('!!document.querySelector("#nav")', 20000);

console.log(JSON.stringify({ failures }, null, 2));
await b.close();
if (failures.length) process.exitCode = 1;
