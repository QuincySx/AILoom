// AIL-110：非 Git 知识库作为主项目的急用闭环 —— 真实 UI + API + 磁盘。
// 甲（非 Git，中文+空格路径）为主操作对象；乙对照；丙为 Git 交叉隔离。
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const S = '/tmp/ailoom-usable';
const A = `${S}/知识库 甲`, B = `${S}/知识库 乙`, C = `${S}/项目 丙`;
const EV = `${S}/evidence`;
const PORT = 8646;
const canon = p => execSync(`cd ${JSON.stringify(p)} && pwd -P`).toString().trim();
const b = await connect(`http://127.0.0.1:${PORT}/`);
const log = (...a) => console.log('[AIL-110]', ...a);
const shot = n => b.screenshot(`${EV}/ail110-${n}.png`);
const sh = cmd => execSync(cmd).toString();
const kbHashes = () => sh(`find "${A}" "${B}" -type f -not -path '*/.git/*' -not -path '*/.claude/*' -not -name 'AGENTS.override.md' -exec shasum -a 256 {} \\; | sort`);
let failures = [];
const expect = (cond, name) => { if (cond) log('PASS', name); else { failures.push(name); log('FAIL', name); } };
const api = p => JSON.parse(sh(`curl -s "http://127.0.0.1:${PORT}${p}"`));
const waitApi = () => { for (let i = 0; i < 40; i++) { try { api('/api/state'); return; } catch { execSync('sleep 0.5'); } } throw new Error('service did not come up'); };

await b.waitFor('!!document.querySelector("#nav")');

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
  await b.waitFor(`document.querySelector('[data-header-path]')?.textContent === ${JSON.stringify(canon(path))}`, 15000);
}

// ---- 1. 取消零写入 ----
await b.goto('#/projects');
await b.waitFor('!!document.querySelector("[data-new]")');
await b.evaluate(`(async()=>{ document.querySelector('[data-new]').click(); await new Promise(r=>setTimeout(r,150)); document.querySelector('[data-path]').value='${A}'; })()`);
await shot('01-new-project-dialog');
await b.evaluate(`document.querySelector('[data-cancel]').click()`);
await new Promise(r => setTimeout(r, 200));
expect(api('/api/state').repos.length === 0, '取消登记零写入（repos 为空）');

// ---- 2. 登记甲/乙/丙 ----
await addProject(A, '知识库甲', '知识库');
await shot('02-a-detail-nongit');
const aBadge = await b.evaluate(`document.querySelector('.config-header')?.textContent`);
expect(aBadge.includes('文件夹项目'), '甲识别为文件夹项目（非 Git，不要求 init/远端）');
expect(aBadge.includes('仅作用于本目录'), '甲范围为项目默认');
await addProject(B, '知识库乙', '知识库');
await addProject(C, '项目丙', '工作');
await b.goto('#/projects');
await b.waitFor('!!document.querySelector("[data-list]")');
await shot('03-list-a-b-c');
const listText = await b.evaluate(`document.querySelector('[data-list]').textContent`);
expect(listText.includes('知识库甲') && listText.includes('知识库乙') && listText.includes('项目丙'), '三个项目均列出');
expect((listText.match(/文件夹/g) || []).length === 2, '甲乙显示文件夹徽标');
await b.evaluate(`document.querySelector('[data-search]').value='知识库'; document.querySelector('[data-search]').dispatchEvent(new Event('input'))`);
const searched = await b.evaluate(`document.querySelector('[data-list]').textContent`);
expect(searched.includes('知识库甲') && searched.includes('知识库乙') && !searched.includes('项目丙'), '搜索「知识库」命中甲乙');
await b.evaluate(`document.querySelector('[data-search]').value=''; document.querySelector('[data-search]').dispatchEvent(new Event('input')); document.querySelector('[data-kind]').value='nongit'; document.querySelector('[data-kind]').dispatchEvent(new Event('change'))`);
const nongitOnly = await b.evaluate(`document.querySelector('[data-list]').textContent`);
expect(nongitOnly.includes('知识库甲') && !nongitOnly.includes('项目丙'), '类型过滤=文件夹 仅甲乙');

