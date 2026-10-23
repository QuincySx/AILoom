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
async function chooseFilter(selector, value) {
  await waitFor(`!!document.querySelector(${JSON.stringify(selector)})?.parentElement.querySelector('[role=combobox]')`);
  await evaluate(`document.querySelector(${JSON.stringify(selector)}).parentElement.querySelector('[role=combobox]').click()`);
  await waitFor(`document.querySelector(${JSON.stringify(selector)}).parentElement.querySelector('[role=combobox]').getAttribute('aria-expanded')==='true'`);
  await evaluate(`{const source=document.querySelector(${JSON.stringify(selector)});const index=[...source.options].findIndex(o=>o.value===${JSON.stringify(value)});if(index<0)throw Error('Missing filter value');source.parentElement.querySelector('[role=option][data-index="'+index+'"]').click();}`);
}
async function chooseRepository(value) {
  await waitFor("!!document.querySelector('[data-repository-filter]')");
  await evaluate(`{const button=[...document.querySelectorAll('[data-repository-filter]')].find(b=>b.dataset.repositoryFilter===${JSON.stringify(value)});if(!button)throw Error('Missing repository filter');button.click();}`);
}
await send('Runtime.enable');
await send('Page.enable');
await send('Page.bringToFront');
// 统一用占宽度的经典滚动条（Linux CI 的默认），macOS 的浮动滚动条会掩盖宽度问题。
await send('Page.addScriptToEvaluateOnNewDocument', { source: "addEventListener('DOMContentLoaded',()=>{const s=document.createElement('style');s.textContent='::-webkit-scrollbar{width:15px;height:15px}';document.head.append(s);})" });
await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
await send('Page.navigate', { url });
await waitFor(mode==='report'?'!!document.querySelector("main")?.textContent.trim()':'!!document.querySelector("#app")?.textContent.trim()');
if(!mode.startsWith('global-targets')&&mode!=='journey')await screenshot(output + '-desktop.png');
async function configureTool(tool,enabled) {
  await evaluate("document.querySelector('[data-tool-change]').click()");
  await waitFor(`!!document.querySelector('dialog:modal [data-host="${tool}"]')`);
  const before=await evaluate(`document.querySelector('dialog:modal [data-host="${tool}"]').getAttribute('aria-pressed')==='true'`);
  await evaluate(`{const parent=document.querySelector('[data-use-parent-tools]');if(parent?.checked)parent.click();const button=document.querySelector('dialog:modal [data-host="${tool}"]');if(${enabled===undefined?'true':JSON.stringify(enabled)}${enabled===undefined?'':"!== (button.getAttribute('aria-pressed')==='true')"})button.click();document.querySelector('dialog:modal .dialog-actions [data-variant=default]').click();}`);
  await waitFor("!document.querySelector('dialog:modal') && !!document.querySelector('[data-tool-change]') && !document.querySelector('[data-tool-change]').disabled");
  return before;
}

