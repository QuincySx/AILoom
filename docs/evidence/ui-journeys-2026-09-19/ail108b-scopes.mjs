// AIL-108 补齐：#/scopes 仓库与作用域页一对一功能驱动
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const profile = () => execSync('cat /tmp/ailoom-flow/data/profile/profile.toml').toString();
const b = await connect('http://127.0.0.1:8642/');
await b.waitFor('!!document.querySelector("#nav")');

await b.goto('#/scopes');
await b.waitFor(`!!document.querySelector('[data-picker]')`);
await new Promise(r=>setTimeout(r,500));
console.log('页面文案:', (await b.evaluate(`document.querySelector('#app').innerText`)).slice(0, 200).replace(/\n/g,' | '));

// 1) ScopePicker 是 DataTable，点击 a2 所在行选择目标
await b.evaluate(`(()=>{
  const rows=[...document.querySelectorAll('[data-picker] tbody tr')];
  const target=rows.find(r=>r.innerText.includes('projects/a2'));
  if(!target) throw new Error('a2 行不存在');
  target.click();
})()`);
await b.waitFor(`document.querySelector('[data-eff] table')`);
await new Promise(r=>setTimeout(r,600));
console.log('通知/目标:', await b.evaluate(`[...document.querySelectorAll('#app p, #app span')].map(x=>x.textContent).find(t=>t.includes('操作目标')) || '(未见通知)'`));

// 2) 有效配置表渲染
await b.waitFor(`!!document.querySelector('[data-eff] table')`);
console.log('有效配置表:', (await b.evaluate(`document.querySelector('[data-eff]').innerText`)).replace(/\n/g,' | ').slice(0, 260));

// 3) 写入选择：deploy-helper enable（仓库默认层）
await b.evaluate(`(()=>{
  const res = document.querySelector('[aria-label="选择引用的 Skill 或 MCP"]');
  res.value = [...res.options].find(o=>o.value.includes('/skill/common/deploy-helper')).value;
})()`);
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='写入选择（仅配置，不写仓库）').click()`);
await b.waitFor(`[...document.querySelectorAll('#app span')].some(x=>x.textContent.includes('已写入 仓库默认'))`);
console.log('写入反馈:', await b.evaluate(`[...document.querySelectorAll('#app span')].find(x=>x.textContent.includes('已写入')).textContent`));
if (!profile().includes('deploy-helper')) throw new Error('profile 应含 deploy-helper 引用');
console.log('profile 写入核对 OK');

// 4) 仓库默认变更影响预览（无写入）
const before = execSync(`find /tmp/ailoom-flow/projects -newer /tmp/ailoom-flow/data -type f 2>/dev/null | wc -l`).toString().trim();
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent.includes('预览各工作树影响')).click()`);
await b.waitFor(`!!document.querySelector('[data-preview-result] table')`);
console.log('影响预览:', (await b.evaluate(`document.querySelector('[data-preview-result]').innerText`)).replace(/\n/g,' | ').slice(0,220));

// 5) 本页的 生成预览 → 应用（真实写文件到所选工作树）
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='生成预览').click()`);
await b.waitFor(`document.querySelector('[data-plan]').innerText.includes('仓库')`);
await b.evaluate(`[...document.querySelectorAll('[data-apply] button')].find(b=>b.textContent==='应用' && !b.disabled).click()`);
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='确认应用').click()`);
await b.waitFor(`document.querySelector('[data-apply]').innerText.includes('已写入') || document.querySelector('[data-apply]').innerText.includes('失败')`, 30000);
const skillLink = execSync(`test -e /tmp/ailoom-flow/projects/a2/.claude/skills/deploy-helper && echo yes || echo no`).toString().trim();
console.log('scopes 页应用后产物:', skillLink);
if (skillLink !== 'yes') throw new Error('scopes 页应用应写入 deploy-helper');
await b.screenshot('/tmp/ailoom-flow/evidence/ail108b-scopes-applied.png');

// 6) 恢复：清除引用 + 再应用，保持沙盒状态干净
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='写入选择（仅配置，不写仓库）')`);
await b.evaluate(`(()=>{
  const state = [...document.querySelectorAll('#app select')].find(s=>[...s.options].some(o=>o.value==='inherit'));
  state.value='inherit';
})()`);
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='写入选择（仅配置，不写仓库）').click()`);
await b.waitFor(`[...document.querySelectorAll('#app span')].some(x=>x.textContent.includes('已写入'))`);
await b.evaluate(`[...document.querySelectorAll('#app button')].find(b=>b.textContent==='生成预览').click()`);
await b.waitFor(`document.querySelector('[data-plan]').innerText.includes('仓库')`);
await b.evaluate(`[...document.querySelectorAll('[data-apply] button')].find(b=>b.textContent==='应用' && !b.disabled).click()`);
await b.waitFor("!!document.querySelector('dialog[open] .confirmation-message')");
await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent==='确认应用').click()`);
await b.waitFor(`document.querySelector('[data-apply]').innerText.includes('已写入') || document.querySelector('[data-apply]').innerText.includes('失败')`, 30000);
const cleaned = execSync(`test -e /tmp/ailoom-flow/projects/a2/.claude/skills/deploy-helper && echo yes || echo no`).toString().trim();
console.log('恢复继承并再应用后产物清理:', cleaned);
if (cleaned !== 'no') throw new Error('inherit+apply 后应清理产物');
console.log('ERRORS:', JSON.stringify(b.errors));
if (b.errors.length) throw new Error('页面存在未处理异常');
await b.close();
console.log('SCOPES PAGE PASSED');
