// 本地浏览器验收。需要先启动一个独立的 Chrome 调试实例与控制台：
//   "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new \
//     --remote-debugging-port=9231 --user-data-dir=<临时目录> about:blank
//   ailoom console --port <端口> --no-open      # 用隔离的 HOME / XDG 变量
// 运行：CDP_PORT=9231 node tests/ui_browser.mjs <控制台URL> <截图前缀> [模式] [模式参数…]
// 模式：current（默认，资源库导入）| grouped | cc-switch [CC Switch 目录] | projects <项目目录>
//       | onboarding <项目目录> <本地 Skill 目录> | design [<项目目录> <本地 Skill 目录>]
import { writeFile } from 'node:fs/promises';
const [url, output, mode = 'current'] = process.argv.slice(2);
const CDP = process.env.CDP_PORT || '9231';
// 新标签页先停在 about:blank，再用 Page.navigate 显式导航：较新的 Chrome 不再按 json/new 的查询串打开页面。
const tab = await (await fetch(`http://127.0.0.1:${CDP}/json/new?about:blank`, { method: 'PUT' })).json();
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
  // 每条 CDP 命令最多等 30 秒：卡住时明确报错，而不是让外层超时静默杀掉进程。
  return new Promise((resolve, reject) => {
    const id = ++seq;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP ${method} 30 秒无响应`)); }, 30000);
    pending.set(id, { resolve: v => { clearTimeout(timer); resolve(v); }, reject: e => { clearTimeout(timer); reject(e); } });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
async function waitFor(expression) {
  // 页面导航中求值可能抛错（文档尚未就绪）：视为「还没满足」继续等待，超时后报出最后一次错误。
  let lastError = null;
  for (let n = 0; n < 100; n++) {
    try {
      if (await evaluate(expression)) return;
      lastError = null;
    } catch (e) { lastError = e; }
    await new Promise(r => setTimeout(r, 100));
  }
  await screenshot(output + '-failed.png');
  const text = await evaluate('document.querySelector("#app")?.innerText ?? ""').catch(() => '');
  throw new Error('Timed out: ' + expression + (lastError ? '\n最后一次求值错误: ' + lastError.message : '') + '\n' + text);
}
async function screenshot(path) {
  const shot = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
  await writeFile(path, Buffer.from(shot.data, 'base64'));
}
await send('Runtime.enable');
await send('Page.enable');
await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
await send('Page.navigate', { url });
await waitFor('!!document.querySelector("#app")?.textContent.trim()');
await screenshot(output + '-desktop.png');
if (mode === 'grouped') {
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');
    const {repositoryGroups}=await import('/ui/features/collectionsPanel.js');
    const make=(id,url,n,management='managed')=>({id,url,name:'CC Switch · legacy',management,lock:{ref:'main',resolved_commit:'1234567890abcdef'},references:[],store_path:'/synthetic/store/'+id,resources:Array.from({length:n},(_,i)=>({id:id+i,kind:'skill',name:['computer-use','orca-cli','code-review'][i]||'skill-'+i,description:['控制桌面应用，检查窗口与界面状态。','管理项目工作区、终端和浏览器。','审查代码质量与实现是否符合需求。'][i]||'用于验证资源展示的模拟说明。',path:'skills/'+id+'/'+i}))});
    const sources=[make('a','https://github.com/example/tools.git',3),make('b','git@github.com:example/tools',1,'external'),make('c','https://gitlab.com/example/tools',2)];
    if(repositoryGroups(sources).length!==2) throw Error('Repository alias grouping failed');
    if(repositoryGroups([make('d','/tmp/one',0),make('e','/tmp/two',0)]).length!==2) throw Error('Local paths merged');
    api.collections=async()=>({sources});
    location.hash='#/library';
  })()`);
  // 资源库为平铺目录：同一仓库的多个来源合并为一个来源标签，按资源去重展示。
  // 只统计 mock 来源的条目：本机个人副本也会出现在同一目录里。
  const mocked = "[...document.querySelectorAll('.repository-resource')].filter(li=>li.querySelector('.library-entry-source')?.textContent.endsWith('example/tools'))";
  await waitFor(`${mocked}.length===6`);
  // 同名仓库分属两个平台：标签带平台名，两组可区分
  if (await evaluate("new Set([...document.querySelectorAll('.library-entry-source')].map(e=>e.textContent).filter(t=>t.endsWith('example/tools'))).size") !== 2) throw new Error('Same-named repositories on different hosts must be distinguishable');
  if (await evaluate("[...document.querySelectorAll('.library-entry-source')].some(e=>e.textContent.includes('CC Switch'))")) throw new Error('Catalog source labels must come from repository identity');
  await screenshot(output + '-grouped-desktop.png');
  await evaluate("document.querySelector('[data-search]').value='code-review'; document.querySelector('[data-search]').dispatchEvent(new Event('input'))");
  if (!await evaluate(`${mocked}.length===1`)) throw new Error('Resource search failed');
  await evaluate("document.querySelector('[data-search]').value=''; document.querySelector('[data-search]').dispatchEvent(new Event('input'))");
  await send('Emulation.setDeviceMetricsOverride', {width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output + '-grouped-mobile.png');
  if (await evaluate('document.documentElement.scrollWidth > innerWidth + 1')) throw new Error('Grouped library mobile overflow');
  await send('Emulation.setDeviceMetricsOverride', {width:1440,height:1000,deviceScaleFactor:1,mobile:false});
}
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
    if (!await evaluate("document.querySelector('[name=cc-management]:checked').value==='managed'")) throw new Error('AILoom management must be the default');
    await evaluate("const mode=document.querySelector('[name=cc-management][value=external]'); mode.checked=true; mode.dispatchEvent(new Event('change')); document.querySelector('[data-cc-prepare]').click()");
    await waitFor("document.querySelector('[data-cc-preview]').textContent.includes('fixture-skill') && !document.querySelector('[data-cc-prepare]').disabled");
    if (!await evaluate("document.querySelector('[data-cc-preview]').textContent.includes('CC Switch 继续维护') && (!document.querySelector('[data-cc-apply]').hidden || document.querySelector('[data-cc-preview]').textContent.includes('已存在'))")) throw new Error('External preview did not produce a registrable source');
    await screenshot(output + '-cc-external-preview.png');
    if (!await evaluate("document.querySelector('[data-cc-apply]').hidden")) {
      await evaluate("document.querySelector('[data-cc-apply]').click()");
      await waitFor("document.querySelector('[data-cc-status]').textContent.includes('已登记') && !document.querySelector('[data-cc-read]').disabled");
    }
    if (!await evaluate("document.querySelector('[data-sources]').textContent.includes('原目录维护') && !document.querySelector('[data-sources] [data-check-one]')")) throw new Error('Externally managed source must not expose Git update controls');
  }
  await evaluate("document.querySelector('[data-cc-fallback]').open=true");
  await evaluate("document.querySelector('[data-cc-json]').value='invalid'; document.querySelector('[data-cc-scan]').click()");
  await waitFor("document.querySelector('[data-cc-status]').textContent.includes('不是有效的 JSON')");
  const fixture = [{name:'fixture-skill',directory:'fixture-skill',repo_owner:'fixture-owner',repo_name:'fixture-repo',repo_branch:'main',readme_url:'https://github.com/fixture-owner/fixture-repo/blob/main/plugins/fixture-skill/SKILL.md'},{name:'没有来源'},{name:'<script>fixture</script>',repo_url:'file:///tmp/forbidden'}];
  await evaluate(`document.querySelector('[data-cc-json]').value=${JSON.stringify(JSON.stringify(fixture))}; document.querySelector('[data-cc-json]').dispatchEvent(new Event('input')); document.querySelector('[data-cc-scan]').click()`);
  await waitFor("document.querySelector('[data-cc-status]').textContent.includes('发现 3 项') && !document.querySelector('[data-cc-scan]').disabled");
  if (!await evaluate("[...document.querySelectorAll('.cc-skill-row')].every(row=>row.getBoundingClientRect().height<=86)")) throw new Error('Import records are not compact');
  await evaluate("document.querySelector('[data-cc-search]').value='fixture-skill'; document.querySelector('[data-cc-search]').dispatchEvent(new Event('input'))");
  if (!await evaluate("document.querySelectorAll('[data-cc-row]:not([hidden])').length===1")) throw new Error('Import source search failed');
  await evaluate("document.querySelector('[data-cc-search]').value=''; document.querySelector('[data-cc-search]').dispatchEvent(new Event('input'))");
  if (!await evaluate("document.querySelectorAll('[data-cc-select]:checked').length===1 && document.querySelectorAll('[data-cc-select]:disabled').length===2 && document.querySelector('[data-cc-items]').textContent.includes('plugins/fixture-skill') && !document.querySelector('[data-cc-items] script')")) throw new Error('Migration scan selection/path/escaping failed');
  await screenshot(output + '-cc-switch-desktop.png');
  await evaluate("document.querySelector('[data-cc-none]').click()");
  if (!await evaluate("document.querySelector('[data-cc-prepare]').disabled")) throw new Error('Empty migration selection must be disabled');
  await evaluate("document.querySelector('[data-cc-all]').click()");
  if (!await evaluate("!document.querySelector('[data-cc-prepare]').disabled && document.querySelectorAll('[data-cc-select]:checked').length===1")) throw new Error('Invalid sources must stay unselected');
  const manySkills = Array.from({length:60}, (_,n)=>({name:`skill-${n}`,repo_owner:'fixture',repo_name:'skills',directory:`skill-${n}`}));
  await evaluate(`document.querySelector('[data-cc-json]').value=${JSON.stringify(JSON.stringify(manySkills))}; document.querySelector('[data-cc-json]').dispatchEvent(new Event('input')); document.querySelector('[data-cc-scan]').click()`);
  await waitFor("document.querySelector('[data-cc-status]').textContent.includes('发现 60 项') && !document.querySelector('[data-cc-scan]').disabled");
  if (!await evaluate("document.querySelector('.cc-skill-list').clientHeight<=320 && document.querySelector('.cc-skill-list').scrollHeight>document.querySelector('.cc-skill-list').clientHeight && [...document.querySelectorAll('.cc-skill-row')].every(r=>r.getBoundingClientRect().height<=72)")) throw new Error('Large import list is not bounded/compact');
  await evaluate("document.querySelector('[data-cc-fallback]').open=false; document.querySelector('[data-cc-items]').scrollIntoView({block:'start'})");
  await screenshot(output + '-cc-compact-60.png');
  await send('Emulation.setDeviceMetricsOverride', {width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output + '-cc-switch-mobile.png');
  if (await evaluate("document.documentElement.scrollWidth > innerWidth + 1 || document.querySelector('[data-cc-json]').closest('dialog').scrollWidth > document.querySelector('[data-cc-json]').closest('dialog').clientWidth + 1")) throw new Error('Migration dialog overflow');
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await waitFor("!document.querySelector('dialog:modal')");
  if (!await evaluate("document.activeElement===document.querySelector('[data-cc-switch]')")) throw new Error('Migration focus not restored');
}
if (mode === 'projects' || mode === 'design') {
  // 目录优先工作台：添加项目 Dialog → 目录页 → 宿主开关 → 管理列表搜索/分类。
  const [project] = process.argv.slice(5);
  await waitFor("location.hash === '#/projects' && !!document.querySelector('[data-add-project]')");
  await evaluate("document.querySelector('[data-add-project]').focus(); document.querySelector('[data-add-project]').click()");
  await waitFor("document.querySelector('input[name=root]')?.closest('dialog')?.matches(':modal')");
  await screenshot(output + '-new-project-dialog.png');
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await waitFor("!document.querySelector('dialog:modal')");
  if (!await evaluate("document.activeElement === document.querySelector('[data-add-project]')")) throw new Error('New-project focus not restored');
  await evaluate("document.querySelector('[data-add-project]').click()");
  await waitFor("!!document.querySelector('dialog:modal input[name=root]')");
  await evaluate(`const root=document.querySelector('dialog:modal input[name=root]'); root.value=${JSON.stringify(project)}; root.dispatchEvent(new Event('change'))`);
  await waitFor("!document.querySelector('dialog:modal [type=submit]').disabled");
  await evaluate("const f=document.querySelector('dialog:modal form'); f.elements.name.value='浏览器验收项目'; f.elements.category.value='验收'; f.requestSubmit()");
  await waitFor("!document.querySelector('dialog:modal') && /^#\\/projects\\/.+/.test(location.hash) && document.querySelector('#app').innerText.includes('浏览器验收项目')");
  await waitFor("!!document.querySelector('[data-host=claude]')");
  const before = await evaluate("document.querySelector('[data-host=claude]').getAttribute('aria-pressed')");
  await evaluate("document.querySelector('[data-host=claude]').click()");
  await waitFor(`document.querySelector('[data-host=claude]')?.getAttribute('aria-pressed') === ${JSON.stringify(before === 'true' ? 'false' : 'true')}`);
  await screenshot(output + '-workspace.png');
  await send('Emulation.setDeviceMetricsOverride', { width:390,height:844,deviceScaleFactor:1,mobile:false });
  await screenshot(output + '-workspace-mobile.png');
  if (await evaluate('document.documentElement.scrollWidth > innerWidth + 1')) throw new Error('Workspace overflow');
  await send('Emulation.setDeviceMetricsOverride', { width:1440,height:1000,deviceScaleFactor:1,mobile:false });
  // 未知项目 id：显示「找不到这个项目」，而不是首次使用引导（U-05）
  await evaluate("location.hash='#/projects/does-not-exist'");
  await waitFor("document.querySelector('#app').innerText.includes('找不到这个项目') && !document.querySelector('#app').innerText.includes('添加你的第一个目录')");
  await evaluate("location.hash='#/projects/manage'");
  await waitFor("!!document.querySelector('[data-search]') && !!document.querySelector('[data-list] article')");
  if (mode === 'design') {
    await waitFor("!!document.querySelector('[data-filter]')?.parentElement.querySelector('[role=combobox]')");
    const fields = await evaluate("[document.querySelector('[data-search]'), document.querySelector('[data-filter]').parentElement.querySelector('[role=combobox]'), document.querySelector('[data-kind]').parentElement.querySelector('[role=combobox]')].map(e=>{const s=getComputedStyle(e);return [e.getBoundingClientRect().height,s.borderRadius,s.borderColor,s.backgroundColor,s.paddingLeft,s.fontSize,s.appearance]})");
    if (fields.some(f => JSON.stringify(f)!==JSON.stringify(fields[0]))) throw new Error('Search and select styles differ: '+JSON.stringify(fields));
    await screenshot(output + '-toolbar.png');
  }
  await evaluate("document.querySelector('[data-search]').value='不存在的项目'; document.querySelector('[data-search]').dispatchEvent(new Event('input'))");
  await waitFor("document.querySelector('[data-list]').textContent.includes('没有匹配')");
  await evaluate("document.querySelector('[data-search]').value=''; document.querySelector('[data-search]').dispatchEvent(new Event('input')); document.querySelector('[data-filter]').value='验收'; document.querySelector('[data-filter]').dispatchEvent(new Event('change'))");
  await waitFor("document.querySelector('[data-list]').textContent.includes('浏览器验收项目')");
  await screenshot(output + '-projects.png');
  await send('Emulation.setDeviceMetricsOverride', { width:390,height:844,deviceScaleFactor:1,mobile:false });
  await screenshot(output + '-projects-mobile.png');
  if (await evaluate('document.documentElement.scrollWidth > innerWidth + 1')) throw new Error('Project list overflow');
  await send('Emulation.setDeviceMetricsOverride', { width:1440,height:1000,deviceScaleFactor:1,mobile:false });
}
if (mode === 'onboarding') {
  // 三步引导页只做讲解与直达入口，不承载配置动作。
  for (const [index, target] of [[0, '#/projects/manage'], [1, '#/library'], [2, '#/projects']]) {
    await evaluate("location.hash='#/onboarding'");
    await waitFor("document.querySelectorAll('.wizard .step').length === 3 && document.querySelectorAll('.wizard .step button').length === 3");
    if (index === 0) await screenshot(output + '-onboarding.png');
    await evaluate(`document.querySelectorAll('.wizard .step button')[${index}].click()`);
    await waitFor(`location.hash === ${JSON.stringify(target)}`);
  }
}
if (mode !== 'before') {
  await evaluate('location.hash = "#/library"');
  await waitFor('!!document.querySelector("[data-add]")');
  await evaluate('document.querySelector("[data-add]").click()');
  await waitFor('document.querySelector("[data-provider]")?.closest("dialog")?.matches(":modal")');
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
    // 成功后对话框关闭，资源库列表出现新资源，并有页面级成功提示。
    const skillName = process.argv[6].split('/').filter(Boolean).pop();
    await waitFor(`!document.querySelector('dialog:modal') && [...document.querySelectorAll('.repository-resource h3')].some(h=>h.textContent.includes(${JSON.stringify(skillName)}))`);
    if (!await evaluate("document.querySelector('#toast')?.textContent.includes('已加入资源库')")) throw new Error('导入成功提示不可见');
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
await fetch(`http://127.0.0.1:${CDP}/json/close/` + tab.id);
if (errors.length) process.exitCode = 1;
