// AIL-113：项目内搜索选择与加号导入连续流程 —— 全程不离开项目页。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const A = `${S}/知识库 甲`;
const EV = `${S}/evidence`;
const IMPORT_DIR = `${S}/导入源/检索技巧`;
const b = await connect('http://127.0.0.1:8646/');
const log = (...a) => console.log('[AIL-113]', ...a);
const shot = n => b.screenshot(`${EV}/ail113-${n}.png`);
const sh = cmd => execSync(cmd).toString();
const api = p => JSON.parse(sh(`curl -s "http://127.0.0.1:8646${p}"`));
let failures = [];
const expect = (c, n) => { if (c) log('PASS', n); else { failures.push(n); log('FAIL', n); } };

await b.waitFor('!!document.querySelector("#nav")');
const nongitId = sh(`ls ${S}/data/repos | grep nongit- | head -1`).trim();

// ---- 重复执行清理（真实服务调用，不手改数据）：删除上次运行留下的个人副本与引用 ----
{
  const tok = sh(`grep "/?token=" ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
  sh(`curl -s -X POST http://127.0.0.1:8646/api/fs/approve -H "X-AILoom-Session: ${tok}" -H 'Content-Type: application/json' -d '{"path":"/private/tmp/ailoom-usable/知识库 甲"}' >/dev/null`);
  sh(`curl -s -X POST http://127.0.0.1:8646/api/profile/select -H "X-AILoom-Session: ${tok}" -H 'Content-Type: application/json' -d '{"root":"${A}","resource":"personal/skill/personal/retrieval-tips","state":"inherit"}' >/dev/null`);
  sh(`curl -s -X POST http://127.0.0.1:8646/api/library/delete -H "X-AILoom-Session: ${tok}" -H 'Content-Type: application/json' -d '{"id":"personal/skill/personal/retrieval-tips","execute":true}' >/dev/null`);
}

// F03 契约：导入前个人库为空；导入后 /api/resources 条目必须有 name/path
const before = api('/api/resources');
expect((before.entries || []).filter(e => e.kind === 'skill').length === 0, '初始个人库为空（乙 skill 未导入，扫描不等于入库）');

await b.goto('#/projects/' + encodeURIComponent(nongitId));
await b.waitFor('!!document.querySelector("[data-tab]")');
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor('!!document.querySelector("[data-add]")');
const addBtnDisabled = await b.evaluate(`document.querySelector('[data-add]').disabled`);
expect(!addBtnDisabled, 'F02：空库存时「添加Skill」仍可点击');

await b.evaluate(`document.querySelector('[data-add]').click()`);
await b.waitFor(`document.querySelector('[data-picker-list]')?.closest('dialog')?.matches(':modal')`);
await shot('01-empty-picker-with-import');
const pickerText = await b.evaluate(`document.querySelector('[data-picker-list]').textContent`);
expect(pickerText.includes('还没有可选资源'), '空库存如实显示，不显示 undefined');
expect(!!await b.evaluate(`document.querySelector('[data-picker-import]')`), '选择器内提供「＋导入」');

// ＋导入 → 内联 ImportDialog → 本地导入（真实入库，不离开项目页）
await b.evaluate(`document.querySelector('[data-picker-import]').click()`);
await b.waitFor(`document.querySelector('[data-provider]')?.closest('dialog')?.matches(':modal')`);
await shot('02-inline-import-dialog');
await b.evaluate(`document.querySelector('[data-provider]').value='local'; document.querySelector('[data-provider]').dispatchEvent(new Event('change'))`);
await b.evaluate(`document.querySelector('[data-url]').value=${JSON.stringify(IMPORT_DIR)}`);
// 中文目录名是合法来源但不是合法存储名（E3002）：提供 ASCII 存储名称。
await b.evaluate(`document.querySelector('[data-name]').value='retrieval-tips'`);
await b.evaluate(`document.querySelector('[data-form]').requestSubmit()`);
await b.waitFor(`document.querySelector('[data-candidate]')?.textContent.includes('确认导入内容')`);
await shot('03-import-preview');
await b.evaluate(`document.querySelector('[data-confirm]').click()`);
// 导入成功 → 回到选择器并自动勾选新条目
await b.waitFor(`document.querySelector('[data-picker-list]')?.textContent.includes('retrieval-tips')`, 15000);
await shot('04-picker-with-imported');
const importedChecked = await b.evaluate(`document.querySelector('[data-picker-item]:checked')?.value || ''`);
expect(importedChecked.includes('retrieval-tips') || importedChecked !== '', '导入成功后自动选中新条目');
expect(!!await b.evaluate(`document.querySelector('[data-picker-list]')?.textContent.includes('个人资源库')`), '新条目按来源分组显示');

// 按真实名称搜索（name 来自 F03 修复，非 undefined）
await b.evaluate(`document.querySelector('[data-picker-search]').value='检索'; document.querySelector('[data-picker-search]').dispatchEvent(new Event('input'))`);
await b.waitFor(`document.querySelector('[data-picker-list]').textContent.includes('retrieval-tips')`);
await b.evaluate(`document.querySelector('[data-picker-search]').value=''; document.querySelector('[data-picker-search]').dispatchEvent(new Event('input'))`);
const rowText = await b.evaluate(`document.querySelector('[data-picker-list]').textContent`);
expect(rowText.includes('retrieval-tips') && rowText.includes('用关键词与过滤条件快速检索资料库'), 'F03：本地副本按真实名称+说明展示（不 undefined）');

// F03 契约（真实 API）
const after = api('/api/resources');
const entry = (after.entries || []).find(e => e.id.includes('retrieval-tips'));
expect(!!entry && !!entry.name && !!entry.path, `F03 契约：/api/resources 条目带 name=${entry?.name} path=${entry?.path}`);

// 提交（添加所选）→ 引用保存，不离开项目
await b.evaluate(`document.querySelector('[data-picker-submit]').click()`);
await b.waitFor(`document.querySelector('[data-message]')?.textContent.includes('已保存 1 条引用')`, 15000);
await shot('05-added-reference');
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('retrieval-tips')`, 15000);
const refRow = await b.evaluate(`document.querySelector('[data-entries]')?.textContent || ''`);
expect(refRow.includes('retrieval-tips'), '引用列表出现检索技巧（仍在项目页内）');
const hash1 = await b.evaluate(`location.hash`);
expect(hash1.startsWith('#/projects/nongit-'), `全程未离开项目页（${hash1}）`);

// 再开选择器：已添加禁选；取消零写入
await b.evaluate(`document.querySelector('[data-add]').click()`);
await b.waitFor(`document.querySelector('[data-picker-list]')?.closest('dialog')?.matches(':modal')`);
const addedDisabled = await b.evaluate(`document.querySelector('[data-picker-item]').disabled`);
expect(addedDisabled, '已添加项在选择器中禁选并标注');
await b.evaluate(`document.querySelector('[data-picker-cancel]').click()`);
await new Promise(r => setTimeout(r, 300));
const libAfterCancel = api('/api/resources');
expect(JSON.stringify(libAfterCancel.entries) === JSON.stringify(after.entries), '取消选择器零写入');

// 导入取消路径：打开 ＋导入 → 直接取消 → 资源库不变、选择器可继续用
await b.evaluate(`document.querySelector('[data-add]').click()`);
await b.waitFor(`document.querySelector('[data-picker-import]')`);
await b.evaluate(`document.querySelector('[data-picker-import]').click()`);
await b.waitFor(`document.querySelector('[data-provider]')?.closest('dialog')?.matches(':modal')`);
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='取消').click()`);
await new Promise(r => setTimeout(r, 300));
const libAfterImportCancel = api('/api/resources');
expect(JSON.stringify(libAfterImportCancel.entries) === JSON.stringify(after.entries), '导入取消零写入（资源库不变）');
await b.evaluate(`[...document.querySelectorAll('dialog[open] button')].find(x=>x.textContent==='取消').click()`);
await new Promise(r => setTimeout(r, 200));

