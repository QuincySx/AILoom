// Node component-state regression; real browser screenshots are separate evidence.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

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

async function fixture(initial={}) {
  const model={revision:1,home:'/fixture',notes:[],skills:[],targets:[{key:'claude',label:'Claude Code',enabled:true,supported:true,dir:'/fixture/claude/skills'}],foreign:[{name:'sample',target:'claude',path:'/fixture/claude/skills/sample',conflicts_with:'x',location:{kind:'dir'}}],archive:[{id:'archive-1'}],...initial};
  const element=tag=>({tag,classList:{add(){},remove(){},toggle(){return true;}},children:[],value:'',textContent:'',innerHTML:'',dataset:{},nodes:new Map(),append(...nodes){this.children.push(...nodes);},setAttribute(key,value){this[key]=value;},addEventListener(event,handler){this['on'+event]=handler;},querySelectorAll(){return [];},querySelector(selector){if(!this.nodes.has(selector))this.nodes.set(selector,element('div'));return this.nodes.get(selector);}});
  const elements = new Map();
  const get = selector => {
    if (!elements.has(selector)) elements.set(selector, {textContent:'',innerHTML:'',value:'',dataset:{},querySelectorAll(){return [];}});
    return elements.get(selector);
  };
  const takeover = {dataset:{takeover:'0'}}, restore = {dataset:{restore:'0'}}, targetBoxes=[];
  const section = {
    set innerHTML(_) {}, append() {}, remove() {},
    querySelector:get,
    querySelectorAll:selector => selector==='[data-takeover]'?[takeover]:selector==='[data-restore]'?[restore]:selector==='[data-global-target-skill]'?targetBoxes:[],
  };
  const document = {body:{},createElement:tag => tag==='section'?section:element(tag)};
  const dialogs=[], notices=[];
  let reads=0, takeoverCalls=0;
  const api = {
    globalSkills:async()=>{reads++;return model;},
    globalSelect:async body=>{model.revision++;const skill=model.skills.find(s=>s.id===body.skill);if(skill){skill.global=body.enabled;skill.targets=body.targets||[];}return {revision:model.revision};},
    globalTakeover:async()=>{takeoverCalls++;throw new Error('无法写入归档：磁盘已满');},
    globalRestore:async()=>{throw new Error('原位置已被占用');},
    globalPlan:async()=>({actions:[{action:'create',path:'claude/skills/sample'}]}),
    globalApply:async()=>{throw new Error('全局应用失败');},
  };
  const context=vm.createContext({document,URL});
  const module=new vm.SourceTextModule(await readFile('src/console/ui/features/globalSkills.js','utf8'),{context});
  await module.link(async name=>{
    if(name.endsWith('/layout.js'))return new vm.SyntheticModule(['PageHeader'],function(){this.setExport('PageHeader',()=>'<header></header>');},{context});
    if(name.endsWith('/item.js')){const values=await sharedComponents(context);return new vm.SyntheticModule(Object.keys(values),function(){for(const [key,value]of Object.entries(values))this.setExport(key,value);},{context});}
    const values=name.includes('/services/')?{api,esc:String}:name.includes('/components/')?{
      Dialog:(_,props)=>{dialogs.push(props);return {show(){},close(){},destroy(){},update(next){Object.assign(props,next);}};},confirmAction:async()=>true,
    }:{notify:value=>notices.push(value)};
    return new vm.SyntheticModule(Object.keys(values),function(){for(const [key,value]of Object.entries(values))this.setExport(key,value);},{context});
  });
  await module.evaluate();module.namespace.GlobalSkills({append(){}});
  const flush=()=>new Promise(resolve=>setImmediate(resolve));await flush();
  return {api,get,model,takeover,restore,targetBoxes,dialogs,notices,flush,reads:()=>reads,takeoverCalls:()=>takeoverCalls};
}

function selectCandidate(body,id) {
  return body.querySelector('[data-global-add-list]').onclick({target:{closest:()=>({dataset:{globalAddId:id}})}});
}
async function addCandidate(body,id) {
  selectCandidate(body,id);
  return body.querySelector('[data-global-add-submit]').onclick();
}

