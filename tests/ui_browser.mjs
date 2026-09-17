// Local browser acceptance; needs an independently launched Chrome on :9231.
import { writeFile } from 'node:fs/promises';
const [url, output, mode = 'current'] = process.argv.slice(2);
const tab = await (await fetch('http://127.0.0.1:9231/json/new?' + encodeURIComponent(url), { method: 'PUT' })).json();
const socket = new WebSocket(tab.webSocketDebuggerUrl);
await new Promise(resolve => socket.addEventListener('open', resolve, { once: true }));
let seq = 0;
const pending = new Map(), errors = [];
socket.addEventListener('message', event => {
  const m = JSON.parse(event.data);
  if (m.id) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.reject(m.error) : p.resolve(m.result); }
  if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.text + ': ' + JSON.stringify(m.params.exceptionDetails.exception));
});
function send(method, params = {}) {
  return new Promise((resolve, reject) => { const id = ++seq; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })); });
}
async function evaluate(expression) {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
async function waitFor(expression) {
  for (let n = 0; n < 100; n++) {
    if (await evaluate(expression)) return;
    await new Promise(r => setTimeout(r, 100));
  }
  await screenshot(output + '-failed.png');
  throw new Error('Timed out: ' + expression + '\n' + await evaluate('document.querySelector("#app").innerText'));
}
async function screenshot(path) {
  const shot = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
  await writeFile(path, Buffer.from(shot.data, 'base64'));
}
await send('Runtime.enable');
await send('Page.enable');
await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
await waitFor('!!document.querySelector("#app")?.textContent.trim()');
await screenshot(output + '-desktop.png');
if (mode === 'cc-switch') {
  await evaluate("location.hash='#/library'");
  await waitFor("!!document.querySelector('[data-cc-switch]')");
  await evaluate("document.querySelector('[data-cc-switch]').focus(); document.querySelector('[data-cc-switch]').click()");
  await waitFor("document.querySelector('[data-cc-json]')?.closest('dialog')?.matches(':modal')");
  if (!await evaluate("getComputedStyle(document.querySelector('[data-cc-apply]')).display === 'none' && getComputedStyle(document.querySelector('[data-cc-prepare]')).display === 'none'")) throw new Error('Migration actions must stay hidden before scan/preview');
  const ccDirectory = process.argv[5];
  if (ccDirectory) {
    // Only pass the explicitly created synthetic fixture directory, never a user database.
    await evaluate(`document.querySelector('[data-cc-directory]').value=${JSON.stringify(ccDirectory)}; document.querySelector('[data-cc-read]').click()`);
    await waitFor("document.querySelector('[data-cc-status]').textContent.includes('发现 2 项') && !document.querySelector('[data-cc-read]').disabled");
    if (!await evaluate("document.querySelector('[data-cc-items]').textContent.includes('plugins/fixture-skill') && document.querySelectorAll('[data-cc-select]:checked').length===1")) throw new Error('One-click source database scan failed');
    await screenshot(output + '-cc-switch-direct.png');
  }
  await evaluate("document.querySelector('[data-cc-fallback]').open=true");
  await evaluate("document.querySelector('[data-cc-json]').value='invalid'; document.querySelector('[data-cc-scan]').click()");
  await waitFor("document.querySelector('[data-cc-status]').textContent.includes('不是有效的 JSON')");
  const fixture = [{name:'fixture-skill',directory:'fixture-skill',repo_owner:'fixture-owner',repo_name:'fixture-repo',repo_branch:'main',readme_url:'https://github.com/fixture-owner/fixture-repo/blob/main/plugins/fixture-skill/SKILL.md'},{name:'没有来源'},{name:'<script>fixture</script>',repo_url:'file:///tmp/forbidden'}];
  await evaluate(`document.querySelector('[data-cc-json]').value=${JSON.stringify(JSON.stringify(fixture))}; document.querySelector('[data-cc-json]').dispatchEvent(new Event('input')); document.querySelector('[data-cc-scan]').click()`);
  await waitFor("document.querySelector('[data-cc-status]').textContent.includes('发现 3 项') && !document.querySelector('[data-cc-scan]').disabled");
  if (!await evaluate("document.querySelectorAll('[data-cc-select]:checked').length===1 && document.querySelectorAll('[data-cc-select]:disabled').length===2 && document.querySelector('[data-cc-items]').textContent.includes('plugins/fixture-skill') && !document.querySelector('[data-cc-items] script')")) throw new Error('Migration scan selection/path/escaping failed');
  await screenshot(output + '-cc-switch-desktop.png');
  await evaluate("document.querySelector('[data-cc-none]').click()");
  if (!await evaluate("document.querySelector('[data-cc-prepare]').disabled")) throw new Error('Empty migration selection must be disabled');
  await evaluate("document.querySelector('[data-cc-all]').click()");
  if (!await evaluate("!document.querySelector('[data-cc-prepare]').disabled && document.querySelectorAll('[data-cc-select]:checked').length===1")) throw new Error('Invalid sources must stay unselected');
  await send('Emulation.setDeviceMetricsOverride', {width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output + '-cc-switch-mobile.png');
  if (await evaluate("document.documentElement.scrollWidth > innerWidth + 1 || document.querySelector('[data-cc-json]').closest('dialog').scrollWidth > document.querySelector('[data-cc-json]').closest('dialog').clientWidth + 1")) throw new Error('Migration dialog overflow');
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await waitFor("!document.querySelector('dialog:modal')");
  if (!await evaluate("document.activeElement===document.querySelector('[data-cc-switch]')")) throw new Error('Migration focus not restored');
}
if (mode === 'projects' || mode === 'design') {
  const [project] = process.argv.slice(5);
  await waitFor("location.hash === '#/projects' && !!document.querySelector('[data-new]')");
  if (mode === 'design') {
    await waitFor("!!document.querySelector('[data-filter]')?.parentElement.querySelector('[role=combobox]')");
    const fields = await evaluate("[document.querySelector('[data-search]'), document.querySelector('[data-filter]').parentElement.querySelector('[role=combobox]'), document.querySelector('[data-kind]').parentElement.querySelector('[role=combobox]')].map(e=>{const s=getComputedStyle(e);return [e.getBoundingClientRect().height,s.borderRadius,s.borderColor,s.backgroundColor,s.paddingLeft,s.fontSize,s.appearance]})");
    if (fields.some(f => JSON.stringify(f)!==JSON.stringify(fields[0]))) throw new Error('Search and select styles differ: '+JSON.stringify(fields));
    await screenshot(output + '-toolbar.png');
  }
  await evaluate("document.querySelector('[data-new]').focus(); document.querySelector('[data-new]').click()");
  await waitFor("document.querySelector('[data-create]')?.closest('dialog')?.matches(':modal')");
  await screenshot(output + '-new-project-dialog.png');
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await waitFor("!document.querySelector('dialog:modal')");
  if (!await evaluate("document.activeElement === document.querySelector('[data-new]')")) throw new Error('New-project focus not restored');
  await evaluate("document.querySelector('[data-new]').click()");
  await evaluate(`document.querySelector('[data-path]').value=${JSON.stringify(project)}; document.querySelector('[data-name]').value='浏览器验收项目'; document.querySelector('[data-category]').value='验收'; document.querySelector('[data-create]').requestSubmit()`);
  await waitFor("!!document.querySelector('[data-tab]') && document.querySelector('[data-content]').textContent.includes('claude')");
  if (mode === 'design') {
    await evaluate("document.querySelector('[data-settings]').click()");
    await waitFor("document.querySelector('[data-meta]').closest('dialog').matches(':modal')");
    await screenshot(output + '-settings-dialog.png');
    await evaluate("document.querySelector('[data-meta-cancel]').click()");
    await waitFor("!document.querySelector('dialog:modal')");
  }
  await evaluate("document.querySelector('[data-entries] select').value='enable'; [...document.querySelectorAll('[data-entries] button')].find(b=>b.textContent==='保存').click()");
  await waitFor("document.querySelector('[data-message]')?.textContent.includes('已保存') && document.querySelector('[data-entries]')?.textContent.includes('当前有效：启用')");
  await evaluate("document.querySelector('[data-tab=\"4\"]').click()");
  await waitFor("!!document.querySelector('[data-save]') && !document.querySelector('[data-save]').disabled");
  await evaluate("document.querySelector('textarea').value='仅当前项目的验收指令'; document.querySelector('textarea').dispatchEvent(new Event('input')); document.querySelector('[data-save]').click()");
  await waitFor("document.querySelector('[data-msg]').textContent.includes('已保存')");
  await evaluate("document.querySelector('[data-tab=\"0\"]').click()");
  await waitFor("!!document.querySelector('[data-entries]')");
  await evaluate("document.querySelector('[data-tab=\"4\"]').click()");
  await waitFor("document.querySelector('textarea')?.value === '仅当前项目的验收指令'");
  await screenshot(output + '-instructions.png');
  await evaluate("location.hash='#/projects'");
  await waitFor("!!document.querySelector('[data-search]')");
  await evaluate("document.querySelector('[data-search]').value='不存在的项目'; document.querySelector('[data-search]').dispatchEvent(new Event('input'))");
  await waitFor("document.querySelector('[data-list]').textContent.includes('没有匹配')");
  await evaluate("document.querySelector('[data-search]').value=''; document.querySelector('[data-search]').dispatchEvent(new Event('input')); document.querySelector('[data-filter]').value='验收'; document.querySelector('[data-filter]').dispatchEvent(new Event('change'))");
  await waitFor("document.querySelector('[data-list]').textContent.includes('浏览器验收项目')");
  await screenshot(output + '-projects.png');
  await send('Emulation.setDeviceMetricsOverride', { width:390,height:844,deviceScaleFactor:1,mobile:false });
  await screenshot(output + '-projects-mobile.png');
  if (await evaluate('document.documentElement.scrollWidth > innerWidth + 1')) throw new Error('Project list overflow');
  await evaluate("document.querySelector('[data-list] a').click()");
  await waitFor("!!document.querySelector('[data-entries]')");
  await screenshot(output + '-detail-mobile.png');
  if (await evaluate('document.documentElement.scrollWidth > innerWidth + 1')) throw new Error('Project detail overflow');
}
if (mode === 'onboarding') {
  const [project, skill] = process.argv.slice(5);
  const click = async text => evaluate(`[...document.querySelectorAll('#app button')].find(b => b.textContent === ${JSON.stringify(text)}).click()`);
  await evaluate("location.hash='#/onboarding'");
  await waitFor("!!document.querySelector('.wizard-steps')");
  await click('1 选择项目');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='批准此目录')");
  await evaluate(`document.querySelector('#app input').value=${JSON.stringify(project)}`);
  await click('批准此目录');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent === '识别当前目录')");
  await click('识别当前目录');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent === '确认项目，下一步')");
  await click('确认项目，下一步');
  await waitFor('!!document.querySelector("[data-h=claude]")');
  await evaluate("document.querySelector('[data-h=claude]').checked=true; document.querySelector('[data-inline-import]').parentElement.open=true");
  await waitFor("!document.querySelector('[data-resource-options]').textContent.includes('正在读取')");
  if (!await evaluate("!!document.querySelector('[data-resource-options] input')")) {
  await evaluate("document.querySelector('[data-add]').click(); document.querySelector('[data-provider]').value='local'; document.querySelector('[data-provider]').dispatchEvent(new Event('change'))");
  await evaluate(`document.querySelector('[data-url]').value=${JSON.stringify(skill)}; document.querySelector('[data-form]').requestSubmit()`);
  await waitFor("!!document.querySelector('[data-confirm]') && !document.querySelector('[data-confirm]').disabled");
  await click('确认导入');
  await waitFor("!!document.querySelector('[data-resource-options] input')");
  }
  await evaluate("const selectedResource = document.querySelector('[data-resource-options] input'); selectedResource.checked=true; selectedResource.dispatchEvent(new Event('change'))");
  await click('保存选择，下一步预览');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='生成预览')");
  await click('生成预览');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)");
  await click('应用');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='完成设置，进入资源库')");
  await screenshot(output + '-applied.png');
  await click('完成设置，进入资源库');
  await waitFor("location.hash === '#/library'");
}
if (mode !== 'before') {
  await evaluate('location.hash = "#/library"');
  await waitFor('!!document.querySelector("[data-add]")');
  await evaluate('document.querySelector("[data-add]").click()');
  await waitFor('document.querySelector("[data-import]")?.closest("dialog")?.matches(":modal")');
  if(mode === 'design') {
    await waitFor("!!document.querySelector('[data-provider]').parentElement.querySelector('[role=combobox]')");
    await evaluate("document.querySelector('[data-provider]').parentElement.querySelector('[role=combobox]').click()");
    await waitFor("!!document.querySelector('.select-menu:popover-open [role=option]')");
    await screenshot(output + '-html-options.png');
    await evaluate("[...document.querySelectorAll('.select-menu:popover-open [role=option]')].find(e=>e.textContent.includes('GitLab')).click()");
    await waitFor("document.querySelector('[data-provider]').value === 'gitlab' && !document.querySelector('.select-menu:popover-open')");
    await send('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowDown',code:'ArrowDown',windowsVirtualKeyCode:40});
    await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
    if(!await evaluate("!!document.querySelector('dialog:modal') && document.querySelector('[data-provider]').value === 'gitlab'")) throw new Error('Escape must close options, not dialog or change selection');
    await send('Input.dispatchKeyEvent',{type:'keyDown',key:'ArrowDown',code:'ArrowDown',windowsVirtualKeyCode:40});
    await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Home',code:'Home',windowsVirtualKeyCode:36});
    await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13,text:'\r'});
    await waitFor("document.querySelector('[data-provider]').value === 'github'");
    if(await evaluate("[...document.querySelectorAll('#app select')].some(s=>getComputedStyle(s).display !== 'none')")) throw new Error('Native select still visible');
  }
  const providers = await evaluate('[...document.querySelector("[data-provider]").options].map(o=>o.value)');
  if (providers.join(',') !== 'github,gitlab,git,local,entry') throw new Error('Missing source choices');
  for (const p of providers) {
    await evaluate(`document.querySelector('[data-provider]').value=${JSON.stringify(p)}; document.querySelector('[data-provider]').dispatchEvent(new Event('change'))`);
  }
  await evaluate("document.querySelector('[data-provider]').value='gitlab'; document.querySelector('[data-provider]').dispatchEvent(new Event('change'))");
  await screenshot(output + '-import.png');
  await send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: false });
  await screenshot(output + '-mobile.png');
  const overflow = await evaluate('({width:innerWidth, scroll:document.documentElement.scrollWidth})');
  if (overflow.scroll > overflow.width + 1) throw new Error('Horizontal overflow: ' + JSON.stringify(overflow));
  if (mode === 'design' && process.argv[6]) {
    await evaluate("document.querySelector('[data-provider]').value='local'; document.querySelector('[data-provider]').dispatchEvent(new Event('change'))");
    await evaluate(`document.querySelector('[data-url]').value=${JSON.stringify(process.argv[6])}; document.querySelector('[data-form]').requestSubmit()`);
    await waitFor("!!document.querySelector('[data-confirm]') && !document.querySelector('[data-confirm]').disabled");
    await evaluate("document.querySelector('[data-confirm]').click()");
    await waitFor("!document.querySelector('dialog:modal') && document.querySelector('[data-msg]').textContent.includes('已加入资源库')");
  }
}
if (mode === 'design') {
  await evaluate("location.hash='#/samples'");
  await waitFor("document.querySelector('#app')?.textContent.includes('Field/Input')");
  await send('Emulation.setDeviceMetricsOverride', {width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  await screenshot(output + '-components.png');
  const style = await evaluate(`({
    styles:[...document.styleSheets].map(s=>s.href),
    invalid:document.querySelector('input[aria-invalid=true]')?.getAttribute('aria-describedby'),
    small:[...document.querySelectorAll('#app button,#app label,#app .badge')].filter(e=>parseFloat(getComputedStyle(e).fontSize)<13).length,
    short:[...document.querySelectorAll('#app button')].filter(e=>e.getBoundingClientRect().height<44).length
  })`);
  if (style.styles.some(s=>s?.endsWith('/theme.css')) || !style.invalid || style.small || style.short) throw new Error(JSON.stringify(style));
  await evaluate(`(async()=>{
    const {DataTable}=await import('/ui/components/dataTable.js');
    const slot=document.createElement('div'); slot.id='keyboard-fixture'; document.querySelector('#app').prepend(slot);
    window.tableTest=DataTable(slot,{rows:[{id:'a&"b'}],rowKey:r=>r.id,columns:[{key:'id',label:'名称'}],onSelect:r=>{window.selectedTestRow=r.id;}});
    slot.querySelector('button').focus();
  })()`);
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Enter',code:'Enter',windowsVirtualKeyCode:13,text:'\r',unmodifiedText:'\r'});
  await send('Input.dispatchKeyEvent',{type:'keyUp',key:'Enter',code:'Enter',windowsVirtualKeyCode:13});
  if (await evaluate('window.selectedTestRow') !== 'a&"b') throw new Error('Table keyboard activation failed: ' + JSON.stringify(await evaluate("({selected:window.selectedTestRow,active:document.activeElement.outerHTML,fixture:document.querySelector('#keyboard-fixture').innerHTML})")));
  await evaluate(`(async()=>{
    const {Dialog}=await import('/ui/components/dialog.js');
    window.testDialog=Dialog(document.body,{title:'弹窗样式验收',content:'<p>只验证组件，不修改业务数据。</p>',actions:[{label:'关闭'}]});
  })()`);
  await waitFor("!!document.querySelector('[role=dialog]')");
  const focus = await evaluate("document.querySelector('[role=dialog]').contains(document.activeElement)");
  if (!focus) throw new Error('Dialog did not receive focus');
  await screenshot(output + '-dialog.png');
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await waitFor("!document.querySelector('[role=dialog]')");
  if (!await evaluate("document.querySelector('#keyboard-fixture').contains(document.activeElement)")) throw new Error('Dialog did not restore focus');
  await evaluate("window.tableTest.destroy(); document.querySelector('#keyboard-fixture').remove()");
  await evaluate("void import('/ui/components/dialog.js').then(({confirmAction}) => { window.confirmResult='pending'; confirmAction('仅测试取消，不执行任何删除', {title:'删除确认验收',destructive:true}).then(v=>window.confirmResult=v); })");
  await waitFor("!!document.querySelector('dialog:modal')");
  await evaluate("document.querySelector('dialog:modal .dialog-actions button').click()");
  await waitFor("window.confirmResult === false && !document.querySelector('dialog:modal')");
  await send('Emulation.setDeviceMetricsOverride', {width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output + '-components-mobile.png');
  if (await evaluate('document.documentElement.scrollWidth>innerWidth+1')) throw new Error('Component sample overflow');
}
console.log(JSON.stringify({ title: await evaluate('document.title'), errors, screenshots:output }));
socket.close();
await fetch('http://127.0.0.1:9231/json/close/' + tab.id);
if (errors.length) process.exitCode = 1;