// 重启后：全局条目与项目引用一致
const token = sh(`grep "/?token=" ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
sh(`curl -s -X POST http://127.0.0.1:8646/api/shutdown -H "X-AILoom-Session: ${token}" -d '{}'`);
await new Promise(r => setTimeout(r, 1200));
execSync(`cd ${S} && HOME=${S}/home XDG_STATE_HOME=${S}/xdg-state XDG_DATA_HOME=${S}/xdg-data GIT_CONFIG_GLOBAL=${S}/home/.gitconfig GIT_CONFIG_NOSYSTEM=1 nohup <repo>/target/debug/ailoom --data-root ${S}/data console --port 8646 --no-open >> ${EV}/console.log 2>&1 &`, { shell: '/bin/zsh' });
for (let i = 0; i < 40; i++) { try { api('/api/state'); break; } catch { execSync('sleep 0.5'); } }
const t2 = sh(`grep "/?token=" ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
sh(`curl -s -X POST http://127.0.0.1:8646/api/fs/approve -H "X-AILoom-Session: ${t2}" -H 'Content-Type: application/json' -d '{"path":"/private/tmp/ailoom-usable/知识库 甲"}' >/dev/null`);
const afterRestart = api('/api/resources');
expect((afterRestart.entries || []).some(e => e.id.includes('retrieval-tips')), '重启后全局条目仍在');
const prof = sh(`cat ${S}/data/profile/profile.toml`);
expect(prof.includes('retrieval-tips'), '重启后项目引用仍在（profile 持久化）');
await b.evaluate(`location.reload()`);
await b.waitFor('!!document.querySelector("#nav")', 20000);
await b.goto('#/projects/' + encodeURIComponent(nongitId));
await b.waitFor('!!document.querySelector("[data-tab]")', 20000);
await b.evaluate(`document.querySelector('[data-tab="1"]').click()`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('retrieval-tips')`, 20000);
expect(true, '重启后项目页仍显示引用');

// 导入未暗启用其他项目：乙无任何引用
const otherId = api('/api/state').repos.find(r => r.repo_id.startsWith('nongit-') && r.common_dir.includes('乙')).repo_id;
const t3 = sh(`grep "/?token=" ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
sh(`curl -s -X POST http://127.0.0.1:8646/api/fs/approve -H "X-AILoom-Session: ${t3}" -H 'Content-Type: application/json' -d '{"path":"/private/tmp/ailoom-usable/知识库 乙"}' >/dev/null`);
const effB = api(`/api/effective?root=${encodeURIComponent(`${S}/知识库 乙`)}`);
expect(!Object.keys(effB.resources || {}).length, '导入没有暗启用其他项目（乙无引用）');

console.log(JSON.stringify({ failures }, null, 2));
await b.close();
if (failures.length) process.exitCode = 1;
