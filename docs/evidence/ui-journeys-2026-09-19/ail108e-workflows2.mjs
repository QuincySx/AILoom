// AIL-108 补齐（续）：流程产物 v1→v2、dirty 保护、复核、导出+备份
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const b = await connect('http://127.0.0.1:8642/');
await b.waitFor('!!document.querySelector("#nav")');
await b.goto('#/workflows');
await b.waitFor(`document.querySelector('[data-sel]').options.length > 0`);
await b.evaluate(`(()=>{ const sel=document.querySelector('[data-sel]'); sel.value=[...sel.options].at(-1).value; [...document.querySelectorAll('#app button')].find(x=>x.textContent==='打开').click(); })()`);
await b.waitFor(`!!document.querySelector('[data-do="put"]')`);
await b.waitFor(`document.querySelector('[data-wf]').innerText.includes('bound')`);
await new Promise(r=>setTimeout(r,500));

// 记录输入版本
await b.evaluate(`(()=>{ const i=document.querySelector('[data-input-res]'); i.value='personal/skill/personal/local-fixture-skill'; i.dispatchEvent(new Event('input')); })()`);
await b.evaluate(`[...document.querySelectorAll('[data-do]')].find(x=>x.textContent==='记录输入').click()`);
await b.waitFor(`document.querySelector('[data-wf]').innerText.includes('输入版本')`, 15000);
console.log('输入版本已记录');

// 新增产物 v1
await b.evaluate(`(()=>{ const s=document.querySelector('[data-stage]'); s.value='spec'; const t=document.querySelector('[data-title]'); t.value='验收规格'; t.dispatchEvent(new Event('input')); const c=document.querySelector('[data-content]'); c.value='# 验收规格\\n\\n- 第一版正文'; c.dispatchEvent(new Event('input')); })()`);
await b.evaluate(`document.querySelector('[data-do="put"]').click()`);
await b.waitFor(`document.querySelector('[data-wf]').innerText.includes('验收规格')`, 15000);
await new Promise(r=>setTimeout(r,800));
console.log('产物已写入（v1）');

// 编辑 → v2
await b.evaluate(`[...document.querySelectorAll('[data-do="edit"]')][0].click()`);
await b.waitFor(`document.querySelector('[data-content]').value.includes('第一版正文')`);
await b.evaluate(`(()=>{ const c=document.querySelector('[data-content]'); c.value='# 验收规格\\n\\n- 第二版正文（修订）'; c.dispatchEvent(new Event('input')); })()`);
await b.evaluate(`document.querySelector('[data-do="put"]').click()`);
await b.waitFor(`document.querySelector('[data-wf]').innerText.includes('v2')`, 15000);
await new Promise(r=>setTimeout(r,600));
console.log('产物编辑后 v2 可见');

// dirty 保护
await b.evaluate(`(()=>{ const c=document.querySelector('[data-content]'); c.value='未保存草稿'; c.dispatchEvent(new Event('input')); })()`);
await b.evaluate(`[...document.querySelectorAll('[data-do]')].find(x=>x.textContent==='标记已复核').click()`);
await b.waitFor(`!!document.querySelector('dialog[open] .confirmation-message')`);
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='取消').click()`);
await b.waitFor("!document.querySelector('dialog[open]')");
if (!(await b.evaluate(`document.querySelector('[data-content]').value`)).includes('未保存草稿')) throw new Error('取消后草稿应保留');
console.log('dirty 确认 + 取消保留 OK');

// 复核（放弃草稿继续）
await b.evaluate(`[...document.querySelectorAll('[data-do]')].find(x=>x.textContent==='标记已复核').click()`);
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='继续').click()`);
await b.waitFor(`document.querySelector('[data-wf]').innerText.includes('已复核')`, 15000);
console.log('已复核标记完成');

// 导出（prompt 注入目标）→ 确认写入 → 文件核对
const exportTarget = '/tmp/ailoom-flow/projects/onboard/EXPORT-SPEC.md';
await b.evaluate(`window.prompt = () => ${JSON.stringify(exportTarget)}`);
await b.evaluate(`[...document.querySelectorAll('[data-do="export"]')][0].click()`);
await b.waitFor(`!!document.querySelector('dialog[open] .confirmation-message')`);
console.log('导出确认:', (await b.evaluate("document.querySelector('dialog[open] .confirmation-message').textContent")).slice(0, 60));
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='确认写入').click()`);
await b.waitFor(`document.querySelector('#toast')?.textContent.includes('已导出')`, 15000);
if (!execSync(`cat ${exportTarget} 2>/dev/null`).toString().includes('第二版正文')) throw new Error('导出应是 v2 内容');
console.log('导出文件内容 = v2 OK');

// 再次导出 → 备份
await b.evaluate(`window.prompt = () => ${JSON.stringify(exportTarget)}`);
await b.evaluate(`[...document.querySelectorAll('[data-do="export"]')][0].click()`);
await b.waitFor(`!!document.querySelector('dialog[open] .confirmation-message')`);
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='确认写入').click()`);
await b.waitFor(`document.querySelector('#toast')?.textContent.includes('已导出')`, 15000);
const backups = execSync(`ls /tmp/ailoom-flow/projects/onboard/ | grep EXPORT`).toString().trim().split('\n');
console.log('导出相关文件:', backups);
if (backups.length < 2) throw new Error('覆盖导出应生成备份');
await b.screenshot('/tmp/ailoom-flow/evidence/ail108b-workflows.png');

console.log('ERRORS:', JSON.stringify(b.errors));
if (b.errors.length) throw new Error('页面存在未处理异常');
await b.close();
console.log('WORKFLOWS DOC FLOW PASSED');
