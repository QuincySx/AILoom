// AIL-108 补齐：#/sources 更新中心 + 个人副本编辑器（打开/保存/指纹冲突/删除归档）
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const b = await connect('http://127.0.0.1:8642/');
await b.waitFor('!!document.querySelector("#nav")');

// ============ A. 更新中心（独立页面，updatesOnly） ============
await b.goto('#/sources');
await b.waitFor(`!!document.querySelector('[data-check]')`);
await b.evaluate("document.querySelector('[data-check]').click()");
await b.waitFor(`document.querySelector('[data-msg]').textContent.includes('检查完成')`);
const srcMsg = await b.evaluate(`document.querySelector('[data-msg]').textContent`);
console.log('更新中心检查结果:', srcMsg);
const srcBadges = await b.evaluate('[...document.querySelectorAll(".repository-registration .repository-meta .badge")].map(x=>x.textContent)');
console.log('更新中心来源徽标:', JSON.stringify(srcBadges));
if (!srcMsg.includes('检查完成')) throw new Error('更新中心检查应完成');
await b.screenshot('/tmp/ailoom-flow/evidence/ail108b-sources.png');
log1: {
  // 无可用更新时「更新全部可用版本」应禁用
  const dis = await b.evaluate(`document.querySelector('[data-update]').disabled`);
  console.log('无可更新时批量更新禁用:', dis);
  if (!dis) throw new Error('无可更新时批量更新应禁用');
}

// ============ B. 个人副本编辑器 ============
// 确保有一个个人副本（local-fixture-skill 已存在则跳过导入）
let entries = JSON.parse(execSync(`curl -s http://127.0.0.1:8642/api/library/list`).toString()).entries;
if (!entries.length) throw new Error('前置：需要至少一个个人副本（local-fixture-skill）');
const target = entries[0];
console.log('个人副本:', target.id);

await b.goto('#/library');
await b.waitFor(`!!document.querySelector('[data-table] table')`);
await b.waitFor(`document.querySelector('[data-table]').innerText.includes('personal/skill')`);
// 点击行 → 编辑器打开
await b.evaluate(`(()=>{ const rows=[...document.querySelectorAll('[data-table] tbody tr')]; rows.find(r=>r.innerText.includes('personal/skill')).click(); })()`);
await b.waitFor(`!document.querySelector('[data-editor]').classList.contains('hidden')`);
await b.waitFor(`!!document.querySelector('[data-box] textarea, [data-box] .CodeMirror, [data-editor] textarea')`);
const opened = await b.evaluate(`document.querySelector('[data-editor] [data-id]')?.textContent`);
console.log('编辑器打开:', opened);
await b.screenshot('/tmp/ailoom-flow/evidence/ail108b-copy-editor.png');

// 修改并保存（指纹保护路径）
await b.evaluate(`(()=>{ const ta=document.querySelector('[data-box] textarea'); ta.value += '\\n<!-- AIL-108b 编辑验收 -->'; ta.dispatchEvent(new Event('input')); })()`);
await b.evaluate("document.querySelector('[data-save]').click()");
await b.waitFor(`document.querySelector('[data-edit-msg]').textContent.includes('已保存')`);
console.log('保存反馈:', await b.evaluate(`document.querySelector('[data-edit-msg]').textContent`));

// 外部改动磁盘 → 再保存 → 冲突提示
const resPath = execSync(`find /tmp/ailoom-flow/data/library -name "SKILL.md" -path "*local-fixture*" | head -1`).toString().trim();
if (!resPath) throw new Error('未找到个人副本磁盘文件');
execSync(`printf '\\n<!-- 外部修改 -->\\n' >> ${JSON.stringify(resPath)}`);
await b.evaluate(`(()=>{ const ta=document.querySelector('[data-box] textarea'); ta.value += '\\n<!-- 冲突保存 -->'; ta.dispatchEvent(new Event('input')); })()`);
await b.evaluate("document.querySelector('[data-save]').click()");
await b.waitFor(`document.querySelector('[data-edit-msg]').textContent.includes('已被外部修改') || document.querySelector('[data-edit-msg]').textContent.includes('校验未通过')`);
console.log('冲突反馈:', await b.evaluate(`document.querySelector('[data-edit-msg]').textContent`));
if (!(await b.evaluate(`document.querySelector('[data-edit-msg]').textContent`)).includes('已被外部修改')) throw new Error('应提示外部修改冲突');
await b.screenshot('/tmp/ailoom-flow/evidence/ail108b-copy-conflict.png');

// 删除个人副本：确认弹窗 → 移入归档
const libFiles = execSync(`find /tmp/ailoom-flow/data/library -type f | wc -l`).toString().trim();
await b.evaluate("document.querySelector('[data-delete]').click()");
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
console.log('删除确认:', (await b.evaluate("document.querySelector('dialog[open] .confirmation-message').textContent")).slice(0, 120));
await b.screenshot('/tmp/ailoom-flow/evidence/ail108b-copy-delete-confirm.png');
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='移入归档').click()`);
await b.waitFor(`document.querySelector('#toast')?.textContent.includes('归档') || true`);
await new Promise(r=>setTimeout(r,600));
const libFilesAfter = execSync(`find /tmp/ailoom-flow/data/library -type f | wc -l`).toString().trim();
const archived = execSync(`find /tmp/ailoom-flow/data/library-archive -type f 2>/dev/null | wc -l`).toString().trim();
console.log('library 文件数:', libFiles, '→', libFilesAfter, '| 归档文件数:', archived);
if (Number(libFilesAfter) >= Number(libFiles)) throw new Error('删除后 library 应减少');
if (Number(archived) < 1) throw new Error('副本应移入 library-archive');

console.log('ERRORS:', JSON.stringify(b.errors));
if (b.errors.length) throw new Error('页面存在未处理异常');
await b.close();
console.log('SOURCES + PERSONAL COPY EDITOR PASSED');
