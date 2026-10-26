// Isolated API fixtures: never check or update the user's actual sources.
export async function verifyResourceUpdates({evaluate,waitFor,send,screenshot,output}) {
  await evaluate(`(async()=>{
    const {api}=await import('/ui/services/api.js');
    const names=['skills','agent-skills','chrome-devtools-mcp','opencli','playwright-cli','orca-skills','smallraw-skills'];
    const fixture=window.resourceUpdates={calls:[],writes:[],pending:null,fast:false,blocked:false};
    const removed='collection-opaque/skill/default/antigravity';
    fixture.sources=names.map((name,i)=>({id:'source-'+i,name:'CC Switch · '+name,url:'https://github.com/fixture/'+name,management:'managed',lock:{resolved_commit:'1111111111111111111111111111111111111111'},resources:[{id:'source-'+i+'/skill/default/item',name:'resource-'+i,kind:'skill'},...(i===0?[{id:removed,name:'antigravity',kind:'skill'}]:[])],references:[{repo_id:'app',scope:'仓库默认',resource_id:'source-'+i+'/skill/default/item'}],update:{state:'available',preview:{preview_id:'preview-'+i,source:{lock:{resolved_commit:'2222222222222222222222222222222222222222'}},resources:[{id:'source-'+i+'/skill/default/item'}],removed:i===0?[removed]:[]}}}));
    fixture.skills=Array.from({length:4},(_,i)=>({id:'personal/skill/personal/solo-'+i,name:'solo-'+i,kind:'skill',source:{repo_url:'https://github.com/fixture/solo',repo_path:'skills/solo-'+i,resolved_commit:'3333333333333333333333333333333333333333'},can_check_update:true,references:i<2?[{repo_id:i===0?null:'app',scope:i===0?'全局':'Worktree opaque-wt/apps/web',resource_id:'personal/skill/personal/solo-'+i}]:[],update:{state:'upstream-new',preview_id:'solo-preview-'+i,upstream_commit:'4444444444444444444444444444444444444444'}}));
    fixture.skills[0].source={source_kind:'local',discovery_entry:'local-dir:/fixture/local-skills/solo-0',imported_digest:'5555555555555555555555555555555555555555'};fixture.skills[0].update.upstream_commit=null;fixture.skills[0].update.upstream_digest='6666666666666666666666666666666666666666';
    api.state=async()=>({repos:[{repo_id:'app',project:{name:'App 项目'},common_dir:'/fixture/app/.git',worktrees:{'opaque-wt':{path:'/fixture/app-feature',branch:'refs/heads/feature/updates',status:'active'}}}]});
    api.collections=async()=>({sources:structuredClone(fixture.sources).map((s,i)=>{if(i===0&&fixture.blocked)s.references.push({repo_id:'app',scope:'仓库默认',resource_id:removed});return s;})});
    api.libraryList=async()=>({entries:structuredClone(fixture.skills)});
    const check=(kind,id)=>{
      fixture.calls.push({kind,id});
      const result=error=>kind==='collection'?{items:[{state:error?'error':'available',error}]}:{status:{state:error?'error':'upstream-new',note:error}};
      if(fixture.fast)return Promise.resolve(result());
      return new Promise(resolve=>fixture.pending={kind,id,finish:error=>{fixture.pending=null;resolve(result(error));}});
    };
    api.collectionCheck=id=>check('collection',id);api.checkUpdate=id=>check('skill',id);
    api.collectionUpdate=async ids=>{fixture.writes.push({kind:'collection',ids});for(const s of fixture.sources)s.update.state='current';};
    api.updateSkill=async(name,execute,preview)=>{fixture.writes.push({kind:'skill',name,execute,preview});fixture.skills.find(e=>e.name===name).update.state='up-to-date';};
    location.hash='#/library';
  })()`);
  const assert=async(expression,message)=>{if(!await evaluate(expression))throw Error(message);};
  await waitFor("!!document.querySelector('[data-check]')&&!document.querySelector('[data-update]').disabled");
  await evaluate("document.querySelector('[data-check]').click()");
  await waitFor("window.resourceUpdates.pending?.kind==='collection'");
  await assert("document.querySelector('[data-check-bar]').value===0&&document.querySelector('[data-check-bar]').max===11&&document.querySelector('[data-check-current]').textContent.includes(window.resourceUpdates.sources.find(s=>s.id===window.resourceUpdates.pending.id).name)",'Initial progress does not identify the current source or full count');
  await assert("!document.querySelector('[data-search]').disabled&&!document.querySelector('[data-library-kind]').disabled&&document.querySelector('[data-update]').disabled&&!document.querySelector('[data-check-stop]').disabled",'Checking blocks browsing or allows a conflicting update');
  await evaluate("document.querySelector('[data-search]').value='solo-0';document.querySelector('[data-search]').dispatchEvent(new Event('input'));window.resourceUpdates.pending.finish()");
  await waitFor("window.resourceUpdates.calls.length===2&&!!window.resourceUpdates.pending");
  await assert("document.querySelector('[data-check-bar]').value===1&&document.querySelectorAll('[data-library-id]').length===1",'Filtering changed the running scope or did not remain usable');
  await send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:false});
  await screenshot(output+'-progress-390.png');
  await evaluate("document.querySelector('[data-check-stop]').click()");
  await assert("document.querySelector('[data-check-current]').textContent.includes('随后停止')",'Stopping promises to abort the in-flight source');
  await evaluate("window.resourceUpdates.pending.finish('网络失败 <img src=x onerror=window.updateInjection=true>')");
  await waitFor("document.querySelector('[data-check-current]').textContent==='检查已停止'");
  await assert("window.resourceUpdates.calls.length===2&&document.querySelector('[data-check-count]').textContent.includes('已完成 2 / 11')&&document.querySelector('[data-check-count]').textContent.includes('失败 1')&&!document.querySelector('[data-check-progress] img')&&!window.updateInjection",'Stopped or failed progress is incorrect or unsafe');
  await evaluate("document.querySelector('[data-search]').value='';document.querySelector('[data-search]').dispatchEvent(new Event('input'));window.resourceUpdates.fast=true;document.querySelector('[data-check]').click()");
  await waitFor("document.querySelector('[data-check-current]').textContent==='检查完成'&&document.querySelector('[data-check-bar]').value===11");
  await evaluate("document.querySelector('[data-update]').click()");
  await waitFor("!!document.querySelector('dialog:modal .library-update-preview')");
  await assert("document.querySelector('[data-update-summary]').textContent.includes('7 个合集和4 个独立 Skill')&&document.querySelectorAll('[data-update-collection]').length===7&&document.querySelectorAll('[data-update-skill]').length===4",'Mixed update hides selected items or mislabels the counts');
  await assert("document.querySelector('[data-update-removed]').textContent.includes('antigravity')&&!document.querySelector('[data-update-removed]').textContent.includes('collection-opaque')&&document.querySelector('.library-update-preview').textContent.includes('9 条资源引用')&&document.querySelector('.library-update-preview').textContent.includes('已启用位置（3 处）')",'Removed resources or usage counts are opaque or incomplete');
  await assert("document.querySelector('.library-update-preview').textContent.includes('App 项目')&&document.querySelector('.library-update-preview').textContent.includes('feature/updates/apps/web')&&!document.querySelector('.library-update-preview').textContent.includes('opaque-wt')&&document.querySelectorAll('.library-update-preview code').length===22",'Project, Worktree or version labels are missing');
  await assert("(()=>{const e=[...document.querySelectorAll('[data-update-skill]')].find(e=>e.dataset.updateSkill===window.resourceUpdates.skills[0].id);return e.textContent.includes('本地文件夹 · /fixture/local-skills/solo-0')&&e.textContent.includes('555555555555')&&e.textContent.includes('666666666666');})()",'Local Skill updates hide their source or content versions');
  for(const width of [1440,390,320]) {
    await send('Emulation.setDeviceMetricsOverride',{width,height:width>700?1000:844,deviceScaleFactor:1,mobile:false});
    await evaluate("document.querySelector('dialog:modal .dialog-body').scrollTop=0");
    const layout=await evaluate(`(()=>{const d=document.querySelector('dialog:modal'),r=d.getBoundingClientRect(),b=d.querySelector('.dialog-body'),f=d.querySelector('.dialog-actions').getBoundingClientRect();return {left:r.left,right:r.right,bottom:r.bottom,footerBottom:f.bottom,scroll:b.scrollWidth,client:b.clientWidth};})()`);
    if(layout.left<0||layout.right>width||layout.bottom>(width>700?1000:844)||layout.footerBottom>layout.bottom||layout.scroll>layout.client+1)throw Error('Update preview clips its content or actions: '+JSON.stringify(layout));
    await screenshot(output+'-confirmation-'+width+'.png');
    await evaluate("[...document.querySelectorAll('[data-update-skill]')].at(-1).scrollIntoView({block:'nearest'})");
    await assert("(()=>{const b=document.querySelector('dialog:modal .dialog-body'),r=b.getBoundingClientRect(),s=[...document.querySelectorAll('[data-update-skill]')].at(-1).getBoundingClientRect();return b.scrollTop>0&&s.top>=r.top-1&&s.bottom<=r.bottom+1;})()",'Independent Skills cannot be reached by scrolling');
  }
  await evaluate("[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(b=>b.textContent==='取消').click()");
  await waitFor("!document.querySelector('dialog:modal')&&!document.querySelector('[data-update]').disabled");
  await assert("window.resourceUpdates.writes.length===0",'Cancel writes updates');
  await evaluate("window.resourceUpdates.blocked=true;document.querySelector('[data-check]').click()");
  await waitFor("document.querySelector('[data-check-current]').textContent==='检查完成'&&!document.querySelector('[data-update]').disabled");
  await evaluate("document.querySelector('[data-update]').click()");
  await waitFor("!!document.querySelector('[data-update-blocked]')");
  await assert("document.querySelectorAll('dialog:modal .dialog-actions button').length===1&&document.querySelector('[data-update-removed]').textContent.includes('仍有启用引用')&&window.resourceUpdates.writes.length===0",'Known removed references still allow a failing update');
  await evaluate("document.querySelector('dialog:modal .dialog-actions button').click();window.resourceUpdates.blocked=false");
  await waitFor("!document.querySelector('dialog:modal')&&!document.querySelector('[data-check]').disabled");
  await evaluate("document.querySelector('[data-check]').click()");
  await waitFor("document.querySelector('[data-check-current]').textContent==='检查完成'&&!document.querySelector('[data-update]').disabled");
  await evaluate("document.querySelector('[data-update]').click()");
  await waitFor("!!document.querySelector('dialog:modal .library-update-preview')");
  await evaluate("[...document.querySelectorAll('dialog:modal .dialog-actions button')].find(b=>b.textContent==='确认更新').click()");
  await waitFor("document.querySelector('[data-msg]').textContent.includes('资源库已更新')");
  await assert("window.resourceUpdates.writes.length===5&&window.resourceUpdates.writes[0].ids.length===7&&window.resourceUpdates.writes.filter(w=>w.kind==='skill').every(w=>w.execute&&w.preview.startsWith('solo-preview-'))",'Confirmation does not apply exactly the reviewed snapshots');
  await evaluate("window.resourceUpdates.fast=false;document.querySelector('[data-check]').click()");
  await waitFor("!!window.resourceUpdates.pending");
  await evaluate("window.resourceUpdates.callsAtLeave=window.resourceUpdates.calls.length;location.hash='#/projects'");
  await waitFor("!document.querySelector('[data-check-progress]')");
  await evaluate("window.resourceUpdates.pending.finish()");
  await new Promise(resolve=>setTimeout(resolve,40));
  await assert("window.resourceUpdates.calls.length===window.resourceUpdates.callsAtLeave",'Leaving the page keeps checking subsequent items');

}