if(mode==='v27-flow')await (await import('./ui_design_v27.mjs')).verifyFlow({evaluate,waitFor,screenshot,output,configureTool,project:process.argv[5]});
if(mode==='v27')await (await import('./ui_design_v27.mjs')).verifyDesign({evaluate,waitFor,send,screenshot,output});
if(mode==='worktree-cleanup')await (await import('./ui_worktree_cleanup.mjs')).verifyWorktreeCleanup({evaluate,waitFor,send,screenshot,output});
if(mode==='knowledge-organization')await (await import('./ui_knowledge_organization.mjs')).verifyKnowledgeOrganization({evaluate,waitFor,send,screenshot,output});
if(mode==='knowledge-ingestion')await (await import('./ui_knowledge_ingestion.mjs')).verifyKnowledgeIngestion({evaluate,waitFor,send,screenshot,output});
if(mode==='journey') {
  // 正常使用的连续流程；仅接受专用隔离 fixtures，不改真实项目或全局目录。
  const [project,skills,teamProject,replacement]=process.argv.slice(5);
  if(![project,skills,teamProject,replacement].every(p=>p?.includes('/user-journeys-')))throw Error('journey requires isolated user-journeys fixtures');
  if(!await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');return (await api.globalSkills()).home===${JSON.stringify(project.slice(0,project.lastIndexOf('/'))+'/home')};})()`))throw Error('journey requires an isolated global home');
  const id='personal/skill/personal/journey-alpha';
  await evaluate("location.hash='#/projects'");
  await waitFor("!!document.querySelector('[data-add-project]')");
  await evaluate("document.querySelector('[data-add-project]').click()");
  await waitFor("!!document.querySelector('dialog:modal input[name=root]')");
  await evaluate(`{const i=document.querySelector('dialog:modal input[name=root]');i.value=${JSON.stringify(project)};i.dispatchEvent(new Event('change'));}`);
  await waitFor("!document.querySelector('dialog:modal [type=submit]').disabled");
  await evaluate("document.querySelector('dialog:modal form').requestSubmit()");
  await waitFor("!document.querySelector('dialog:modal') && !!document.querySelector('[data-tool-change]')");
  const projectHash=await evaluate('location.hash');
  await evaluate(`location.hash=${JSON.stringify(projectHash+'/settings')}`);
  await waitFor("document.querySelector('[data-team-source]')?.hidden===true");
  await evaluate("location.hash='#/library'");
  await waitFor("!!document.querySelector('[data-git-add]')");
  await evaluate(`document.querySelector('[data-git-add]').click();document.querySelector('[data-git-url]').value=${JSON.stringify(skills)};document.querySelector('[data-git-form]').requestSubmit()`);
  await waitFor("document.querySelectorAll('[data-git-item]').length===2 && !document.querySelector('[data-git-scan]').disabled");
  await evaluate("document.querySelector('[data-git-item][value=\"skills/journey-alpha\"]').click();document.querySelector('[data-git-install]').click()");
  await waitFor("!document.querySelector('dialog:modal') && document.querySelectorAll('[data-library-id]').length===1");
  await evaluate("document.querySelector('[data-group-install]').click()");
  await waitFor("document.querySelectorAll('[data-git-item]').length===2 && document.querySelector('[data-git-item][value=\"skills/journey-alpha\"]').disabled");
  if(await evaluate("document.querySelector('[data-git-item][value=\"skills/journey-beta\"]').disabled"))throw Error('Uninstalled Skill cannot be selected');
  await evaluate("document.querySelector('[data-git-cancel]').click()");
  await evaluate(`location.hash=${JSON.stringify(projectHash)}`);
  await waitFor("!!document.querySelector('[data-tool-change]')");
  await configureTool('claude',true);
  await waitFor("!document.querySelector('[data-add-capability]').disabled");
  await evaluate("document.querySelector('[data-add-capability]').click()");
  await waitFor(`!!document.querySelector('[data-picker-item][value="${id}"]')`);
  await evaluate(`document.querySelector('[data-picker-item][value="${id}"]').click();document.querySelector('[data-picker-submit]').click()`);
  await waitFor("!document.querySelector('dialog:modal') && !!document.querySelector('[data-capability-list] .workspace-resource')");
  await evaluate("document.querySelector('[data-open-worktree]')?.click()");
  await waitFor("!!document.querySelector('[data-review]') && !document.querySelector('[data-review]').disabled");
  await evaluate("document.querySelector('[data-review]').click()");
  await waitFor("!document.querySelector('[data-confirm-apply]').disabled");
  await evaluate("document.querySelector('[data-confirm-apply]').click()");
  await waitFor("document.querySelector('[data-close-review]')?.textContent==='返回目录' && !document.querySelector('[data-close-review]').disabled");
  if(!await evaluate("document.querySelector('[data-review-content]').textContent.includes('已应用')"))throw Error('Project apply did not finish');
  await evaluate("document.querySelector('[data-close-review]').click();location.hash='#/tasks'");
  await waitFor("document.querySelector('.tasks-page')?.textContent.includes('应用配置') && document.querySelector('.tasks-page')?.textContent.includes('成功')");
  await evaluate("location.hash='#/library/global'");
  await waitFor("!!document.querySelector('[data-add-skill]') && !document.querySelector('[data-add-skill]').disabled");
  await evaluate("document.querySelector('[data-add-skill]').click()");
  await waitFor(`!!document.querySelector('[data-global-add-id="${id}"]')`);
  await evaluate(`{const b=document.querySelector('[data-global-add-target-skill="${id}"][data-target-key="agents"]');if(b.getAttribute('aria-pressed')==='true')b.click();document.querySelector('[data-global-add-id="${id}"]').click();document.querySelector('[data-global-add-submit]').click();}`);
  await waitFor(`document.querySelector('[data-global-skill="${id}"]')?.textContent.includes('已启用') && !document.querySelector('[data-refresh]').disabled`);
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  await evaluate(`document.querySelector('[data-global-target-skill="${id}"][data-target-key="claude"]').click()`);
  await waitFor(`!document.querySelector('[data-global-skill="${id}"]') && !document.querySelector('[data-refresh]').disabled`);
  if(!await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');return (await api.libraryList()).entries.some(e=>e.id===${JSON.stringify(id)});})()`))throw Error('Global removal deleted the library copy');
  const teamHash=await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');const state=await api.state();const repo=state.repos.find(r=>Object.values(r.worktrees||{}).some(w=>w.path===${JSON.stringify(teamProject)}));if(!repo)throw Error('Team fixture missing');return '#/projects/'+repo.repo_id;})()`);
  await evaluate(`location.hash=${JSON.stringify(teamHash+'/settings')}`);
  await waitFor("!!document.querySelector('[data-edit-team-source]') && !document.querySelector('[data-edit-team-source]').disabled");
  await evaluate("document.querySelector('[data-edit-team-source]').click()");
  await waitFor("!!document.querySelector('[data-source-save]') && !document.querySelector('[data-source-save]').disabled");
  await evaluate("document.querySelector('[data-source-git]').click()");
  if(!await evaluate("document.querySelector('[data-source-cache-root]').value.length>0 && document.querySelector('[data-source-cache-default]').hidden"))throw Error('Git download default missing');
  await evaluate(`document.querySelector('[data-source-existing]').click();document.querySelector('[data-source-directory]').value=${JSON.stringify(replacement)};document.querySelector('dialog:modal form').requestSubmit()`);
  await waitFor("!document.querySelector('dialog:modal') && !document.querySelector('[data-edit-team-source]').disabled");
  if(!await evaluate("document.querySelector('[data-team-source-info]').textContent.includes('/team-new')"))throw Error('Healthy source management did not refresh');
}
if(mode==='report') {
  await waitFor("[...document.images].every(i=>i.complete&&i.naturalWidth>0)");
  if(!await evaluate("[...document.querySelectorAll('.doc-toc a')].every(a=>document.querySelector(a.getAttribute('href')))"))throw Error('Report navigation targets missing');
  await evaluate("document.querySelector('.doc-toc a[href=\"#screens\"]').click()");
  await waitFor("location.hash==='#screens' && Math.abs(document.querySelector('#screens').getBoundingClientRect().top)<120");
  await screenshot(output+'-screens.png');
  await evaluate("document.querySelector('.doc-toc a[href=\"#result\"]').click()");
  await waitFor("location.hash==='#result' && Math.abs(document.querySelector('#result').getBoundingClientRect().top)<120");
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  await evaluate("window.scrollTo({top:0,behavior:'instant'})");
  await screenshot(output+'-mobile.png');
  if(await evaluate('document.documentElement.scrollWidth>innerWidth+1'))throw Error('Report mobile overflow');
  if(!await evaluate("[...document.querySelectorAll('a[href$=\".png\"]')].every(a=>[...document.images].some(i=>i.src===a.href))"))throw Error('Report image links mismatch');
}
if(mode==='repository-filters') {
  const repo=process.argv[5];
  if(!repo)throw Error('repository-filters requires an isolated fixture repository');
  await evaluate("location.hash='#/library'");
  await waitFor("!!document.querySelector('[data-git-add]')");
  if(await evaluate("(async()=>{const {api}=await import('/ui/services/api.js');return (await api.libraryList()).entries.length;})()")!==0)throw Error('Filter install test requires an empty isolated library');
  await evaluate(`document.querySelector('[data-git-add]').click();document.querySelector('[data-git-url]').value=${JSON.stringify(repo)};document.querySelector('[data-git-form]').requestSubmit()`);
  await waitFor("document.querySelectorAll('[data-git-item]').length===36 && !document.querySelector('[data-git-scan]').disabled");
  await evaluate("document.querySelector('[data-git-search]').value='design';document.querySelector('[data-git-search]').dispatchEvent(new Event('input'))");
  if(!await evaluate("document.querySelectorAll('[data-git-item]').length===12"))throw Error('Category must reduce a large repository list');
  await evaluate("document.querySelector('[data-git-search]').value='fixture-design-01';document.querySelector('[data-git-search]').dispatchEvent(new Event('input'));document.querySelector('[data-git-all]').click()");
  if(!await evaluate("document.querySelectorAll('[data-git-item]:checked').length===1"))throw Error('Select all must respect search within category');
  await evaluate("document.querySelector('[data-git-search]').value='';document.querySelector('[data-git-search]').dispatchEvent(new Event('input'))");
  await evaluate("document.querySelector('[data-git-search]').value='development';document.querySelector('[data-git-search]').dispatchEvent(new Event('input'))");
  if(!await evaluate("document.querySelectorAll('[data-git-item]:checked').length===0 && document.querySelector('[data-git-count]').textContent.includes('筛选外 1 个')"))throw Error('Hidden selections must remain visible in the count');
  await evaluate("document.querySelector('[data-git-item][value=\"skills/development/fixture-development-01\"]').click()");
  await screenshot(output+'-picker-desktop.png');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-picker-mobile.png');
  if(await evaluate("document.documentElement.scrollWidth>innerWidth+1 || document.querySelector('dialog:modal').scrollWidth>document.querySelector('dialog:modal').clientWidth+1"))throw Error('Category picker mobile overflow');
  await evaluate("document.querySelector('[data-git-install]').click()");
  await waitFor("!document.querySelector('dialog:modal') && document.querySelectorAll('[data-library-id]').length===2");
  const installed=await evaluate("(async()=>{const {api}=await import('/ui/services/api.js');return (await api.libraryList()).entries.map(e=>e.name).sort();})()");
  if(installed.join(',')!=='fixture-design-01,fixture-development-01')throw Error('Only selected Skills across categories should install');
  const key=await evaluate("document.querySelector('[data-repository-filter]:not([data-repository-filter=\"\"])').dataset.repositoryFilter");
  await chooseRepository(key);
  await evaluate("document.querySelector('[data-search]').value='development';document.querySelector('[data-search]').dispatchEvent(new Event('input'))");
  if(!await evaluate("document.querySelectorAll('[data-library-id]').length===1 && document.querySelector('[data-library-id]').textContent.includes('fixture-development-01') && document.querySelector('[data-check]').textContent==='检查筛选项'"))throw Error('Library category filter failed');
  await evaluate("(async()=>{const {api}=await import('/ui/services/api.js');const check=api.checkUpdate;window.filteredChecks=[];api.checkUpdate=async name=>{window.filteredChecks.push(name);return check(name);};document.querySelector('[data-check]').click();})()");
  await waitFor("!document.querySelector('[data-check]').disabled && document.querySelector('[data-msg]').textContent.includes('检查完成')");
  if(!await evaluate("window.filteredChecks.join(',')==='fixture-development-01'"))throw Error('Filtered check touched hidden resources');
  await screenshot(output+'-library-mobile.png');
  if(await evaluate('document.documentElement.scrollWidth>innerWidth+1'))throw Error('Library filters mobile overflow');
  await send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-library-desktop.png');
  await evaluate("document.querySelector('[data-library-clear]').click()");
  await waitFor("document.querySelectorAll('[data-library-id]').length===2");
  await evaluate("document.querySelector('[data-group-install]').click()");
  await waitFor("document.querySelectorAll('[data-git-item]').length===36 && !document.querySelector('[data-git-scan]').disabled");
  await evaluate("document.querySelector('[data-git-search]').value='docs';document.querySelector('[data-git-search]').dispatchEvent(new Event('input'))");
  await evaluate("document.querySelector('[data-git-all]').click();document.querySelector('[data-git-cancel]').click()");
  if(await evaluate("(async()=>{const {api}=await import('/ui/services/api.js');return (await api.libraryList()).entries.length;})()")!==2)throw Error('Cancel after selecting a category wrote resources');
}
if(mode==='source-binding-collection') {
  const repo=process.argv[5];
  await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');await api.approveDir(${JSON.stringify(repo)});const p=await api.collectionPreview({name:'browser-collection',url:${JSON.stringify(repo)},ref:'main'});await api.collectionApply(p.preview_id);window.boundCollection=(await api.collections()).sources.find(s=>s.name==='browser-collection');location.hash='#/library';})()`);
  await waitFor("!!document.querySelector('[data-catalog-source]')");
  await evaluate("document.querySelector('[data-catalog-source]').click();[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(b=>b.textContent==='变更来源').click()");
  await waitFor("!!document.querySelector('dialog:modal [data-binding-url]')");
  await evaluate(`document.querySelector('[data-binding-url]').value=${JSON.stringify('file://'+repo)};document.querySelector('[data-binding-url]').dispatchEvent(new Event('input',{bubbles:true}));document.querySelector('[data-binding-ref]').value='';document.querySelector('[data-binding-preview]').click()`);
  await waitFor("!!document.querySelector('[data-binding-relationship]') && !document.querySelector('[data-binding-preview]').disabled");
  await evaluate("document.querySelector('[data-binding-relationship]').value='same';document.querySelector('[data-binding-relationship]').dispatchEvent(new Event('change'));document.querySelector('[data-binding-save]').click()");
  await waitFor("document.querySelector('[data-binding-message]').textContent.includes('来源已保存') && !document.querySelector('[data-binding-cancel]').disabled");
  await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');const next=(await api.collections()).sources.find(s=>s.id===window.boundCollection.id);if(next.url!==window.boundCollection.url||JSON.stringify(next.lock)!==JSON.stringify(window.boundCollection.lock)||JSON.stringify(next.resources.map(r=>r.id))!==JSON.stringify(window.boundCollection.resources.map(r=>r.id)))throw Error('Collection binding save changed confirmed version or identity');document.querySelector('[data-binding-cancel]').click();document.querySelector('[data-catalog-source]').click();})()`);
  await waitFor("!!document.querySelector('dialog:modal .dialog-actions button')");
  if(!await evaluate(`document.querySelector('dialog:modal').textContent.includes(${JSON.stringify('file://'+repo)})`))throw Error('Source details must display the new update connection');
  await evaluate("[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(b=>b.textContent==='变更来源').click()");
  await waitFor("!!document.querySelector('[data-binding-ref]')");
  if(!await evaluate("document.querySelector('[data-binding-ref]').value===''") )throw Error('Collection cleared branch fell back to old lock');
  await evaluate("document.querySelector('[data-binding-cancel]').click();document.querySelector('[data-group-install]').click()");
  await waitFor("!!document.querySelector('dialog:modal [data-git-url]')");
  if(!await evaluate(`document.querySelector('[data-git-url]').value===${JSON.stringify('file://'+repo)} && document.querySelector('[data-git-ref]').value===''`))throw Error('Install other Skills must use current transport and default branch');
  await screenshot(output+'-collection-install.png');
  await evaluate("document.querySelector('[data-git-cancel]').click()");
}
if(mode==='source-binding') {
  const repo=process.argv[5];
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');
    await api.approveDir(${JSON.stringify(repo)});
    await api.libraryImport(${JSON.stringify(repo+'/skills/alpha')},undefined,true);
    window.bindingBefore=(await api.libraryList()).entries.find(e=>e.name==='alpha');
    window.bindingScan=await api.gitSkillsPreview(${JSON.stringify(repo)},'main');
    location.hash='#/library';
  })()`);
  await waitFor("!!document.querySelector('[data-catalog-binding]')");
  await evaluate("document.querySelector('[data-catalog-binding]').click()");
  await waitFor("!!document.querySelector('dialog:modal [data-binding-detect]')");
  await evaluate("document.querySelector('[data-binding-detect]').click()");
  await waitFor("document.querySelector('[data-binding-remotes]').options.length===3 && !document.querySelector('[data-binding-preview]').disabled");
  if(!await evaluate("document.querySelector('[data-binding-url]').value==='' && document.querySelector('[data-binding-mode]').value==='local'"))throw Error('Multiple remotes need an explicit selection; local development must remain the default');
  await screenshot(output+'-multiple-remotes.png');
  await evaluate(`document.querySelector('[data-binding-remotes]').value=${JSON.stringify(repo)};document.querySelector('[data-binding-remotes]').dispatchEvent(new Event('change'));document.querySelector('[data-binding-preview]').click()`);
  await waitFor("!!document.querySelector('.source-binding-preview') && !document.querySelector('[data-binding-save]').disabled");
  await screenshot(output+'-local-preview.png');
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');const original=api.sourceBinding;
    window.bindingOriginal=original;
    api.sourceBinding=body=>body.action==='apply'?new Promise(resolve=>{window.bindingRelease=async()=>resolve(await original(body));}):original(body);
    document.querySelector('[data-binding-save]').click();
  })()`);
  await waitFor("!!window.bindingRelease && document.querySelector('dialog:modal').getAttribute('closedby')==='none'");
  for(let i=0;i<2;i++){await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape'});await send('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape'});}
  if(!await evaluate("!!document.querySelector('dialog:modal [data-binding-save]') && document.querySelector('[data-binding-cancel]').disabled"))throw Error('Saving must remain visible and prevent Escape dismissal');
  await evaluate("window.bindingRelease()");
  await waitFor("document.querySelector('[data-binding-message]').textContent.includes('来源已保存') && !document.querySelector('[data-binding-cancel]').disabled");
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');api.sourceBinding=window.bindingOriginal;
    const entry=(await api.libraryList()).entries.find(e=>e.name==='alpha');
    if(entry.id!==window.bindingBefore.id||entry.source.source_kind!=='local'||entry.source.imported_digest!==window.bindingBefore.source.imported_digest)throw Error('Association changed installed content or identity');
    let rejected=false;try{await api.gitSkillsInstall(window.bindingScan.preview_id,['skills/beta']);}catch(e){rejected=e.status===409;}
    if(!rejected)throw Error('Old catalog selection must be invalidated by rebinding');
    document.querySelector('[data-binding-cancel]').click();
    document.querySelector('[data-catalog-binding]').click();
    document.querySelector('[data-binding-url]').value='git@quincy-github:Example/skills.git';
    document.querySelector('[data-binding-url]').dispatchEvent(new Event('input',{bubbles:true}));
    document.querySelector('[data-binding-ref]').value='';
    document.querySelector('[data-binding-advanced]').open=true;
    document.querySelector('[data-binding-alias]').value='quincy-github';
    document.querySelector('[data-binding-host]').value='github.com';
    document.querySelector('[data-binding-user]').value='git';
    document.querySelector('[data-binding-port]').value='22';
    document.querySelector('[data-binding-preview]').click();
  })()`);
  await waitFor("!!document.querySelector('[data-binding-relationship]') && !document.querySelector('[data-binding-preview]').disabled");
  if(!await evaluate("document.querySelector('[data-binding-save]').disabled"))throw Error('An unverified upstream must require explicit confirmation');
  await screenshot(output+'-alias-preview.png');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  await evaluate("document.querySelector('[data-binding-impact]').scrollIntoView({block:'end'})");
  await screenshot(output+'-alias-mobile.png');
  if(await evaluate("document.documentElement.scrollWidth>innerWidth+1 || document.querySelector('dialog:modal').scrollWidth>document.querySelector('dialog:modal').clientWidth+1"))throw Error('Binding modal overflows mobile viewport');
  await evaluate("document.querySelector('[data-binding-relationship]').value='replace';document.querySelector('[data-binding-relationship]').dispatchEvent(new Event('change'));document.querySelector('[data-binding-save]').click()");
  await waitFor("document.querySelector('[data-binding-message]').textContent.includes('来源已保存') && !document.querySelector('[data-binding-cancel]').disabled");
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');const entry=(await api.libraryList()).entries.find(e=>e.name==='alpha');
    if(entry.connection_url!=='git@quincy-github:Example/skills.git'||entry.repository_key!=='github.com/Example/skills'||entry.source.ref!==undefined)throw Error('SSH alias transport, grouping or cleared ref incorrect');
    document.querySelector('[data-binding-cancel]').click();document.querySelector('[data-catalog-binding]').click();
    if(document.querySelector('[data-binding-ref]').value!=='')throw Error('Cleared branch must not fall back to old main');
    document.querySelector('[data-binding-cancel]').click();
  })()`);
  await screenshot(output+'-group-mobile.png');
  await send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-group-desktop.png');
}
if(mode==='source-failure'){
  const project=process.argv[5];
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');
    const state=await api.state();
    const repo=state.repos.find(r=>Object.values(r.worktrees||{}).some(w=>w.path===${JSON.stringify(project)}));
    if(!repo)throw Error('Expected a registered source-failure fixture');
    window.sourceFailureWrites=[];
    const original=window.fetch;
    window.fetch=(path,options)=>{
      const action=options?.body?JSON.parse(options.body).action:null;
      if(String(path).startsWith('/api/jobs/')||String(path).startsWith('/api/profile/select')||String(path).startsWith('/api/profile/scope')||String(path)==='/api/native-files'&&['save','delete','restore'].includes(action))window.sourceFailureWrites.push(String(path));
      return original(path,options);
    };
    location.hash='#/projects/'+repo.repo_id;
  })()`);
  await waitFor("!!document.querySelector('[data-config-unavailable]')");
  await waitFor("!!document.querySelector('.native-files [data-list] article')");
  if(!await evaluate("document.querySelector('[data-config-unavailable]').textContent.includes('团队来源暂不可用') && document.querySelector('[data-config-unavailable]').textContent.includes('资源源目录不存在')"))throw Error('Source failure must identify the missing source directory');
  if(!await evaluate("!!document.querySelector('[data-folders]') && !!document.querySelector('[data-config-unavailable] a[href=\"#/library\"]') && document.querySelector('[data-workspace-footer] button').disabled"))throw Error('Navigation, catalog and unknown deployment status must survive');
  if(await evaluate("!!document.querySelector('[data-host],[data-add-capability],.native-files [data-delete],.native-files [data-skill-delete],.native-files [data-takeover]') || !document.querySelector('.native-files [data-new]').hidden"))throw Error('Incomplete configuration must not offer writes');
  await screenshot(output+'-source-failure.png');
  await evaluate("document.querySelector('.native-files [data-view]')?.click()");
  await waitFor("!!document.querySelector('dialog:modal textarea[readonly]')");
  if(await evaluate("!!document.querySelector('dialog:modal [type=submit]')"))throw Error('File view must be read only');
  await screenshot(output+'-file-view.png');
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  await evaluate("document.querySelector('[data-retry]').click()");
  await waitFor("!!document.querySelector('[data-config-unavailable]') && !!document.querySelector('.native-files [data-list] article')");
  if(!await evaluate("window.sourceFailureWrites.length===0"))throw Error('Reading or retrying must not replay writes');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-source-mobile.png');
  if(await evaluate('document.documentElement.scrollWidth>innerWidth+1'))throw Error('Failure state overflows mobile viewport');
  await send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  // 换一个可用的团队来源：页面恢复，团队来源启用的资源列出来
  const replacement=process.argv[6];
  await evaluate("document.querySelector('[data-edit-team-source]').click()");
  await waitFor("!!document.querySelector('dialog:modal [data-source-save]') && !document.querySelector('dialog:modal [data-source-save]').disabled");
  await evaluate(`{document.querySelector('dialog:modal [data-source-existing]').click();const i=document.querySelector('dialog:modal [data-source-directory]');i.value=${JSON.stringify(replacement)};i.dispatchEvent(new Event('input'));}`);
  await evaluate("document.querySelector('dialog:modal form').requestSubmit()");
  await waitFor("!document.querySelector('dialog:modal') && !document.querySelector('[data-config-unavailable]') && !document.querySelector('.workspace-loading')");
  if(!await evaluate("[...document.querySelectorAll('[data-capability-list] .workspace-resource')].some(r=>r.textContent.includes('common-greet')&&r.querySelector('[data-resource-details]').title.includes('团队来源'))"))throw Error('Team resources must be listed after rebinding');
  await screenshot(output+'-source-rebound.png');
}
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
  await waitFor("!!document.querySelector('[data-library-sort]')");
  await evaluate("document.querySelector('[data-library-sort]').value='repository';document.querySelector('[data-library-sort]').dispatchEvent(new Event('change'))");
  // 同一仓库的多个来源合并为一个来源标签，按资源去重展示。
  // 只统计 mock 来源的条目：本机个人副本也会出现在同一目录里。
  const mocked = "[...document.querySelectorAll('[data-repository$=\"example/tools\"] .repository-resource')]";
  await waitFor(`${mocked}.length===6`);
  // 同名仓库分属两个平台：标签带平台名，两组可区分
  if (await evaluate("new Set([...document.querySelectorAll('[data-repository$=\"example/tools\"] h3')].filter(e=>e.parentElement.parentElement.classList.contains('library-repository-heading')).map(e=>e.textContent)).size") !== 2) throw new Error('Same-named repositories on different hosts must be distinguishable');
  await screenshot(output + '-grouped-desktop.png');
  await evaluate("document.querySelector('[data-search]').value='code-review'; document.querySelector('[data-search]').dispatchEvent(new Event('input'))");
  if (!await evaluate(`${mocked}.length===1`)) throw new Error('Resource search failed');
  await evaluate("document.querySelector('[data-search]').value=''; document.querySelector('[data-search]').dispatchEvent(new Event('input'))");
  await send('Emulation.setDeviceMetricsOverride', {width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output + '-grouped-mobile.png');
  if (await evaluate('document.documentElement.scrollWidth > innerWidth + 1')) throw new Error('Grouped library mobile overflow');
  await send('Emulation.setDeviceMetricsOverride', {width:1440,height:1000,deviceScaleFactor:1,mobile:false});
}
if (mode === 'compact-library') {
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');
    window.pickerWrites=[];window.groupChecks=[];
    const repo='https://github.com/example/skills.git';
    const sources=[{id:'fixture-a',name:'团队 Skills',url:repo,management:'managed',lock:{ref:'main'},resources:[{id:'fixture/skill/alpha',kind:'skill',name:'alpha',description:'自动检查代码风格，定位常见问题。'}]},
      {id:'fixture-b',name:'迁入的工具集',url:'git@github.com:example/skills',management:'external',lock:{ref:'main'},resources:[{id:'fixture/skill/delta',kind:'skill',name:'delta',description:'生成清楚易读的接口文档。'}]},
      {id:'fixture-c',name:'另一个平台',url:'https://gitlab.com/example/skills.git',management:'external',resources:[{id:'fixture/skill/omega',kind:'skill',name:'omega',description:'在另一台 Git 平台管理的技能。'}]}];
    const entries=[{id:'personal/skill/local',kind:'skill',name:'local-review',description:'保留你自己的代码审查习惯。',source:{repo_url:repo,ref:'main'},can_check_update:true,update:{state:'up-to-date'}},{id:'personal/rule/local',kind:'rule',name:'项目约定',description:'项目的代码规范和沟通约定。'}];
    api.collections=async()=>({sources});api.libraryList=async()=>({entries});
    api.collectionCheck=async id=>{window.groupChecks.push(id);return {sources};};
    api.checkUpdate=async name=>{window.groupChecks.push(name);return {status:{state:'up-to-date'}};};
    const skills=[{id:'skills/alpha',name:'alpha',description:'已经在资源库中的技能。',repo_path:'skills/alpha',installed:true},
      {id:'skills/beta',name:'beta',description:'检查网页布局、按钮和关键流程。',repo_path:'skills/beta',installed:false},
      {id:'packages/gamma',name:'gamma',description:'整理 Git 仓库里的变更并生成摘要。',repo_path:'packages/gamma',installed:false},
      {id:'skills/local',name:'local-review',description:'同名本地内容会保留。',repo_path:'skills/local',installed:false,conflict:'资源库已有同名 Skill，请先处理名称冲突'}];
    api.gitSkillsPreview=async(url,ref)=>({preview_id:'browser-fixture',url,ref,skills:structuredClone(skills)});
    api.gitSkillsInstall=async(token,selected)=>{
      window.pickerWrites.push({token,selected});
      const imported=selected.filter(id=>window.pickerWrites.length>1||id!=='packages/gamma').map(id=>({id,name:skills.find(s=>s.id===id).name}));
      for(const item of imported)entries.push({id:'personal/skill/'+item.name,kind:'skill',name:item.name,description:skills.find(s=>s.id===item.id).description,source:{repo_url:repo,ref:'main'},can_check_update:true,update:{state:'up-to-date'}});
      return {imported,failed:selected.filter(id=>!imported.some(s=>s.id===id)).map(id=>({id,name:'gamma',error:'模拟连接中断，请重试'}))};
    };
    location.hash='#/library';
  })()`);
  await waitFor("document.querySelectorAll('[data-repository]').length===3 && document.querySelectorAll('[data-library-id]').length===5");
  if(!await evaluate("{const selectors=['[data-search]','[data-library-kind]','[data-library-sort]'];const fields=selectors.map(s=>{const el=document.querySelector(s);return el.tagName==='SELECT'?el.parentElement.querySelector('[role=combobox]'):el;});new Set(fields.map(el=>Math.round(el.getBoundingClientRect().top))).size===1 && fields.every(el=>el.closest('.toolbar')===fields[0].closest('.toolbar'));}"))throw Error('Desktop filters must share a single toolbar row');
  if(!await evaluate("document.querySelector('[data-check]').getClientRects().length>0 && !document.querySelector('[data-check]').closest('details') && document.querySelector('[data-repository=\"github.com/example/skills\"] [data-group-install]')"))throw Error('Common update/install actions must be outside disclosure');
  await evaluate("document.querySelector('[data-repository=\"github.com/example/skills\"] [data-group-check]').click()");
  await waitFor("window.groupChecks.length===2 && !document.querySelector('[data-check]').disabled");
  if(!await evaluate("window.groupChecks.includes('fixture-a') && window.groupChecks.includes('local-review') && !window.groupChecks.includes('fixture-b')"))throw Error('Group update must respect external management');
  await screenshot(output+'-groups-desktop.png');
  await chooseRepository('github.com/example/skills');
  if(!await evaluate("document.querySelectorAll('[data-repository]').length===1 && document.querySelector('[data-library-clear]').getBoundingClientRect().top===document.querySelector('[data-search]').getBoundingClientRect().top"))throw Error('Active desktop filters must remain together');
  await screenshot(output+'-combined-desktop.png');
  await send('Emulation.setDeviceMetricsOverride',{width:1280,height:900,deviceScaleFactor:1,mobile:false});
  if(!await evaluate("new Set(['[data-search]','[data-library-kind]','[data-library-sort]'].map(s=>{const el=document.querySelector(s);return Math.round((el.tagName==='SELECT'?el.parentElement.querySelector('[role=combobox]'):el).getBoundingClientRect().top);})).size===1 && document.documentElement.scrollWidth<=innerWidth+1"))throw Error('1280px desktop filters must fit one row');
  await screenshot(output+'-combined-1280.png');
  await send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  await evaluate("document.querySelector('[data-library-clear]').click()");
  await evaluate("document.querySelector('[data-catalog-source=\"github.com/example/skills\"]').click()");
  await waitFor("document.querySelectorAll('dialog:modal [data-source-choice]').length===2");
  await evaluate("document.querySelector('[data-source-choice=\"fixture-b\"]').click()");
  await waitFor("document.querySelector('dialog:modal')?.textContent.includes('迁入的工具集')");
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  await evaluate("document.querySelector('[data-library-sort]').value='name';document.querySelector('[data-library-sort]').dispatchEvent(new Event('change'));document.querySelector('[data-manage-sources]').click()");
  await waitFor("document.querySelectorAll('dialog:modal [data-source-choice]').length===3");
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click();document.querySelector('[data-library-sort]').value='repository';document.querySelector('[data-library-sort]').dispatchEvent(new Event('change'));document.querySelector('[data-repository=\"github.com/example/skills\"] [data-group-install]').click()");
  await waitFor("document.querySelectorAll('dialog:modal [data-git-item]').length===4 && !document.querySelector('[data-git-scan]').disabled");
  if(!await evaluate("document.querySelectorAll('[data-git-item]:disabled').length===2 && document.querySelectorAll('[data-git-item]:checked').length===0 && document.querySelector('[data-git-install]').disabled"))throw Error('Installed/conflicted items must be disabled and install must require selection');
  await screenshot(output+'-picker-desktop.png');
  await evaluate("document.querySelector('[data-git-cancel]').click()");
  if(!await evaluate("window.pickerWrites.length===0 && !document.querySelector('dialog:modal')"))throw Error('Cancel must perform no installation');
  await evaluate("document.querySelector('[data-repository=\"github.com/example/skills\"] [data-group-install]').click()");
  await waitFor("document.querySelectorAll('[data-git-item]').length===4 && !document.querySelector('[data-git-scan]').disabled");
  await evaluate("document.querySelector('[data-git-search]').value='beta';document.querySelector('[data-git-search]').dispatchEvent(new Event('input'));document.querySelector('[data-git-all]').click()");
  if(!await evaluate("document.querySelectorAll('[data-git-item]:checked').length===1 && document.querySelector('[data-git-item]:checked').value==='skills/beta'"))throw Error('Select all must apply only to filtered available skills');
  await evaluate("document.querySelector('[data-git-search]').value='';document.querySelector('[data-git-search]').dispatchEvent(new Event('input'));document.querySelector('[data-git-item][value=\"packages/gamma\"]').click();document.querySelector('[data-git-install]').click()");
  await waitFor("document.querySelector('[data-git-message]').textContent.includes('1 个失败') && !document.querySelector('[data-git-install]').disabled");
  if(!await evaluate("window.pickerWrites.length===1 && window.pickerWrites[0].selected.length===2 && document.querySelectorAll('[data-git-item]:checked').length===1 && document.querySelector('[data-git-item]:checked').value==='packages/gamma' && document.querySelector('[data-git-item][value=\"skills/beta\"]').disabled"))throw Error('Partial success must retain only failed selections');
  await screenshot(output+'-partial-failure.png');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-picker-mobile.png');
  if(await evaluate("document.documentElement.scrollWidth>innerWidth+1 || document.querySelector('dialog:modal').scrollWidth>document.querySelector('dialog:modal').clientWidth+1"))throw Error('Picker mobile overflow');
  await evaluate("document.querySelector('[data-git-install]').click()");
  await waitFor("!document.querySelector('dialog:modal') && document.querySelectorAll('[data-library-id]').length===7");
  if(!await evaluate("window.pickerWrites.length===2 && window.pickerWrites[1].selected.join(',')==='packages/gamma'"))throw Error('Retry must not reinstall successful items');
  await screenshot(output+'-groups-mobile.png');
  if(await evaluate('document.documentElement.scrollWidth>innerWidth+1'))throw Error('Compact library mobile overflow');
  // 写入成功但列表刷新失败，不能说成安装失败；应用中 Escape 不能取消。
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');const {GitSkillDialog}=await import('/ui/features/gitSkillDialog.js');
    const slot=document.createElement('div');document.querySelector('#app').append(slot);window.pickerTestSlot=slot;
    api.gitSkillsPreview=async()=>({preview_id:'refresh-test',skills:[{id:'fixture',name:'fixture',description:'成功安装后模拟列表刷新失败。',repo_path:'fixture',installed:false}]});
    api.gitSkillsInstall=()=>new Promise(resolve=>window.finishPickerInstall=()=>resolve({imported:[{id:'fixture',name:'fixture'}],failed:[]}));
    window.pickerTest=GitSkillDialog(slot,{onChanged:()=>{throw Error('模拟刷新连接中断');}});window.pickerTest.show({url:'https://example.test/skills'});
  })()`);
  await waitFor("document.querySelectorAll('dialog:modal [data-git-item]').length===1 && !document.querySelector('dialog:modal [data-git-scan]').disabled");
  await evaluate("document.querySelector('dialog:modal [data-git-item]').click();document.querySelector('dialog:modal [data-git-install]').click()");
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await send('Input.dispatchKeyEvent',{type:'keyUp',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  if(!await evaluate("!!document.querySelector('dialog:modal') && document.querySelector('dialog:modal [data-git-cancel]').disabled && document.querySelector('dialog:modal .dialog-dismiss').disabled"))throw Error('In-flight install must not be dismissed');
  await evaluate("window.finishPickerInstall()");
  await waitFor("document.querySelector('dialog:modal [data-git-message]').textContent.includes('列表刷新失败') && !document.querySelector('dialog:modal [data-git-cancel]').disabled");
  if(!await evaluate("document.querySelector('dialog:modal [data-git-message]').textContent.startsWith('已安装 1 个。') && document.querySelector('dialog:modal [data-git-item]').disabled && document.querySelector('dialog:modal [data-git-install]').disabled"))throw Error('Refresh failure must preserve truthful install status');
  await evaluate("window.pickerTest.destroy();window.pickerTestSlot.remove()");
  // 扫描尚未返回时关闭，再打开空弹窗，旧结果不能进入新一次选择。
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');const {GitSkillDialog}=await import('/ui/features/gitSkillDialog.js');
    const slot=document.createElement('div');document.querySelector('#app').append(slot);window.pickerTestSlot=slot;
    api.gitSkillsPreview=()=>new Promise(resolve=>window.finishPickerScan=()=>resolve({preview_id:'stale',skills:[{id:'stale',name:'stale',repo_path:'stale',installed:false}]}));
    window.pickerTest=GitSkillDialog(slot);window.pickerTest.show({url:'https://example.test/skills'});
  })()`);
  await waitFor("!!window.finishPickerScan && document.querySelector('dialog:modal [data-git-scan]').disabled");
  await evaluate("document.querySelector('dialog:modal [data-git-cancel]').click();window.pickerTest.show();window.finishPickerScan()");
  if(!await evaluate("document.querySelectorAll('dialog:modal [data-git-item]').length===0 && !document.querySelector('dialog:modal [data-git-scan]').disabled"))throw Error('Cancelled scan result must be discarded');
  await evaluate("window.pickerTest.destroy();window.pickerTestSlot.remove()");
  await send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
}
if (mode === 'cc-switch') {
  await evaluate("location.hash='#/library'");
  await waitFor("!!document.querySelector('[data-cc-switch]')");
  // 按钮在「更新与迁移」折叠区里：像用户一样先展开，否则折叠内容不可聚焦，焦点无从恢复
  await evaluate("{const b=document.querySelector('[data-cc-switch]'); const d=b.closest('details'); if(d) d.open=true; b.focus(); b.click();}");
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
  if (!await evaluate("document.activeElement===document.querySelector('.action-menu-trigger')")) throw new Error('Migration focus not restored');
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
  await waitFor("!document.querySelector('dialog:modal') && /^#\\/projects\\/.+/.test(location.hash) && document.querySelector('#page-breadcrumb').textContent.includes('浏览器验收项目')");
  await waitFor("!!document.querySelector('[data-tool-change]')");
  await configureTool('claude');
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
    // Filters sit in the toolbar row (v39); there is no disclosure to open.
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
if (mode === 'instructions') {
  // 项目说明的并发保护：别处先保存后，本页保存出现冲突确认；取消则载入最新内容，不覆盖别处的保存。
  const [project] = process.argv.slice(5);
  const api = body => `(async()=>{const {api}=await import('/ui/services/api.js');return api.instructions(${JSON.stringify(body)});})()`;
  const repoId = await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');await api.approveDir(${JSON.stringify(project)});return (await api.repoDiscover(${JSON.stringify(project)})).repo_id;})()`);
  await evaluate(`location.hash='#/projects/${repoId}/instructions'`);
  await waitFor("!!document.querySelector('[data-save]') && !document.querySelector('[data-save]').disabled");
  await evaluate(api({ root: project, content: '别处保存的说明' }));
  await evaluate("{const t=document.querySelector('[data-editor] textarea'); t.value='本页的说明'; t.dispatchEvent(new Event('input')); document.querySelector('[data-save]').click()}");
  await waitFor("document.querySelector('dialog:modal')?.innerText.includes('已在其他页面被修改')");
  await screenshot(output + '-instructions-conflict.png');
  await evaluate("[...document.querySelectorAll('dialog:modal button')].find(b=>b.textContent.trim()==='取消').click()");
  await waitFor("document.querySelector('[data-editor] textarea').value==='别处保存的说明' && document.querySelector('[data-msg]').textContent.includes('已载入最新')");
  if ((await evaluate(api({ root: project, read: true }))).content !== '别处保存的说明') throw new Error('Concurrent save was overwritten');
  await evaluate("{const t=document.querySelector('[data-editor] textarea'); t.value='合并后的说明'; t.dispatchEvent(new Event('input')); document.querySelector('[data-save]').click()}");
  await waitFor("document.querySelector('[data-msg]').textContent.includes('已保存')");
  if ((await evaluate(api({ root: project, read: true }))).content !== '合并后的说明') throw new Error('Save on latest revision failed');
}
if(mode==='global-targets') {
  // 只用于 global_skill_apply 的隔离服务，单张截图检查新控件布局。
  const id='personal/skill/personal/alpha';
  const target=(key,adding=false)=>`[${adding?'data-global-add-target-skill':'data-global-target-skill'}="${id}"][data-target-key="${key}"]`;
  const status=()=>evaluate("(async()=>{const {api}=await import('/ui/services/api.js');return api.globalSkills();})()");
  await evaluate("location.hash='#/native-files'");
  await waitFor("!!document.querySelector('[data-add-skill]') && !document.querySelector('[data-add-skill]').disabled");
  const before=await status();
  if(!before.home.includes('isolated')||before.skills.length!==3)throw Error('Target test requires its isolated fixture');
  if(await evaluate("!!document.querySelector('[data-target]')"))throw Error('Shared master switches must be replaced by per-Skill choices');
  await evaluate("document.querySelector('[data-add-skill]').click()");
  await waitFor("document.querySelectorAll('[data-global-add-id]').length===3");
  await evaluate(`document.querySelector(${JSON.stringify(target('agents',true))}).click()`);
  await evaluate("document.querySelector('[data-global-add-search]').value='beta';document.querySelector('[data-global-add-search]').dispatchEvent(new Event('input'));document.querySelector('[data-global-add-search]').value='';document.querySelector('[data-global-add-search]').dispatchEvent(new Event('input'))");
  if(!await evaluate(`document.querySelector(${JSON.stringify(target('claude',true))}).getAttribute('aria-pressed')==='true' && document.querySelector(${JSON.stringify(target('agents',true))}).getAttribute('aria-pressed')==='false'`))throw Error('Filter changed per-Skill target draft');
  await screenshot(output+'-picker.png');
  await evaluate(`document.querySelector('[data-global-add-id="${id}"]').click();document.querySelector('[data-global-add-submit]').click()`);
  await waitFor(`document.querySelector('[data-global-skill="${id}"]')?.textContent.includes('已启用') && !document.querySelector('[data-refresh]').disabled`);
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  let s=await status();
  const paths=s=>s.deployed.filter(d=>d.resource_id===id).map(d=>d.path.split('#')[0]).sort();
  if(paths(s).join(',')!=='.claude/skills/alpha'||s.deployed.some(d=>d.resource_id.endsWith('/beta')))throw Error('Adding to Claude touched another target or unrelated Skill');
  await evaluate(`document.querySelector(${JSON.stringify(target('agents'))}).click()`);
  await waitFor(`!document.querySelector('[data-refresh]').disabled && document.querySelector(${JSON.stringify(target('agents'))})?.getAttribute('aria-pressed')==='true'`);
  if(paths(await status()).length!==2)throw Error('Adding a second target did not deploy');
  await evaluate(`document.querySelector(${JSON.stringify(target('claude'))}).click()`);
  await waitFor(`!document.querySelector('[data-refresh]').disabled && document.querySelector(${JSON.stringify(target('claude'))})?.getAttribute('aria-pressed')==='false'`);
  s=await status();
  if(paths(s).join(',')!=='.agents/skills/alpha')throw Error('Switching target did not remove the old managed link');
  await evaluate('window.globalTargetReloadMarker=true');
  await send('Page.reload');
  await waitFor(`!window.globalTargetReloadMarker && document.querySelector(${JSON.stringify(target('agents'))})?.getAttribute('aria-pressed')==='true' && !document.querySelector('[data-refresh]').disabled`);
  if(await evaluate(`document.querySelector(${JSON.stringify(target('claude'))}).getAttribute('aria-pressed')==='true'`))throw Error('Target selection did not survive reload');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  if(await evaluate('document.documentElement.scrollWidth>innerWidth+1'))throw Error('Per-Skill target controls overflow on mobile');
  await evaluate("document.querySelector('[data-add-skill]').click()");
  await waitFor("!!document.querySelector('dialog:modal')");
  if(await evaluate("document.querySelector('dialog:modal').scrollWidth>document.querySelector('dialog:modal').clientWidth+1"))throw Error('Target picker overflows on mobile');
  if(!await evaluate(`document.querySelector(${JSON.stringify(target('agents',true))}).disabled`))throw Error('Added Skill must be edited on main list');
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  await evaluate(`document.querySelector(${JSON.stringify(target('agents'))}).click()`);
  await waitFor(`!document.querySelector('[data-global-skill="${id}"]') && !document.querySelector('[data-refresh]').disabled`);
  s=await status();
  if(paths(s).length||!s.skills.some(s=>s.id===id&&!s.global)||s.deployed.some(d=>d.resource_id.endsWith('/beta')))throw Error('Last target removal did not preserve source or isolated unrelated Skills');
  if(JSON.stringify(before.deployed.filter(d=>d.resource_id.endsWith('/gamma')))!==JSON.stringify(s.deployed.filter(d=>d.resource_id.endsWith('/gamma'))))throw Error('Editing alpha affected gamma');
  console.log(JSON.stringify({checks:'independent targets, filter draft, target switching, reload, last-target removal, mobile layout'}));
}
if(mode==='global-targets-installed') {
  // 对已安装服务只读检查，不改变用户的选择或文件。
  await evaluate("location.hash='#/native-files'");
  await waitFor("!!document.querySelector('[data-global-count]')?.textContent && !document.querySelector('[data-add-skill]').disabled");
  if(await evaluate("!!document.querySelector('[data-target]')"))throw Error('Installed UI still has shared master switches');
  const count=await evaluate("(async()=>{const {api}=await import('/ui/services/api.js');const s=await api.globalSkills();if(!s.skill_targets||!s.skills.every(s=>Array.isArray(s.available_targets)))throw Error('Installed API lacks per-Skill target data');return s.skills.length;})()");
  await evaluate("document.querySelector('[data-add-skill]').click()");
  await waitFor("!!document.querySelector('dialog:modal [data-global-add-target-skill]')");
  if(await evaluate("document.querySelectorAll('[data-global-add-id]').length")!==count)throw Error('Installed target picker omitted candidates');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  if(await evaluate("document.documentElement.scrollWidth>innerWidth+1 || document.querySelector('dialog:modal').scrollWidth>document.querySelector('dialog:modal').clientWidth+1"))throw Error('Installed picker mobile overflow');
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  console.log(JSON.stringify({checks:'installed API, per-Skill target controls, complete catalog, mobile layout',candidates:count}));
}
if(mode==='global-search') {
  // 仅连接 global_skill_apply 的隔离子进程，真实执行单项添加/移除。
  const id='personal/skill/personal/alpha';
  await evaluate("location.hash='#/native-files'");
  await waitFor("!!document.querySelector('[data-add-skill]') && !document.querySelector('[data-add-skill]').disabled");
  if(await evaluate(`!!document.querySelector('[data-global-skill="${id}"]')`))throw Error('Unselected library Skills must not appear in global main list');
  await screenshot(output+'-global-before.png');
  await evaluate("document.querySelector('[data-add-skill]').click()");
  await waitFor("!!document.querySelector('dialog:modal [data-global-add-search]')");
  if(await evaluate("document.querySelectorAll('[data-global-add-id]').length")!==3)throw Error('Opening global picker must list every library candidate');
  if(!await evaluate("document.querySelector('dialog:modal').getBoundingClientRect().width>=1000 && !!document.querySelector('[data-global-add-group]') && document.querySelectorAll('[data-global-add-state]').length===3"))throw Error('Global picker must be enlarged and have repository/status filters');
  await screenshot(output+'-browse-desktop.png');
  await evaluate("[...document.querySelectorAll('[data-global-add-group]')].find(b=>b.dataset.globalAddGroup==='github.com/example/catalog').click()");
  if(await evaluate("document.querySelectorAll('[data-global-add-id]').length")!==2)throw Error('Repository filter must group HTTPS and SSH candidates');
  await evaluate("document.querySelector('[data-global-add-group=\"\"]').click()");
  await evaluate("document.querySelector('[data-global-add-state=available]').click()");
  if(await evaluate("document.querySelectorAll('[data-global-add-id]').length")!==1)throw Error('Available filter must exclude added candidates');
  await evaluate("document.querySelector('[data-global-add-state=all]').click()");
  await evaluate("document.querySelector('[data-global-add-search]').value='alpha';document.querySelector('[data-global-add-search]').dispatchEvent(new Event('input'))");
  if(await evaluate("document.querySelectorAll('[data-global-add-id]').length")!==1)throw Error('Global picker search must narrow candidates');
  await screenshot(output+'-add-desktop.png');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-add-mobile.png');
  if(await evaluate("document.documentElement.scrollWidth>innerWidth+1 || document.querySelector('dialog:modal').scrollWidth>document.querySelector('dialog:modal').clientWidth+1"))throw Error('Global search picker mobile overflow');
  await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');const apply=api.globalApply;window.originalGlobalApply=apply;window.globalApplyCalls=[];api.globalApply=skill=>{window.globalApplyCalls.push(skill);return new Promise((resolve,reject)=>{window.releaseGlobalApply=()=>apply(skill).then(resolve,reject);});};document.querySelector('[data-global-add-id="${id}"]').click();document.querySelector('[data-global-add-submit]').click();})()`);
  await waitFor("typeof window.releaseGlobalApply==='function'");
  await send('Input.dispatchKeyEvent',{type:'keyDown',key:'Escape',code:'Escape',windowsVirtualKeyCode:27});
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  if(!await evaluate("!!document.querySelector('dialog:modal') && document.querySelector('[data-global-add-search]').disabled"))throw Error('Global add must guard closing and repeat clicks in flight');
  await evaluate("window.releaseGlobalApply()");
  await waitFor(`document.querySelector('[data-global-skill="${id}"]')?.textContent.includes('已启用') && !document.querySelector('[data-refresh]').disabled`);
  if(!await evaluate(`window.globalApplyCalls.length===1 && window.globalApplyCalls[0]===${JSON.stringify(id)} && document.querySelector('[data-global-add-id="${id}"]').disabled`))throw Error('Add must apply only one chosen Skill');
  const deployed=await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');const s=await api.globalSkills();return {alpha:s.deployed.filter(d=>d.resource_id===${JSON.stringify(id)}).length,beta:s.deployed.filter(d=>d.resource_id.endsWith('/beta')).length};})()`);
  if(deployed.alpha!==2||deployed.beta!==0)throw Error('Add applied pending unrelated Skill: '+JSON.stringify(deployed));
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  await screenshot(output+'-global-mobile.png');
  if(await evaluate('document.documentElement.scrollWidth>innerWidth+1'))throw Error('Global main mobile overflow');
  await send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-global-desktop.png');
  // 移除只取消全局部署，来源与候选保留；恢复原 API 避免等待包装影响移除。
  await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');api.globalApply=window.originalGlobalApply;document.querySelector('[data-toggle="${id}"]').click();})()`);
  await waitFor(`!document.querySelector('[data-global-skill="${id}"]') && !document.querySelector('[data-refresh]').disabled`);
  if(!await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');const s=await api.globalSkills();return !s.deployed.some(d=>d.resource_id===${JSON.stringify(id)}) && s.skills.some(s=>s.id===${JSON.stringify(id)}&&!s.global) && !s.deployed.some(d=>d.resource_id.endsWith('/beta'));})()`))throw Error('Remove must preserve library candidate and leave unrelated pending Skill unchanged');
  await screenshot(output+'-removed.png');
  // 注入一次列表读取失败；文件写入仍通过真实 API，刷新可恢复。
  await evaluate(`document.querySelector('[data-add-skill]').click();document.querySelector('[data-global-add-search]').value='alpha';document.querySelector('[data-global-add-search]').dispatchEvent(new Event('input'))`);
  await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');const read=api.globalSkills;api.globalSkills=async()=>{api.globalSkills=read;throw new Error('验收注入：列表连接中断');};document.querySelector('[data-global-add-id="${id}"]').click();document.querySelector('[data-global-add-submit]').click();})()`);
  await waitFor("document.querySelector('[data-global-add-error]')?.textContent.includes('操作已提交，但列表刷新失败')");
  if(!await evaluate("!document.querySelector('[data-refresh]').disabled && !document.querySelector('[data-add-skill]').disabled && !document.querySelector('.global-skills [data-search]').disabled"))throw Error('Read failure must restore main-page controls');
  await screenshot(output+'-refresh-failure.png');
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click();document.querySelector('[data-refresh]').click()");
  await waitFor(`document.querySelector('[data-global-skill="${id}"]')?.textContent.includes('已启用')`);
  await evaluate(`document.querySelector('[data-toggle="${id}"]').click()`);
  await waitFor(`!document.querySelector('[data-global-skill="${id}"]') && !document.querySelector('[data-refresh]').disabled`);
}
if (mode === 'global') {
  // AIL-152：全局配置页 —— 全局启用一个 Skill，预览并应用后确实部署到（临时）用户目录；规则与 Agent 栏仍可用。
  const [skillDir] = process.argv.slice(5);
  const id = 'personal/skill/personal/global-skill';
  await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');await api.approveDir(${JSON.stringify(skillDir)});await api.libraryImport(${JSON.stringify(skillDir)},null,true);})()`);
  await evaluate("location.hash='#/native-files'");
  await waitFor("!!document.querySelector('[data-add-skill]') && !document.querySelector('[data-add-skill]').disabled");
  if (!await evaluate("document.querySelector('nav.sidebar a[href=\"#/library/global\"]')?.getAttribute('aria-current')==='page' && !document.querySelector('.library-context')")) throw new Error('全局技能应使用独立的主导航入口');
  await evaluate("document.querySelector('[data-add-skill]').click();document.querySelector('[data-global-add-search]').value='global-skill';document.querySelector('[data-global-add-search]').dispatchEvent(new Event('input'))");
  await waitFor(`!!document.querySelector('[data-global-add-id="${id}"]')`);
  await screenshot(output + '-global-search.png');
  await evaluate(`document.querySelector('[data-global-add-id="${id}"]').click();document.querySelector('[data-global-add-submit]').click()`);
  await waitFor(`document.querySelector('[data-global-skill="${id}"]')?.textContent.includes('已启用') && !document.querySelector('[data-refresh]').disabled`);
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  await waitFor("!document.querySelector('dialog:modal') && [...document.querySelectorAll('.global-skills .workspace-resource-status')].some(s=>s.textContent==='已启用')");
  const deployed = await evaluate(`(async()=>{const {api}=await import('/ui/services/api.js');return (await api.globalSkills()).deployed.filter(d=>d.resource_id===${JSON.stringify(id)}).map(d=>d.path.split('#')[0]).sort();})()`);
  if (JSON.stringify(deployed) !== JSON.stringify(['.agents/skills/global-skill', 'claude/skills/global-skill'])) throw new Error('全局部署路径不符：' + JSON.stringify(deployed));
  // 每行最多两段路径：安装位置 → 实际位置；链接套链接时，中间经过的链接写在实际位置的提示里
  await evaluate("document.querySelectorAll('.global-skills details').forEach(d=>d.open=true)");
  const realCells = await evaluate("[...document.querySelectorAll('.global-location-row')].map(row=>row.querySelectorAll('.ui-item-path')[1]?.textContent||'').filter(Boolean)");
  if (realCells.length !== 2 || !realCells.every(c => c.includes('elsewhere/foreign-skill'))) throw new Error('非托管条目的真实目录未展示：' + JSON.stringify(realCells));
  if (!await evaluate("[...document.querySelectorAll('.global-location-row .ui-item-path')].some(p=>p.title.includes('经过'))")) throw new Error('链接套链接未展示中间经过');
  await screenshot(output + '-global.png');
  await send('Emulation.setDeviceMetricsOverride', {width:390,height:844,deviceScaleFactor:1,mobile:true});
  await screenshot(output + '-global-mobile.png');
  if (await evaluate('document.documentElement.scrollWidth > window.innerWidth + 1')) throw new Error('全局配置页窄屏出现横向滚动');
  await send('Emulation.setDeviceMetricsOverride', {width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  await evaluate("location.hash='#/library/rules'");
  await waitFor("!!document.querySelector('.native-files [data-new]')");
}
if (mode === 'onboarding') {
  // 三步引导页只做讲解与直达入口，不承载配置动作。
  for (const [index, target] of [[0, '#/projects'], [1, '#/library'], [2, '#/projects']]) {
    await evaluate("location.hash='#/onboarding'");
    await waitFor("document.querySelectorAll('.wizard .step').length === 3 && document.querySelectorAll('.wizard .step button').length === 3");
    if (index === 0) await screenshot(output + '-onboarding.png');
    await evaluate(`document.querySelectorAll('.wizard .step button')[${index}].click()`);
    await waitFor(`location.hash === ${JSON.stringify(target)}`);
  }
}
if (mode === 'library-view') {
  await evaluate("location.hash='#/library'");
  await waitFor("!!document.querySelector('[data-git-add]') && !!document.querySelector('[data-capability-catalog]')?.textContent.trim()");
  await screenshot(output+'-installed-desktop.png');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-installed-mobile.png');
  if(await evaluate('document.documentElement.scrollWidth>innerWidth+1'))throw Error('Installed library mobile overflow');
  const repository=await evaluate("document.querySelector('[data-repository-filter]:not([data-repository-filter=\"\"])')?.dataset.repositoryFilter");
  if(!await evaluate("!!document.querySelector('.library-repository-nav') && !document.querySelector('[data-library-category]')"))throw Error('Installed build must use repository sidebar without category dropdown');
  if(repository) {
    await chooseRepository(repository);
    if(!await evaluate("document.querySelectorAll('[data-repository]').length<=1 && document.querySelector('[data-check]').textContent==='检查筛选项'"))throw Error('Installed repository filter failed');
    await screenshot(output+'-filtered-mobile.png');
    await evaluate("document.querySelector('[data-library-clear]').click()");
  }
  await evaluate("location.hash='#/projects'");
  await waitFor("!!document.querySelector('[data-add-project]')");
  await screenshot(output+'-projects-mobile.png');
  await send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-projects-desktop.png');
}
if(mode==='global-view') {
  // 安装后的真实资料只读验收：查看列表与搜索弹窗，始终不点击添加/移除。
  await evaluate("location.hash='#/native-files'");
  await waitFor("!!document.querySelector('[data-global-count]')?.textContent");
  const expected=await evaluate("(async()=>{const {api}=await import('/ui/services/api.js');const s=await api.globalSkills();return new Set([...(s.selected||[]),...s.skills.filter(s=>s.global).map(s=>s.id),...s.deployed.map(d=>d.resource_id)]).size;})()");
  if(await evaluate("document.querySelectorAll('[data-global-skill]').length")!==expected)throw Error('Installed global list must contain only selected or deployed Skills');
  if(await evaluate("document.querySelector('.native-files-page .page-head')!==null"))throw Error('Global page repeats the active navigation heading');
  await screenshot(output+'-global-desktop.png');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-global-mobile.png');
  if(await evaluate('document.documentElement.scrollWidth>innerWidth+1'))throw Error('Installed global mobile overflow');
  if(!await evaluate("document.querySelector('[data-add-skill]').disabled")) {
    await evaluate("document.querySelector('[data-add-skill]').click()");
    await waitFor("!!document.querySelector('dialog:modal [data-global-add-search]')");
    const total=await evaluate("(async()=>{const {api}=await import('/ui/services/api.js');return (await api.globalSkills()).skills.length;})()");
    if(await evaluate("document.querySelectorAll('[data-global-add-id]').length")!==total)throw Error('Installed global empty search must list all Skills');
    await screenshot(output+'-browse-mobile.png');
    if(await evaluate("document.documentElement.scrollWidth>innerWidth+1 || document.querySelector('dialog:modal').scrollWidth>document.querySelector('dialog:modal').clientWidth+1"))throw Error('Installed browse catalog mobile overflow');
    const key=await evaluate("document.querySelector('[data-global-add-group]:not([data-global-add-group=\"\"])')?.dataset.globalAddGroup");
    if(key) {
      await evaluate(`{const b=[...document.querySelectorAll('[data-global-add-group]')].find(b=>b.dataset.globalAddGroup===${JSON.stringify(key)});b.click();}`);
      if(!await evaluate("document.querySelectorAll('[data-global-add-list]>section').length===1 && document.activeElement.hasAttribute('data-global-add-group')"))throw Error('Installed repository filter must limit catalog and retain keyboard focus');
      await screenshot(output+'-filtered-browse-mobile.png');
    }
    await send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false});
    await evaluate("document.querySelector('[data-global-add-group=\"\"]').click()");
    await screenshot(output+'-browse-desktop.png');
    if(!await evaluate("document.querySelector('dialog:modal').getBoundingClientRect().width>=1000 && document.querySelector('[data-global-add-list]').scrollHeight>document.querySelector('[data-global-add-list]').clientHeight"))throw Error('Installed large catalog must scroll inside the enlarged dialog');
    await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  }
}
if (mode === 'git-install') {
  const repo=process.argv[5];
  if(!repo)throw Error('git-install requires an isolated fixture repository');
  await evaluate("location.hash='#/library'");
  await waitFor("!!document.querySelector('[data-git-add]')");
  const initial=await evaluate("(async()=>{const {api}=await import('/ui/services/api.js');return (await api.libraryList()).entries.length;})()");
  if(initial!==0)throw Error('Real install test requires an empty isolated library');
  await evaluate(`document.querySelector('[data-git-add]').click();document.querySelector('[data-git-url]').value=${JSON.stringify(repo)};document.querySelector('[data-git-form]').requestSubmit()`);
  await waitFor("document.querySelectorAll('[data-git-item]').length===2 && !document.querySelector('[data-git-scan]').disabled");
  await screenshot(output+'-real-scan.png');
  await evaluate("document.querySelector('[data-git-cancel]').click()");
  if(await evaluate("(async()=>{const {api}=await import('/ui/services/api.js');return (await api.libraryList()).entries.length;})()")!==0)throw Error('Real cancel wrote library entries');
  await evaluate(`document.querySelector('[data-git-add]').click();document.querySelector('[data-git-url]').value=${JSON.stringify(repo)};document.querySelector('[data-git-form]').requestSubmit()`);
  await waitFor("document.querySelectorAll('[data-git-item]').length===2 && !document.querySelector('[data-git-scan]').disabled");
  await evaluate("document.querySelector('[data-git-item][value=\"tools/nested/skills/fixture-beta\"]').click();document.querySelector('[data-git-install]').click()");
  await waitFor("!document.querySelector('dialog:modal') && document.querySelectorAll('[data-library-id]').length===1");
  if(!await evaluate("document.querySelector('[data-library-id]').textContent.includes('fixture-beta') && !document.querySelector('[data-library-id]').textContent.includes('fixture-alpha')"))throw Error('Only the chosen nested Skill must install');
  await evaluate("document.querySelector('[data-group-check]').click()");
  await waitFor("document.querySelector('[data-library-id]').textContent.includes('已是最新') && !document.querySelector('[data-check]').disabled");
  await screenshot(output+'-real-installed.png');
  await evaluate("document.querySelector('[data-group-install]').click()");
  await waitFor("document.querySelectorAll('[data-git-item]').length===2 && !document.querySelector('[data-git-scan]').disabled");
  if(!await evaluate("document.querySelector('[data-git-item][value=\"tools/nested/skills/fixture-beta\"]').disabled && !document.querySelector('[data-git-item][value=\"skills/fixture-alpha\"]').disabled"))throw Error('Real rescan must offer only the remaining uninstalled Skill');
  await screenshot(output+'-real-add-more.png');
  await evaluate("document.querySelector('[data-git-cancel]').click()");
}
if (!['knowledge-ingestion','knowledge-organization','worktree-cleanup','v27-flow','v27','journey','before','compact-library','git-install','library-view','global-view','report','repository-filters','global-search','global-targets','global-targets-installed'].includes(mode)) {
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
    await waitFor(`!document.querySelector('dialog:modal') && [...document.querySelectorAll('.repository-resource .ui-item-title')].some(h=>h.textContent.includes(${JSON.stringify(skillName)}))`);
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
    small:[...document.querySelectorAll('#app button,#app label,#app .badge')].filter(e=>parseFloat(getComputedStyle(e).fontSize)<12).length,
    // List-row actions are compact on desktop (v38: 28px, 44 on phones); every other button keeps 32.
    short:[...document.querySelectorAll('#app button')].filter(e=>{const h=e.getBoundingClientRect().height;return h>0&&h<(e.closest('.ui-item-actions')?28:32);}).length
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
  await evaluate("document.querySelector('dialog:modal .dialog-actions button')?.click()");
  await waitFor("window.confirmResult === false && !document.querySelector('dialog:modal')");
  await send('Emulation.setDeviceMetricsOverride', {width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output + '-components-mobile.png');
  if (await evaluate('document.documentElement.scrollWidth>innerWidth+1')) throw new Error('Component sample overflow');
}
console.log(JSON.stringify({ title: await evaluate('document.title'), errors, screenshots:mode==='journey'?null:output }));
socket.close();
await fetch(`http://127.0.0.1:${CDP}/json/close/` + tab.id);
if (errors.length) process.exitCode = 1;
