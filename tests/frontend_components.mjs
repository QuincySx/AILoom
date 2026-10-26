// Node 内置测试；这里只验证组件状态，不冒充真实浏览器验收。
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import path from 'node:path';

// icons.js has no dependencies; tests load the real module wherever a component imports it.
async function iconsModule(context) {
  const module=new vm.SourceTextModule(await readFile('src/console/ui/components/icons.js','utf8'),{context});
  await module.link(()=>{throw new Error('icons.js has no dependencies');});
  return module;
}

async function sharedComponents(context) {
  const module=new vm.SourceTextModule(await readFile('src/console/ui/components/item.js','utf8'),{context});
  await module.link(name=>name.endsWith('/icons.js')?iconsModule(context):new vm.SyntheticModule(['esc'],function(){this.setExport('esc',value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c])));},{context}));
  await module.evaluate();return Object.fromEntries(Object.entries(module.namespace));
}

async function targetHelpers(context) {
  const module=new vm.SourceTextModule(await readFile('src/console/ui/state/target.js','utf8'),{context});
  await module.link(()=>new vm.SyntheticModule(['get','set'],function(){this.setExport('get',()=>null);this.setExport('set',()=>{});},{context}));
  await module.evaluate();return Object.fromEntries(Object.entries(module.namespace));
}

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.value = ''; this.style = {}; this.classList = { add() {}, remove() {},toggle(){return true;} }; }
  append(...nodes) { this.children.push(...nodes); }
  appendChild(node) { this.append(node); }
  setAttribute(name, value) { (this.attributes ??= {})[name] = value; }
  remove() { this.removed = true; }
}

test('团队来源恢复：读取当前路径，失败保留输入，成功绑定后才刷新关闭', async () => {
  let form, dialog, readSource, rejectSave=true, refreshed=0, failure=new Error('新目录不是有效的团队资源源');
  const writes=[], approvals=[];
  const document={createElement:tag=>{
    const nodes=new Map();
    form=new Element(tag);
    form.querySelector=selector=>{if(!nodes.has(selector))nodes.set(selector,new Element('input'));return nodes.get(selector);};
    return form;
  }};
  const context=vm.createContext({document});
  const api={
    projectSource:body=>{
      if(body.action==='read')return new Promise(resolve=>{readSource=resolve;});
      writes.push(body);
      return rejectSave?Promise.reject(failure):Promise.resolve({});
    },
    approveDir:async path=>{approvals.push(path);},
    pickDirectory:async purpose=>({path:purpose==='resource-storage'?'/custom/downloads':'/new/team'}),
  };
  const Dialog=(_container,props)=>{
    dialog={props,closed:false,update(){},close(){this.closed=true;props.onClose();},destroy(){}};
    return dialog;
  };
  const mod=new vm.SourceTextModule(await readFile('src/console/ui/features/teamSourceDialog.js','utf8'),{context});
  await mod.link(name=>new vm.SyntheticModule([name.includes('api.js')?'api':'Dialog'],function(){this.setExport(name.includes('api.js')?'api':'Dialog',name.includes('api.js')?api:Dialog);},{context}));
  await mod.evaluate();
  mod.namespace.TeamSourceDialog({}, {rootPath:'/project/sub',onSaved:async()=>{refreshed++;}});
  assert.equal(dialog.props.canClose(),false);
  readSource({root:'/project',cache_root:'/ailoom/data/cache',default_cache_root:'/ailoom/data/cache',base_fingerprint:'original',source:{type:'git',url:'/missing/team',ref:'old-branch'}});
  await new Promise(setImmediate);
  const q=selector=>form.querySelector(selector);
  assert.equal(q('[data-source-directory]').value,'/missing/team');
  assert.equal(q('[data-source-existing]').checked,true,'本机 Git 路径应该预选已有文件夹');
  assert.equal(q('[data-source-remote]').disabled,true,'隐藏的 Git 字段不能阻止本地表单提交');
  assert.equal(dialog.props.canClose(),true);
  await q('[data-source-pick]').onclick();
  assert.equal(q('[data-source-directory]').value,'/new/team');
  assert.equal(q('[data-source-storage]').textContent,'/new/team');
  q('[data-source-git]').onchange();
  assert.equal(q('[data-source-cache-root]').value,'/ailoom/data/cache');
  assert.equal(q('[data-source-cache-default]').hidden,true);
  assert.equal(q('[data-source-local]').disabled,true);
  q('[data-source-url]').value='git@quincy-github:owner/team.git';
  q('[data-source-url]').oninput();
  failure=Object.assign(new Error('首次获取资源源失败（clone）'),{data:{code:'E2002',context:{cause:{context:{stderr:'git@github.com: Permission denied (publickey).'}}}}});
  await form.onsubmit({preventDefault(){}});
  assert.equal(writes[0].value,'git@quincy-github:owner/team.git');
  assert.equal(writes[0].ref,'','换 Git 地址应使用新仓库的默认分支');
  assert.equal(writes[0].cache_root,'/ailoom/data/cache');
  assert.deepEqual(approvals,[],'远程 Git 地址由缓存管理，不要求用户选下载目录');
  assert.equal(q('[data-source-error-title]').textContent,'无法从 Git 获取资源');
  assert.ok(q('[data-source-error]').textContent.includes('SSH 身份验证失败'));
  assert.ok(q('[data-source-error-cause]').textContent.includes('Permission denied'));
  assert.equal(dialog.closed,false);
  await q('[data-source-cache-pick]').onclick();
  assert.equal(q('[data-source-cache-root]').value,'/custom/downloads');
  assert.equal(q('[data-source-cache-default]').hidden,false);
  q('[data-source-cache-default]').onclick();
  assert.equal(q('[data-source-cache-root]').value,'/ailoom/data/cache');
  q('[data-source-cache-root]').value='/custom/downloads';
  q('[data-source-cache-root]').oninput();
  await form.onsubmit({preventDefault(){}});
  assert.equal(writes[1].cache_root,'/custom/downloads');
  assert.equal(q('[data-source-cache-root]').value,'/custom/downloads','获取失败应保留自定义下载位置');
  q('[data-source-existing]').onchange();
  assert.equal(q('[data-source-directory]').value,'/new/team','切换方式不能丢失原目录');
  failure=new Error('新目录不是有效的团队资源源');
  await form.onsubmit({preventDefault(){}});
  assert.equal(q('[data-source-directory]').value,'/new/team');
  assert.equal(q('[data-source-error]').textContent,'新目录不是有效的团队资源源');
  assert.equal(dialog.closed,false);
  assert.equal(refreshed,0);
  assert.equal(q('[data-source-save]').disabled,false);
  rejectSave=false;
  await form.onsubmit({preventDefault(){}});
  assert.equal(writes[3].root,'/project');
  assert.equal(writes[3].base_fingerprint,'original');
  assert.equal(writes[3].type,'local');
  assert.equal(writes[3].cache_root,undefined,'已有文件夹模式不修改下载目录');
  assert.deepEqual(approvals,['/custom/downloads','/new/team','/new/team']);
  assert.equal(refreshed,1);
  assert.equal(dialog.closed,true);
});