test('全局工具只用 Agent 标志切换，保留名称、状态与不可用原因',async()=>{
  const f=await fixture({targets:[{key:'claude',label:'Claude Code',enabled:true,supported:true},{key:'agents',label:'Codex',enabled:true,supported:true}],skills:[{id:'a',name:'review',global:true,targets:['claude']}],deployed:[{resource_id:'a',tool:'claude'}]});
  const html=f.get('[data-list]').innerHTML;
  // Claude has its own switch; the shared directory is one group switch naming the agents that read it.
  const buttons=[...html.matchAll(/<button[^>]*class="agent-(?:icon-toggle|group)"[^>]*>.*?<\/button>/g)].map(m=>m[0]);
  assert.equal(buttons.length,2);
  assert.match(buttons[0],/class="agent-icon-toggle" [^>]*data-target-key="claude" aria-pressed="true"/);
  assert.match(buttons[1],/class="agent-group" [^>]*data-target-key="agents" aria-pressed="false"/);
  assert.match(buttons[0],/aria-label="review · Claude Code · 已部署/);
  assert.match(buttons[1],/aria-label="review · 通用目录（Codex 等读取） · 未选择/);
  assert.ok(buttons.every(b=>b.includes('<svg')&&!b.includes('<span>')&&!b.includes('role="checkbox"')));
  assert.doesNotMatch(html,/data-global-manage/);
});

test('全局主列表只显示已添加技能，搜索涵盖用途；未添加的大库留给弹窗',async()=>{
  const f=await fixture({skills:[{id:'selected',name:'already-added',description:'网页布局检查',global:true},...Array.from({length:300},(_,i)=>({id:'unused-'+i,name:'unused-'+i,global:false}))],deployed:[{resource_id:'selected'}]});
  assert.ok(f.get('[data-list]').innerHTML.includes('already-added'));
  assert.ok(!f.get('[data-list]').innerHTML.includes('unused-'));
  f.get('[data-search]').oninput({target:{value:'布局'}});
  assert.ok(f.get('[data-list]').innerHTML.includes('already-added'));
  assert.match(f.get('[data-global-count]').textContent,/1 个/);
  f.get('[data-add-skill]').onclick();const body=f.dialogs[0].content;
  assert.ok(body.querySelector('[data-global-add-list]').innerHTML.includes('unused-299'),'all matching skills are available by continuous scrolling');
  body.querySelector('[data-global-add-search]').value='unused-299';body.querySelector('[data-global-add-search]').oninput();
  assert.ok(body.querySelector('[data-global-add-list]').innerHTML.includes('unused-299'));
  body.querySelector('[data-global-add-search]').value='';body.querySelector('[data-global-add-search]').oninput();
  assert.match(body.querySelector('[data-global-add-count]').textContent,/301 \/ 301 项/);
});

test('添加弹窗以仓库分组，状态过滤叠加用途搜索；清空搜索后可继续浏览',async()=>{
  const f=await fixture({skills:[
    {id:'a',name:'alpha',description:'检查网页布局',global:false,source:'资源库',repository_url:'https://github.com/example/skills.git',repository_key:'github.com/example/skills'},
    {id:'b',name:'beta',description:'检查网页布局',global:true,source:'团队合集',repository_url:'git@github.com:example/skills',repository_key:'github.com/example/skills'},
    {id:'c',name:'other-platform',global:false,source:'另一个合集',repository_url:'https://gitlab.com/example/skills.git',repository_key:'https://gitlab.com/example/skills'},
    {id:'local',name:'local',global:false},
  ]});
  f.get('[data-add-skill]').onclick();const body=f.dialogs[0].content,q=s=>body.querySelector(s);
  const group=value=>q('[data-global-add-groups]').onclick({target:{closest:()=>({dataset:{globalAddGroup:value}})}});
  const state=value=>q('[data-global-add-states]').onclick({target:{closest:()=>({dataset:{globalAddState:value}})}});
  assert.match(q('[data-global-add-count]').textContent,/4 \/ 4 项/);
  assert.match(q('[data-global-add-groups]').innerHTML,/github.com/);
  assert.match(q('[data-global-add-groups]').innerHTML,/本地与个人资源/);
  group('github.com/example/skills');
  assert.match(q('[data-global-add-count]').textContent,/2 \/ 4 项/);
  assert.ok(!q('[data-global-add-list]').innerHTML.includes('other-platform'));
  state('available');assert.match(q('[data-global-add-count]').textContent,/1 \/ 4 项/);
  assert.ok(q('[data-global-add-list]').innerHTML.includes('alpha'));
  assert.ok(!q('[data-global-add-list]').innerHTML.includes('>beta<'));
  q('[data-global-add-search]').value='布局';q('[data-global-add-search]').oninput();
  assert.ok(q('[data-global-add-list]').innerHTML.includes('alpha'));
  q('[data-global-add-search]').value='不存在';q('[data-global-add-search]').oninput();
  assert.match(q('[data-global-add-list]').innerHTML,/清除筛选/);
  q('[data-global-add-search]').value='';q('[data-global-add-search]').oninput();
  assert.ok(q('[data-global-add-list]').innerHTML.includes('alpha'));
  state('added');assert.ok(q('[data-global-add-list]').innerHTML.includes('>beta<'));
  assert.match(q('[data-global-add-list]').innerHTML,/disabled checked/);
  q('[data-global-add-search]').value='missing';q('[data-global-add-search]').oninput();
  q('[data-global-add-clear]').onclick();assert.match(q('[data-global-add-count]').textContent,/4 \/ 4 项/);
});

test('搜索添加并开启只应用点选项，写入期间阻止关闭',async()=>{
  const writes=[];
  const f=await fixture({targets:[{key:'agents',enabled:true,supported:true}],skills:[{id:'chosen',name:'design-review',description:'检查布局',global:false},{id:'other',name:'other',global:false}],deployed:[]});
  f.api.globalSelect=async body=>{writes.push(['select',body.skill,body.enabled,body.base_revision]);f.model.skills[0].global=true;return {revision:2};};
  let release;
  f.api.globalApply=id=>{writes.push(['apply',id]);return new Promise(resolve=>{release=()=>{f.model.deployed=[{resource_id:id,tool:'codex'}];resolve({skipped_conflicts:[]});};});};
  f.get('[data-add-skill]').onclick();const dialog=f.dialogs[0],body=dialog.content;
  assert.ok(body.querySelector('[data-global-add-list]').innerHTML.includes('design-review'));
  body.querySelector('[data-global-add-search]').value='布局';body.querySelector('[data-global-add-search]').oninput();
  assert.ok(body.querySelector('[data-global-add-list]').innerHTML.includes('design-review'));
  assert.ok(!body.querySelector('[data-global-add-list]').innerHTML.includes('>other<'));
  const saving=addCandidate(body,'chosen');
  await f.flush();assert.equal(dialog.canClose(),false);assert.equal(body.querySelector('[data-global-add-cancel]').disabled,true);
  release();assert.equal(await saving,true);
  assert.deepEqual(writes,[['select','chosen',true,1],['apply','chosen']]);
  assert.equal(dialog.canClose(),true);assert.match(f.notices[0],/已开启/);
});

test('开启失败保留已添加状态与错误，不误报已启用',async()=>{
  const f=await fixture({targets:[{key:'agents',enabled:true,supported:true}],skills:[{id:'chosen',name:'selected',global:false}],deployed:[],pending:1,actions:[{resource_id:'chosen',action:'create'}]});
  f.get('[data-add-skill]').onclick();const body=f.dialogs[0].content;
  body.querySelector('[data-global-add-search]').value='selected';body.querySelector('[data-global-add-search]').oninput();
  assert.equal(await addCandidate(body,'chosen'),false);
  assert.match(body.querySelector('[data-global-add-error]').textContent,/已添加，但开启失败/);
  assert.ok(f.get('[data-list]').innerHTML.includes('待应用'));
  assert.equal(f.notices.length,0);
});

test('开启写入成功但刷新失败时，主页面恢复操作并保留核对提示',async()=>{
  const f=await fixture({skills:[{id:'chosen',name:'selected',global:false}],deployed:[]});
  f.api.globalApply=async()=>({skipped_conflicts:[]});
  f.get('[data-add-skill]').onclick();const dialog=f.dialogs[0],body=dialog.content;
  body.querySelector('[data-global-add-search]').value='selected';body.querySelector('[data-global-add-search]').oninput();
  f.api.globalSkills=async()=>{throw new Error('连接中断');};
  assert.equal(await addCandidate(body,'chosen'),false);
  for(const selector of ['[data-refresh]','[data-add-skill]','[data-search]'])assert.equal(f.get(selector).disabled,false,selector+' must be usable for recovery');
  assert.equal(dialog.canClose(),true);
  assert.match(f.get('[data-status]').textContent,/操作已提交，但列表刷新失败/);
  assert.equal(f.notices.length,0);
});

test('每个候选独立选择工具，过滤保留草稿，无工具不可添加，重新打开重置草稿',async()=>{
  const f=await fixture({targets:[{key:'claude',label:'Claude Code',supported:true,enabled:false},{key:'agents',supported:true,enabled:false}],skills:[{id:'alpha',name:'alpha',global:false},{id:'beta',name:'beta',global:false}],deployed:[]});
  assert.equal(f.get('[data-add-skill]').disabled,false,'默认全关也能打开弹窗');
  f.get('[data-add-skill]').onclick();const body=f.dialogs[0].content,list=body.querySelector('[data-global-add-list]');
  assert.equal(body.querySelector('[data-global-add-submit]').disabled,true);
  selectCandidate(body,'alpha');assert.equal(body.querySelector('[data-global-add-submit]').disabled,true,'选择技能后仍需选择工具');selectCandidate(body,'alpha');
  const choose=(id,key,checked)=>list.onclick({target:{closest:()=>({checked,dataset:{globalAddTargetSkill:id,targetKey:key}})}});
  choose('alpha','claude',true);choose('beta','agents',true);
  body.querySelector('[data-global-add-search]').value='beta';body.querySelector('[data-global-add-search]').oninput();
  body.querySelector('[data-global-add-search]').value='';body.querySelector('[data-global-add-search]').oninput();
  assert.match(list.innerHTML,/data-global-add-target-skill="alpha" data-target-key="claude"[^>]* aria-pressed="true"/);
  assert.doesNotMatch(list.innerHTML,/data-global-add-target-skill="alpha" data-target-key="agents"[^>]* aria-pressed="true"/);
  const writes=[];
  const select=f.api.globalSelect;f.api.globalSelect=async body=>{writes.push(JSON.parse(JSON.stringify(body)));return select(body);};
  f.api.globalApply=async id=>{for(const t of f.model.skills.find(s=>s.id===id).targets)f.model.deployed.push({resource_id:id,tool:t==='agents'?'codex':t});return {skipped_conflicts:[]};};
  selectCandidate(body,'alpha');selectCandidate(body,'beta');
  assert.equal(writes.length,0,'确认前不能写入');
  assert.match(body.querySelector('[data-global-selected-count]').textContent,/2 项/);
  await body.querySelector('[data-global-add-submit]').onclick();
  assert.deepEqual(writes.map(w=>[w.skill,w.targets]),[['alpha',['claude']],['beta',['agents']]]);
  assert.match(f.get('[data-list]').innerHTML,/data-global-target-skill="beta" data-target-key="agents"[^>]* aria-pressed="true"/);
  f.model.skills.push({id:'fresh',name:'fresh',global:false});
  choose('fresh','claude',true);
  f.get('[data-add-skill]').onclick();
  assert.doesNotMatch(list.innerHTML,/data-global-add-target-skill="fresh" data-target-key="claude"[^>]* aria-pressed="true"/);
});

test('主列表调整单个 Skill 的工具并单项应用，取消最后一个工具时移除',async()=>{
  const f=await fixture({targets:[{key:'claude',label:'Claude Code',supported:true,enabled:true},{key:'agents',supported:true,enabled:true}],skills:[{id:'alpha',name:'alpha',global:true,targets:['claude']},{id:'beta',name:'beta',global:true,targets:['agents']}],deployed:[{resource_id:'alpha'},{resource_id:'beta'}]});
  const box={dataset:{globalTargetSkill:'alpha',targetKey:'agents'},checked:true,focus(){}};f.targetBoxes.push(box);
  await f.get('[data-refresh]').onclick();
  const writes=[],select=f.api.globalSelect;
  f.api.globalSelect=async body=>{writes.push(JSON.parse(JSON.stringify(body)));return select(body);};
  const applied=[];f.api.globalApply=async id=>{applied.push(id);f.model.deployed=f.model.deployed.filter(d=>d.resource_id!==id).concat(f.model.skills.find(s=>s.id===id).targets.map(t=>({resource_id:id,tool:t==='agents'?'codex':t})));return {skipped_conflicts:[]};};
  await box.onclick();assert.deepEqual(writes[0].targets,['claude','agents']);
  box.dataset.targetKey='claude';box.checked=false;await box.onclick();assert.deepEqual(writes[1].targets,['agents']);
  box.dataset.targetKey='agents';box.checked=false;await box.onclick();
  assert.equal(writes[2].enabled,false);assert.equal('targets' in writes[2],false);
  assert.deepEqual(applied,['alpha','alpha','alpha']);
  assert.deepEqual(f.model.skills[1].targets,['agents']);
  assert.equal(f.model.skills[1].global,true);
});

test('技能声明的限制禁用不支持的工具；只部署部分目标不报成功',async()=>{
  const targets=[{key:'claude',label:'Claude Code',supported:true,enabled:true},{key:'agents',supported:true,enabled:true}];
  const f=await fixture({targets,skills:[{id:'restricted',name:'restricted',global:false,available_targets:['claude']}],deployed:[]});
  f.get('[data-add-skill]').onclick();const list=f.dialogs[0].content.querySelector('[data-global-add-list]');
  assert.match(list.innerHTML,/data-target-key="agents"[^>]*aria-pressed="false"\s+disabled/);
  assert.match(list.innerHTML,/不支持/);
  let requested;const select=f.api.globalSelect;f.api.globalSelect=async body=>{requested=body.targets;return select(body);};
  f.api.globalApply=async id=>{f.model.deployed=[{resource_id:id,tool:'claude'}];return {skipped_conflicts:[]};};
  assert.equal(await addCandidate(f.dialogs[0].content,'restricted'),true);
  assert.deepEqual(Array.from(requested),['claude']);
  const partial=await fixture({targets,skills:[{id:'partial',name:'partial',global:false}],deployed:[]});
  partial.get('[data-add-skill]').onclick();
  partial.api.globalApply=async id=>{partial.model.deployed=[{resource_id:id,tool:'claude'}];return {skipped_conflicts:[]};};
  assert.equal(await addCandidate(partial.dialogs[0].content,'partial'),false);
  assert.match(partial.get('[data-list]').innerHTML,/部分开启/);
  assert.match(partial.get('[data-status]').textContent,/尚未全部部署/);
  assert.equal(partial.notices.length,0);
});

test('takeover failure retains confirmation and actionable error after successful reload; retry can succeed',async()=>{
  const f=await fixture();
  f.takeover.onclick();
  const dialog=f.dialogs[0], confirm=dialog.actions[1];
  assert.equal(await confirm.onAction(),false,'Dialog keeps the confirmation open on failure');
  assert.equal(f.reads(),2,'state was reloaded after the rejected write');
  assert.match(f.get('[data-status]').textContent,/磁盘已满/);
  const alert=dialog.content.children[1];
  assert.equal(alert.role,'alert');assert.match(alert.textContent,/磁盘已满/);
  assert.equal(f.notices.length,0,'no false success notification');
  f.api.globalTakeover=async()=>({});
  assert.equal(await confirm.onAction(),true,'successful retry closes confirmation');
  assert.equal(f.notices.length,1);
  assert.equal(f.get('[data-status]').textContent,'','successful retry clears stale error');
});

test('cancel does not take over and cannot dismiss an in-flight takeover',async()=>{
  const f=await fixture();f.takeover.onclick();
  const dialog=f.dialogs[0];
  assert.equal(dialog.actions[0].onAction(),true);
  assert.equal(f.takeoverCalls(),0);
  let release;
  f.api.globalTakeover=()=>new Promise(resolve=>{release=resolve;});
  const pending=dialog.actions[1].onAction();
  assert.equal(dialog.canClose(),false);
  assert.equal(dialog.actions[0].onAction(),false);
  release({});assert.equal(await pending,true);assert.equal(dialog.canClose(),true);
});

test('restore and apply errors survive the follow-up state reload',async()=>{
  const f=await fixture();f.restore.onclick();await f.flush();
  assert.match(f.get('[data-status]').textContent,/原位置已被占用/);
  await f.get('[data-apply]').onclick();
  assert.match(f.get('[data-status]').textContent,/全局应用失败/);
});

test('refresh failure preserves the write error and does not report success',async()=>{
  const f=await fixture();f.takeover.onclick();
  f.api.globalSkills=async()=>{throw new Error('读取失败 fixture');};
  assert.equal(await f.dialogs[0].actions[1].onAction(),false);
  const text=f.get('[data-status]').textContent;
  assert.match(text,/磁盘已满/);assert.match(text,/读取失败 fixture/);
  assert.equal(f.notices.length,0);
});
