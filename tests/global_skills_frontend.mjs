// Node component-state regression; real browser screenshots are separate evidence.
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';

async function fixture() {
  const elements = new Map();
  const get = selector => {
    if (!elements.has(selector)) elements.set(selector, {textContent:'',innerHTML:'',dataset:{}});
    return elements.get(selector);
  };
  const takeover = {dataset:{takeover:'0'}}, restore = {dataset:{restore:'0'}};
  const section = {
    set innerHTML(_) {}, append() {}, remove() {},
    querySelector:get,
    querySelectorAll:selector => selector==='[data-takeover]'?[takeover]:selector==='[data-restore]'?[restore]:[],
  };
  const document = {body:{},createElement:tag => tag==='section'?section:{tag,children:[],textContent:'',append(...nodes){this.children.push(...nodes);},setAttribute(key,value){this[key]=value;}}};
  const dialogs=[], notices=[];
  let reads=0, takeoverCalls=0;
  const api = {
    globalSkills:async()=>{reads++;return {home:'/fixture',notes:[],skills:[],targets:[],foreign:[{name:'sample',target:'claude',path:'/fixture/claude/skills/sample',conflicts_with:'x',location:{kind:'dir'}}],archive:[{id:'archive-1'}]};},
    globalTakeover:async()=>{takeoverCalls++;throw new Error('无法写入归档：磁盘已满');},
    globalRestore:async()=>{throw new Error('原位置已被占用');},
    globalPlan:async()=>({actions:[{action:'create',path:'claude/skills/sample'}]}),
    globalApply:async()=>{throw new Error('全局应用失败');},
  };
  const context=vm.createContext({document});
  const module=new vm.SourceTextModule(await readFile('src/console/ui/features/globalSkills.js','utf8'),{context});
  await module.link(name=>{
    const values=name.includes('/services/')?{api,esc:String}:name.includes('/components/')?{
      Dialog:(_,props)=>{dialogs.push(props);},confirmAction:async()=>true,
    }:{notify:value=>notices.push(value)};
    return new vm.SyntheticModule(Object.keys(values),function(){for(const [key,value]of Object.entries(values))this.setExport(key,value);},{context});
  });
  await module.evaluate();module.namespace.GlobalSkills({append(){}});
  const flush=()=>new Promise(resolve=>setImmediate(resolve));await flush();
  return {api,get,takeover,restore,dialogs,notices,flush,reads:()=>reads,takeoverCalls:()=>takeoverCalls};
}

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