async function component(file) {
  const document = {createElement: tag => new Element(tag), activeElement: null};
  const context = vm.createContext({document});
  const module = new vm.SourceTextModule(await readFile('src/console/ui/components/' + file, 'utf8'), {context});
  await module.link(name => { if (name === './icons.js') return iconsModule(context); throw new Error('Unexpected dependency'); });
  await module.evaluate();
  return module.namespace;
}

test('按钮变体、重复点击保护，以及异步期间销毁', async () => {
  const {Button} = await component('button.js');
  const root = new Element('div');
  let release, calls = 0;
  const button = Button(root, {label:'保存', variant:'default', onPress:() => { calls++; return new Promise(resolve => {release=resolve;}); }});
  const el = root.children[0];
  assert.equal(el.attributes['data-variant'], 'default');
  const pending = el.onclick();
  assert.equal(el.attributes['aria-busy'], 'true');
  assert.equal(el.disabled, true);
  await el.onclick();
  assert.equal(calls, 1);
  button.destroy(); release(); await pending;
  assert.equal(el.removed, true);
});

test('Field 关联标签、错误语义和禁用状态', async () => {
  const {Field} = await component('field.js');
  const root = new Element('div');
  const field = Field(root, {label:'名称', error:'必填', disabled:true});
  const [label,input,error] = root.children[0].children;
  assert.equal(label.htmlFor, input.id);
  assert.equal(input.attributes['aria-invalid'], 'true');
  assert.equal(error.attributes.role, 'alert');
  assert.equal(input.disabled, true);
  field.update({error:null, disabled:false});
  assert.equal(input.attributes['aria-invalid'], 'false');
  assert.equal(input.disabled, false);
});

