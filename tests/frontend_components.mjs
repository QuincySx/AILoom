// Node 内置测试；这里只验证组件状态，不冒充真实浏览器验收。
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import path from 'node:path';

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.value = ''; this.style = {}; this.classList = { add() {}, remove() {} }; }
  append(...nodes) { this.children.push(...nodes); }
  appendChild(node) { this.append(node); }
  setAttribute(name, value) { (this.attributes ??= {})[name] = value; }
  remove() { this.removed = true; }
}

async function component(file) {
  const document = {createElement: tag => new Element(tag), activeElement: null};
  const module = new vm.SourceTextModule(await readFile('src/console/ui/components/' + file, 'utf8'), {context:vm.createContext({document})});
  await module.link(() => { throw new Error('Unexpected dependency'); });
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
  async function load(file) {
    if (modules.has(file)) return modules.get(file);
    const module = new vm.SourceTextModule(await readFile(file, 'utf8'), {context,identifier:file});
    modules.set(file,module);
    await module.link((name,referencing)=>load(path.resolve(path.dirname(referencing.identifier),name)));
    return module;
  }
  const module = await load(path.resolve('src/console/ui/pages/workspace.js'));
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
    names:{agent:'Agent'},tools:{pi:'Pi'},busy:false,
  });
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

test('知识库迁移失败后清除旧预览，下一次提交重新预览', async () => {
  const elements = new Map();
  const get = selector => {
    if (!elements.has(selector)) elements.set(selector, {value:'',textContent:'',disabled:false});
    return elements.get(selector);
  };
  get('[data-form]').elements={path:get('[name=path]')};
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
    const values=name.includes('knowledgeRecovery')?{recoveryFields:()=>'',cloneDestination:()=>''}:{api,esc:String};
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


async function collectionPanel(api) {
  class PanelElement extends Element {
    constructor(tag) { super(tag); this.dataset = {}; this.elements = new Map(); }
    querySelector(selector) {
      if (!this.elements.has(selector)) this.elements.set(selector, new PanelElement('div'));
      return this.elements.get(selector);
    }
    querySelectorAll() { return []; }
    before() {}
    insertBefore(node) { this.children.push(node); }
    addEventListener(name, listener) { this['on' + name] = listener; }
    removeAttribute(name) { delete this.attributes?.[name]; }
  }
  const context = vm.createContext({document:{createElement:tag => new PanelElement(tag)}});
  const module = new vm.SourceTextModule(await readFile('src/console/ui/features/collectionsPanel.js','utf8'), {context});
  const dialog = () => ({show(){},destroy(){}});
  const dependencies = {
    '../services/api.js': {api,esc:String},
    '../state/target.js': {setTarget(){},currentTarget:()=>null},
    '../components/dialog.js': {Dialog:dialog,confirmAction:async()=>true},
    './ccSwitchImport.js': {CcSwitchImport:dialog},
    './resourceReferences.js': {ResourceReferences:dialog},
    './importDialog.js': {ImportDialog:dialog},
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
  await panel.refresh();
  return {panel,root:container.children[0]};
}

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
  assert.ok(root.querySelector('[data-msg]').textContent.includes('上游离线'));
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