// ---- 3. 甲为主对象：宿主选择 + 保存（UI 与 API 同状态）----
const nongitId = sh(`ls ${S}/data/repos | grep nongit | head -1`).trim();
await b.goto('#/projects/' + encodeURIComponent(nongitId));
await b.waitFor('!!document.querySelector("[data-entries]")');
await b.evaluate(`(async()=>{
  const row=[...document.querySelectorAll('[data-entries] .project-row')].find(r=>r.querySelector('.row-head strong')?.textContent==='Claude Code');
  row.querySelector('select').value='enable';
  [...row.querySelectorAll('button')].find(x=>x.textContent==='保存').click();
})()`);
await b.waitFor(`document.querySelector('[data-message]')?.textContent.includes('已保存')`);
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('已启用')`);
await shot('04-a-host-enable');
const eff = api(`/api/effective?root=${encodeURIComponent(A)}`);
expect(eff.hosts?.claude?.enabled === true, 'API effective：claude 启用（与 UI 同状态）');
expect(eff.is_nongit === true, `API effective：is_nongit=true（repo_id=${eff.repo_id}）`);

// ---- 4. 指令（产生真实部署物）+ plan / apply 接受普通目录；写磁盘 ----
await b.evaluate(`document.querySelector('[data-tab="4"]').click()`);
await b.waitFor(`!!document.querySelector('[data-save]') && !document.querySelector('[data-save]').disabled`);
await b.evaluate(`document.querySelector('textarea').value='甲知识库的个人补充：回答用中文。'; document.querySelector('textarea').dispatchEvent(new Event('input')); document.querySelector('[data-save]').click()`);
await b.waitFor(`document.querySelector('[data-msg]')?.textContent.includes('已保存')`);
await shot('05b-a-instructions-saved');
await b.evaluate(`document.querySelector('[data-tab="5"]').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='生成预览')");
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='生成预览').click()`);
await b.waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)");
await shot('05-a-plan');
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='应用' && !b.disabled).click()`);
await b.waitFor("[...document.querySelectorAll('dialog:modal .dialog-actions button')].some(x=>x.textContent==='确认应用')");
await b.evaluate(`[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(x=>x.textContent==='确认应用').click()`);
await new Promise(r => setTimeout(r, 1500));
await shot('06-a-apply-after');
const applyView = await b.evaluate(`document.querySelector('[data-content]').textContent`);
const applyIdx = applyView.indexOf('应用：');
log('应用后面板文本（F07 反例：停留进度=缺陷）:', JSON.stringify(applyView.slice(applyIdx, applyIdx + 100)));
const disk = sh(`find "${A}" \\( -type f -o -type l \\) | sort`).split('\n').filter(l => l && !l.includes('/skills/'));
log('甲目录磁盘变化:', JSON.stringify(disk, null, 1));
expect(disk.some(l => l.includes('.claude') || l.includes('AGENTS.override')), 'apply 在甲（普通目录）真实写盘');
for (const f of disk.filter(l => l.trim() && l.includes('.claude'))) log('写盘内容预览:', sh(`head -c 200 "${f}"`).replace(/\n/g, ' | '));
const st = api(`/api/deploy-status?root=${encodeURIComponent(A)}`);
log('deploy-status:', JSON.stringify(st.items));

// ---- 5. 知识库内容不受影响（哈希不变、无 .git）----
expect(kbHashes() === sh(`cat ${EV}/baseline-kb-hashes.txt`), '甲乙知识文档与 Skill 文件哈希完全不变');
expect(!sh(`/bin/ls -a "${A}"`).includes('.git'), '甲无 .git（未 init）');

// ---- 6. 乙 / 丙隔离 ----
expect(!api(`/api/effective?root=${encodeURIComponent(B)}`).hosts?.claude?.enabled, '乙未启用任何宿主（甲的操作不外溢）');
expect(!api(`/api/effective?root=${encodeURIComponent(C)}`).hosts?.claude?.enabled, '丙（Git）未受影响');

// ---- 7. 重启持久化（真实重启：token shutdown → 重新拉起 → 浏览器刷新）----
const token = sh(`grep "/?token=" ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
sh(`curl -s -X POST http://127.0.0.1:${PORT}/api/shutdown -H "X-AILoom-Session: ${token}" -d '{}'`);
await new Promise(r => setTimeout(r, 1500));
execSync(`cd ${S} && HOME=${S}/home XDG_STATE_HOME=${S}/xdg-state XDG_DATA_HOME=${S}/xdg-data GIT_CONFIG_GLOBAL=${S}/home/.gitconfig GIT_CONFIG_NOSYSTEM=1 nohup <repo>/target/debug/ailoom --data-root ${S}/data console --port ${PORT} --no-open >> ${EV}/console.log 2>&1 &`, { shell: '/bin/zsh' });
waitApi();
// approved_roots 是服务内存态，重启即清空；与 UI 渲染先 approveDir 保持同一前提（写接口需新 token）。
const token2 = sh(`grep "/?token=" ${EV}/console.log | tail -1 | sed -E 's/.*token=([a-f0-9-]+).*/\\1/'`).trim();
sh(`curl -s -X POST http://127.0.0.1:${PORT}/api/fs/approve -H "X-AILoom-Session: ${token2}" -H 'Content-Type: application/json' -d '{"path":${JSON.stringify(A)}}'`);
expect(api('/api/state').repos.length === 3, '重启后 3 个项目仍在注册表');
expect(api(`/api/effective?root=${encodeURIComponent(A)}`).hosts?.claude?.enabled === true, '重启后甲的 claude 启用状态持久化');
await b.evaluate(`location.reload()`);
await b.waitFor('!!document.querySelector("#nav")', 20000);
await b.goto('#/projects/' + encodeURIComponent(nongitId));
await b.waitFor(`document.querySelector('[data-entries]')?.textContent.includes('已启用')`, 20000);
await shot('07-a-restart-persist');

// ---- 8. 失联恢复（修复前反例记录）----
sh(`mv "${A}" "${A}-失联测试"`);
await b.evaluate(`location.reload()`);
await b.waitFor('!!document.querySelector("#nav")', 20000);
await b.goto('#/projects/' + encodeURIComponent(nongitId));
await new Promise(r => setTimeout(r, 1200));
await shot('08-a-missing-dir-before-fix');
const missingText = await b.evaluate(`document.querySelector('#app').innerText`);
log('失联时页面文本（前 500 字）:', missingText.slice(0, 500).replace(/\n/g, ' | '));
const stateBadge = await b.evaluate(`document.querySelector('.config-header')?.textContent ?? ''`);
log('失联时范围徽标:', JSON.stringify(stateBadge.slice(0, 120)));
sh(`mv "${A}-失联测试" "${A}"`);

console.log(JSON.stringify({ failures }, null, 2));
await b.close();
if (failures.length) process.exitCode = 1;