test('活动样式只有一个 Token 来源，组件与布局不写独立色值', async () => {
  const root = 'src/console/ui/';
  const tokens = await readFile(root + 'tokens.css', 'utf8');
  const base = await readFile(root + 'base.css', 'utf8');
  const controls = await readFile(root + 'components.css', 'utf8') + await readFile(root + 'workspace.css', 'utf8');
  const shell = await readFile(root + 'shell.html', 'utf8');
  assert.ok(!shell.includes('/ui/theme.css') && !shell.includes('/ui/theme.js'));
  assert.ok(shell.includes('/ui/components.css'));
  const defined = new Set([...(tokens + base + controls).matchAll(/(--[\w-]+)\s*:/g)].map(m => m[1]));
  for (const [,name] of (tokens + base + controls).matchAll(/var\((--[\w-]+)/g)) assert.ok(defined.has(name), name + ' must be defined');
  assert.ok(!/(?:#[\da-f]{3,8}\b|rgba?\(|hsla?\(|oklch\()/i.test(base + controls), 'color values belong in tokens.css');
  for (const name of ['--background','--foreground','--primary','--primary-foreground','--border','--input','--ring']) assert.ok(defined.has(name));
});

test('编辑器标记输入、保留脏正文、正确处理保存期间的新输入', async () => {
  const document = { createElement: tag => new Element(tag), activeElement: null };
  const context = vm.createContext({ document });
  const modules = new Map();
  async function load(file) {
    if (modules.has(file)) return modules.get(file);
    const module = new vm.SourceTextModule(await readFile(file, 'utf8'), { context, identifier: file });
    modules.set(file, module);
    await module.link((name, referencing) => load(path.resolve(path.dirname(referencing.identifier), name)));
    return module;
  }
  const mod = await load(path.resolve('src/console/ui/components/editor.js'));
  await mod.evaluate();
  const root = new Element('div');
  let changed;
  const editor = mod.namespace.Editor(root, { content: 'base', onChange: v => { changed = v; } });
  const input = root.children[0].children[0].children[1];
  assert.equal(editor.value(), 'base');
  input.value = 'my draft'; input.oninput();
  assert.equal(editor.isDirty(), true);
  assert.equal(changed, 'my draft');
  editor.update({ content: 'server response' });
  assert.equal(editor.value(), 'my draft', '过期响应不能覆盖用户输入');
  editor.markSaved('earlier submitted text');
  assert.equal(editor.isDirty(), true, '保存期间的新输入仍是未保存');
  editor.markSaved('my draft');
  assert.equal(editor.isDirty(), false);
  input.value = 'later'; input.oninput();
  editor.setValue('explicit reload');
  assert.equal(editor.value(), 'explicit reload');
  assert.equal(editor.isDirty(), false);
  editor.destroy(); assert.equal(root.children[0].removed, true);
});

test('文件管理器的写入始终绑定所选目录，宿主与能力使用不同字段', async () => {
  const context = vm.createContext({window:{},document:{}});
  const modules = new Map();
  // Only create modules here; link() walks the graph itself, so shared imports (icons.js) are linked once.
  async function load(file) {
    if (!modules.has(file)) modules.set(file, new vm.SourceTextModule(await readFile(file, 'utf8'), {context,identifier:file}));
    return modules.get(file);
  }
  const module = await load(path.resolve('src/console/ui/pages/workspace.js'));
  await module.link((name,referencing)=>load(path.resolve(path.dirname(referencing.identifier),name)));
  await module.evaluate();
  for (const kind of ['git','nongit']) for (const relativeDir of [null,'web/docs']) {
    const target = {kind,rootPath:'/fixture',relativeDir};
    const host = JSON.parse(JSON.stringify(module.namespace.selectionForDirectory(target,{host:'claude'},'enable')));
    assert.equal(host.root,'/fixture');
    assert.equal(host.host,'claude');
    assert.equal(host.resource,undefined);
    assert.equal(host.worktree,kind==='git'||!!relativeDir);
    assert.equal(host.subproject,relativeDir||undefined);
    const resource = module.namespace.selectionForDirectory(target,{resource:'source/skill/ns/example'},'disable');
    assert.equal(resource.resource,'source/skill/ns/example');
    assert.equal(resource.host,undefined);
    assert.equal(resource.state,'disable');
    const project = module.namespace.selectionForDirectory({...target,relativeDir:null,viewKind:'project-shared'},{resource:'x'},'enable');
    assert.equal(project.worktree,false,'Git 项目节点写项目默认，不写当前 Worktree');
  }
});

test('Pi 官方扩展的配置落盘不冒充运行可用状态', async () => {
  const source=await readFile('src/console/ui/pages/workspace.js','utf8');
  const functions=source.slice(source.indexOf('  function extensionRequirements('),source.indexOf('  function renderFooter('));
  const context=vm.createContext({
    effective:{resources:{review:{trace:[]}}}, deployment:{items:[{resource_id:'review',tool:'pi',state:'current'}]},
    capabilities:{capabilities:[{tool:'pi',kind:'agent',required_extension:'Pi 官方 subagent'}]},
    target:{viewKind:'directory'}, diffOf:()=>({}),layerOfTarget:()=>'',icon:()=>'',esc:s=>String(s||''),
    names:{agent:'Agent'},tools:{pi:'Pi'},catalog:{entries:[]},busy:false,parentNode:()=>null,inherits:()=>false,
  });
  Object.assign(context,await sharedComponents(context),await targetHelpers(context));
  vm.runInContext(functions,context);
  const entry={id:'review',kind:'agent',name:'Reviewer'};
  let html=context.row(entry,false);
  assert.match(html,/已配置/);
  assert.match(html,/需要安装 Pi 官方 subagent/,'配置写好不等于扩展可用：提示用户安装');
  assert.doesNotMatch(html,/workspace-resource-status current/);
  context.deployment.items[0].state='not-deployed';
  assert.match(context.row(entry,false),/待应用/);
  context.deployment.items[0]={resource_id:'review',tool:'cursor',state:'current'};
  html=context.row(entry,false);
  assert.match(html,/已应用/);
  assert.doesNotMatch(html,/依赖官方扩展/);
  context.deployment.issues=[{resource_id:'review',target_tool:'pi',reason:'配置不兼容'}];
  assert.match(context.row(entry,false),/需处理/);
});

test('配置读取失败后的 finally 不覆盖未知状态，也不显示可应用', async () => {
  const source=await readFile('src/console/ui/pages/workspace.js','utf8');
  const footer=source.slice(source.indexOf('  function renderFooter()'),source.indexOf('  async function write('));
  const element={innerHTML:''};
  const context=vm.createContext({effective:null,target:{viewKind:'directory'},q:()=>element});
  vm.runInContext(footer,context);
  for(const viewKind of ['directory','project-shared']){
    context.target.viewKind=viewKind;
    context.renderFooter();
    assert.match(element.innerHTML,/文件状态尚未确认/);
    assert.match(element.innerHTML,/<button disabled>/);
    assert.doesNotMatch(element.innerHTML,/默认选择已保存|暂无待应用改动/);
  }
});

test('知识库迁移失败后清除旧预览，下一次提交重新预览', async () => {
  const elements = new Map();
  const get = selector => {
    if (!elements.has(selector)) elements.set(selector, {value:'',textContent:'',disabled:false});
    return elements.get(selector);
  };
  get('[data-form]').elements={path:get('[name=path]')};
  get('[data-form]').querySelectorAll=()=>[];
  const section={set innerHTML(_){},querySelector:s=>s==='[data-recovery-mode]'?null:get(s),querySelectorAll:()=>[],remove(){}};
  const requests=[];
  let moves=0;
  const api={approveDir:async()=>({}),knowledge:async body=>{
    if(body.action==='status')return {initialized:true,location:{path:'/old'},files:1};
    if(body.action==='inspect')return {git:null};
    requests.push(body);
    if(++moves===2)throw new Error('知识库或迁移设置已变化，请重新预览');
    return {preview:true,expected:'preview-'+moves,to:'/new',files:moves,affected_projects:['project']};
  }};
  const context=vm.createContext({document:{createElement:()=>section}});
  const module=new vm.SourceTextModule(await readFile('src/console/ui/features/knowledgePanel.js','utf8'),{context});
  await module.link(name=>{
    const values=name.includes('knowledgeRecovery')?{recoveryFields:()=>'',cloneDestination:()=>''}:name.includes('knowledgeOrganization')?{KnowledgeOrganization:()=>({destroy(){}})}:{api,esc:String};
    return new vm.SyntheticModule(Object.keys(values),function(){for(const [k,v]of Object.entries(values))this.setExport(k,v);},{context});
  });
  await module.evaluate();module.namespace.KnowledgePanel({append(){}},{rootPath:'/project'});
  await new Promise(resolve=>setImmediate(resolve));
  get('[name=path]').value='/new';
  const submit=()=>get('[data-form]').onsubmit({preventDefault(){}});
  await submit();assert.equal(get('[data-submit]').textContent,'迁移并切换');
  await submit();assert.equal(get('[data-submit]').textContent,'预览迁移');
  assert.equal(get('[data-preview]').textContent,'');
  await submit();assert.equal(requests[2].execute,false);assert.equal(requests[2].expected,undefined);
});


async function collectionPanel(api, confirmations=[], approve=true) {
  class PanelElement extends Element {
    constructor(tag) { super(tag); this.dataset = {}; this.elements = new Map(); }
    querySelector(selector) {
      if (!this.elements.has(selector)) this.elements.set(selector, new PanelElement('div'));
      return this.elements.get(selector);
    }
    querySelectorAll() { return []; }
    before() {}
    after() {}
    insertAdjacentHTML() {}
    get nextElementSibling() { return this.querySelector(':next'); }
    insertBefore(node) { this.children.push(node); }
    setAttribute(name,value) { (this.attributes??={})[name]=value; }
    addEventListener(name, listener) { this['on' + name] = listener; }
    removeAttribute(name) { delete this.attributes?.[name]; }
  }
  const context = vm.createContext({URL,document:{createElement:tag => new PanelElement(tag)}});
  const module = new vm.SourceTextModule(await readFile('src/console/ui/features/collectionsPanel.js','utf8'), {context});
  const dialog = (_container,props) => {
    if(props?.title==='更新资源库') {
      confirmations.push(props.content.innerHTML);
      queueMicrotask(()=>props.onClose(approve&&props.actions.some(a=>a.returnValue===true)));
    }
    return {show(){},close(){props?.onClose?.(false);},destroy(){}};
  };
  api.state??=async()=>({repos:[]});
  const dependencies = {
    '../services/api.js': {api,esc:String},
    '../components/item.js':await sharedComponents(context),
    '../components/layout.js':{PageHeader:()=>'<header></header>'},
    '../state/target.js': {setTarget(){},currentTarget:()=>null},
    '../components/dialog.js': {Dialog:dialog,confirmAction:async text=>{confirmations.push(text);return true;}},
    './ccSwitchImport.js': {CcSwitchImport:dialog},
    './resourceReferences.js': {ResourceReferences:dialog},
    './importDialog.js': {ImportDialog:dialog},
    './gitSkillDialog.js': {GitSkillDialog:dialog},
    './sourceBindingDialog.js': {SourceBindingDialog:dialog},
  };
  await module.link(name => {
    const exports = dependencies[name];
    return new vm.SyntheticModule(Object.keys(exports), function() {
      for (const [key,value] of Object.entries(exports)) this.setExport(key,value);
    }, {context});
  });
  await module.evaluate();
  const container = new PanelElement('div');
  const panel = module.namespace.CollectionsPanel(container, {compactLibrary:true});
  container.children[0].querySelector('[data-library-sort]').value='repository';
  await panel.refresh();
  return {panel,root:container.children[0],catalogGroups:module.namespace.catalogGroups};
}

test('左栏仓库过滤叠加搜索限制检查与更新，合集按完整版本确认', async () => {
  const checks=[],writes=[],confirmations=[];
  const source={id:'a',name:'设计与开发合集',url:'https://github.com/example/tools',management:'managed',resources:[
    {id:'a/design',kind:'skill',name:'design-template',path:'skills/design/template'},
    {id:'a/code',kind:'skill',name:'code-template',path:'skills/development/template'}],update:{state:'available',preview:{preview_id:'a-new',removed:[]}}};
  const other={id:'b',name:'其他合集',url:'https://gitlab.com/example/tools',resources:[{id:'b/design',name:'other-template',kind:'skill',path:'skills/design/template'}],update:{state:'available',preview:{preview_id:'b-new'}}};
  const entries=['design','development'].map(category=>({id:'personal/'+category,kind:'skill',name:category,source:{repo_url:source.url,repo_path:'skills/'+category+'/solo'},can_check_update:true,update:{state:'upstream-new',preview_id:category+'-new'}}));
  const {root}=await collectionPanel({collections:async()=>({sources:[source,other]}),libraryList:async()=>({entries}),
    collectionCheck:async id=>{checks.push(id);return {items:[]};},checkUpdate:async name=>{checks.push(name);return {status:{state:'upstream-new'}};},
    collectionUpdate:async ids=>{writes.push(...ids);},updateSkill:async name=>{writes.push(name);}},confirmations);
  root.querySelector('[data-repository-filters]').onclick({target:{closest:()=>({dataset:{repositoryFilter:'github.com/example/tools'}})}});
  root.querySelector('[data-search]').value='design';root.querySelector('[data-search]').oninput();
  const catalog=root.querySelector('[data-capability-catalog]');
  assert.ok(catalog.innerHTML.includes('design-template'));
  assert.ok(!catalog.innerHTML.includes('code-template')&&!catalog.innerHTML.includes('other-template'));
  assert.equal(root.querySelector('[data-check]').textContent,'检查筛选项');
  await root.querySelector('[data-check]').onclick();assert.deepEqual(checks,['a','design']);
  await root.querySelector('[data-update]').onclick();assert.deepEqual(writes,['a-new','design']);
  assert.ok(confirmations[0].includes('1 个合集和1 个独立 Skill'));
  assert.ok(confirmations[0].includes('每个合集整体更新'));
  assert.ok(confirmations[0].includes('data-update-skill="personal/design"'));
  assert.ok(!confirmations[0].includes('data-update-skill="personal/development"'));
  root.querySelector('[data-search]').value='does-not-exist';root.querySelector('[data-search]').oninput();
  assert.equal(root.querySelector('[data-check]').disabled,true);
  assert.equal(root.querySelector('[data-update]').disabled,true);
});

test('同仓库 Skill 集中排序，Git 传输别名合并，跨平台同名仓库分开', async () => {
  const {catalogGroups}=await collectionPanel({collections:async()=>({sources:[]}),libraryList:async()=>({entries:[]})});
  const sources=[
    {id:'a',name:'legacy',url:'https://github.com/example/tools.git',resources:[{id:'a/z',kind:'skill',name:'zebra'}]},
    {id:'b',name:'legacy',url:'git@github.com:example/tools',resources:[{id:'b/a',kind:'skill',name:'alpha'}]},
    {id:'c',name:'legacy',url:'https://gitlab.com/example/tools',resources:[{id:'c/a',kind:'skill',name:'alpha'}]},
  ];
  const personal=[{id:'personal/skill',kind:'skill',name:'middle',source:{repo_url:'https://github.com/example/tools',ref:'main'}},{id:'local',kind:'rule',name:'规则'}];
  const groups=catalogGroups(sources,personal);
  assert.equal(groups.length,3);
  const github=groups.find(g=>g.key==='github.com/example/tools');
  assert.deepEqual(Array.from(github.entries,e=>e.name),['alpha','middle','zebra']);
  assert.equal(github.sources.length,2);
  assert.equal(groups.find(g=>g.key==='gitlab.com/example/tools').entries.length,1);
  assert.equal(groups.at(-1).key,'local:personal');
});

test('SSH 别名使用后端仓库身份分组，默认分支不回退旧版本分支', async () => {
  const {catalogGroups}=await collectionPanel({collections:async()=>({sources:[]}),libraryList:async()=>({entries:[]})});
  const sources=[{id:'a',name:'source',url:'https://github.com/owner/tools',repository_url:'https://github.com/owner/tools',repository_key:'github.com/owner/tools',connection_url:'git@quincy-github:owner/tools.git',update_ref:null,lock:{ref:'main'},resources:[]}];
  const entries=[{id:'personal/skill/personal/x',name:'x',kind:'skill',repository_url:'https://github.com/owner/tools',repository_key:'github.com/owner/tools',connection_url:'git@different-alias:owner/tools.git',source:{repo_url:'https://github.com/owner/tools',ref:'main',connection:{ref:null}}}];
  const groups=catalogGroups(sources,entries);assert.equal(groups.length,1);assert.equal(groups[0].entries.length,1);assert.equal(groups[0].url,'git@quincy-github:owner/tools.git');assert.equal(groups[0].ref,null);
  const personal=catalogGroups([],entries);assert.equal(personal[0].ref,null);assert.equal(personal[0].url,'git@different-alias:owner/tools.git');
});

function checkedSkill() {
  return {id:'personal/skill/personal/solo',kind:'skill',name:'solo',can_check_update:true,
    source:{source_kind:'future-provider'},update:{state:'upstream-new',preview_id:'checked-two'}};
}

test('Skill 更新控件使用后端能力，不依赖来源类型或编辑入口', async () => {
  const entry = checkedSkill(), checks = [];
  const {root} = await collectionPanel({
    collections:async()=>({sources:[]}),libraryList:async()=>({entries:[entry]}),
    checkUpdate:async name => { checks.push(name); entry.update={state:'up-to-date'}; return {status:entry.update}; },
  });
  const catalog = root.querySelector('[data-capability-catalog]');
  assert.ok(catalog.innerHTML.includes('data-skill-update="solo"'));
  await root.querySelector('[data-check]').onclick();
  assert.deepEqual(checks,['solo']);
  assert.ok(catalog.innerHTML.includes('已是最新'));
  assert.ok(!catalog.innerHTML.includes('data-skill-update'));
});

test('检查失败后的后端状态会替换旧候选，刷新后仍不可更新', async () => {
  const entry = checkedSkill();
  const {root,panel} = await collectionPanel({
    collections:async()=>({sources:[]}),libraryList:async()=>({entries:[entry]}),
    checkUpdate:async()=>{entry.update={state:'error',note:'上游离线'};throw new Error('上游离线');},
  });
  await root.querySelector('[data-check]').onclick();
  await panel.refresh();
  assert.ok(root.querySelector('[data-capability-catalog]').innerHTML.includes('检查失败'));
  assert.ok(!root.querySelector('[data-capability-catalog]').innerHTML.includes('data-skill-update'));
  assert.ok(root.querySelector('[data-check-progress]').innerHTML.includes('上游离线'));
});

test('合集批量更新失败后继续独立 Skill，并刷新两者状态', async () => {
  const entry=checkedSkill(), writes=[];
  const source={id:'tools',name:'tools',url:'https://example.test/tools',resources:[],
    update:{state:'available',preview:{preview_id:'tools-two'}}};
  const {root} = await collectionPanel({
    collections:async()=>({sources:[source]}),libraryList:async()=>({entries:[entry]}),
    collectionUpdate:async tokens=>{writes.push(['collection',...tokens]);source.update={state:'stale'};throw new Error('合集预览过期');},
    updateSkill:async(...args)=>{writes.push(['skill',...args]);entry.update={state:'up-to-date'};},
  });
  await root.querySelector('[data-update]').onclick();
  assert.deepEqual(writes,[['collection','tools-two'],['skill','solo',true,'checked-two']]);
  assert.ok(root.querySelector('[data-msg]').textContent.includes('合集预览过期'));
  assert.ok(root.querySelector('[data-capability-catalog]').innerHTML.includes('已是最新'));
  assert.ok(!root.querySelector('[data-capability-catalog]').innerHTML.includes('data-skill-update'));
  assert.equal(root.querySelector('[data-update]').disabled,true);
});


test('项目与记录保留分页边界；Item 文本正确转义',async()=>{
  const {paginate,Item,Pagination}=await sharedComponents(vm.createContext({}));
  const resources=Array.from({length:200},(_,i)=>({id:'source-'+i+'/skill/review',name:'review'}));
  const first=paginate(resources,1,5),last=paginate(resources,40,5);
  assert.equal(first.items.length,5);assert.equal(last.items[4].id,resources[199].id);
  assert.equal(paginate(resources,100,5).page,40);assert.equal(paginate([],3,5).page,1);
  assert.equal(new Set([...first.items,...last.items].map(e=>e.id)).size,10);
  assert.match(Pagination(last),/disabled>下一页/);
  const html=Item({title:'<img src=x onerror=alert(1)>',description:'A&B',meta:'<script>'});
  assert.ok(!html.includes('<img'));assert.match(html,/A&amp;B/);assert.match(html,/&lt;script&gt;/);
});

test('Item 主入口包含内容和留白，次操作独立且不会嵌套按钮',async()=>{
  const {Item}=await sharedComponents(vm.createContext({}));
  const html=Item({title:'review',description:'检查代码',leading:'<svg></svg>',primaryAction:{attributes:'data-open',label:'查看 review'},actions:'<button data-remove>移除</button>'});
  const primary=html.match(/<button class="ui-item-primary item-surface"[\s\S]*?<\/button>/)?.[0];
  assert.ok(primary,'主入口必须是真实按钮，可由 Enter / Space 激活');
  assert.match(primary,/<svg>/);assert.match(primary,/review/);assert.match(primary,/检查代码/);
  assert.match(primary,/type="button"/);assert.match(primary,/aria-label="查看 review"/);
  assert.ok(!primary.includes('data-remove'),'移除必须在主入口以外');
  assert.match(html,/<\/button><div class="ui-item-actions"><button data-remove>/);
  const link=Item({title:'项目',primaryAction:{href:'#/projects/a?x=1&y=2'},trailing:'Git'});
  assert.match(link,/<a class="ui-item-primary item-surface" href="#\/projects\/a\?x=1&amp;y=2"/);
  assert.match(link,/Git[\s\S]*<\/a>/,'仅展示的右侧信息属于同一个链接');
  const passive=Item({title:'待写入文件'});
  assert.ok(!passive.includes('ui-item-primary'),'说明条目不伪装成可点击项');
});


test('全局去重按完整资源身份与当前工具的实际部署判断',async()=>{
  const {globallyAvailable}=await targetHelpers(vm.createContext({}));
  const effective={hosts:{codex:{enabled:true},claude:{enabled:true}},global_skills:['source-a/skill/review'],global_deployments:[{resource_id:'source-a/skill/review',tool:'codex'}]};
  assert.equal(globallyAvailable(effective).size,0,'只部署 Codex 时，仍可为 Claude 选择');
  effective.global_deployments.push({resource_id:'source-a/skill/review',tool:'claude'});
  assert.equal(globallyAvailable(effective).has('source-a/skill/review'),true);
  assert.equal(globallyAvailable(effective).has('source-b/skill/review'),false,'同名不同来源不合并');
  effective.hosts.claude.enabled=false;effective.global_deployments.pop();assert.equal(globallyAvailable(effective).size,1);
  effective.hosts.codex.enabled=false;assert.equal(globallyAvailable(effective).size,0);
});


test('工作目录全部失联时，登记入口给出恢复提示而非空选择器',async()=>{
  const source=await readFile('src/console/ui/pages/workspace.js','utf8');
  const fn=source.slice(source.indexOf('  function addEnvironment()'),source.indexOf('  async function refresh()'));
  const notices=[];
  const context=vm.createContext({busy:false,isGit:()=>true,repo:{worktrees:{lost:{status:'missing'}}},message:notice=>notices.push(notice),document:{createElement(){throw Error('不能打开没有可用工作目录的表单');}}});
  vm.runInContext(fn,context);context.addEnvironment();
  assert.equal(notices.length,1);assert.match(notices[0],/先重新关联/);
});


test('特殊路径保持可见，常规 Skill 根目录与托管缓存不占列表行',async()=>{
  const {resourcePath,Item}=await sharedComponents(vm.createContext({}));
  for(const path of ['skills/review','.agents/skills/review','review','.'])assert.equal(resourcePath({name:'review',source:{repo_path:path}}),'');
  assert.equal(resourcePath({name:'review',source:{repo_path:'packages/web/skills/review'}}),'packages/web/skills/review');
  assert.equal(resourcePath({name:'review',source:{repo_path:'packages/web/.agents/skills/review'}}),'packages/web/.agents/skills/review');
  assert.equal(resourcePath({name:'review',readonly:true,path:'/custom/project/.agents/skills/review'}),'/custom/project/.agents/skills/review');
  assert.equal(resourcePath({name:'review',path:'resources/skills/review'}),'','个人副本的内部存储位置不显示');
  assert.equal(resourcePath({name:'review',source:{source_kind:'local',discovery_entry:'local-dir:/Users/example/Skills/review'}}),'/Users/example/Skills/review');
  assert.equal(resourcePath({name:'review',readonly:true,path:'/Users/example/.agents/skills/review'}),'');
  const html=Item({title:'review',path:'packages/<web>/skills/review'});
  assert.match(html,/ui-item-path/);assert.ok(html.includes('packages/&lt;web&gt;/skills/review'));
});


test('带工具动作的选择行使用完整 label 命中区，工具按钮保留在外侧',async()=>{
  const {Item}=await sharedComponents(vm.createContext({}));
  const html=Item({title:'review',primaryAction:{tag:'label',attributes:'for="skill-choice"'},leading:'<input id="skill-choice" type="checkbox">',actions:'<button>Codex</button>'});
  assert.match(html,/<label class="ui-item-primary item-surface"/);
  assert.ok(html.indexOf('</label>')<html.indexOf('<button>Codex</button>'));
  assert.doesNotMatch(html,/<button[^>]*>[\s\S]*<input/);
});


test('能力资源库连续列出 200 项，按完整身份保留同名条目',async()=>{
  const entries=Array.from({length:200},(_,i)=>({id:'personal/skill/source-'+i+'/review',kind:'skill',name:'review',description:'用途 '+i}));
  const {root}=await collectionPanel({collections:async()=>({sources:[]}),libraryList:async()=>({entries})});
  const html=root.querySelector('[data-capability-catalog]').innerHTML;
  const ids=[...html.matchAll(/data-library-id="([^"]+)"/g)].map(m=>m[1]);
  assert.equal(ids.length,200);assert.equal(new Set(ids).size,200);
  assert.ok(ids.includes(entries.at(-1).id));assert.ok(!html.includes('data-page='));
});
