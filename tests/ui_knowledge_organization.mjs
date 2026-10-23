// Browser-only API fixtures: never invoke a real CLI or write knowledge files.
import {writeFile} from 'node:fs/promises';

export async function verifyKnowledgeOrganization({evaluate,waitFor,send,screenshot,output}) {
  await evaluate(String.raw`(async()=>{
    const {api}=await import('/ui/services/api.js');
    const root='/fixture/projects/knowledge-organization-with-a-long-directory-name';
    const injection='<img src=x onerror="window.knowledgeInjection=true">';
    const fixture=window.knowledgeOrganization={
      root,injection,initialized:true,sourceCount:3,
      report:'# 已保存的维护建议\n\n旧报告保留原始记录与出处。\n'+injection,
      runners:[{id:'codex',name:'Codex',available:true,executable:'/fixture/bin/codex'},{id:'claude',name:'Claude Code',available:true,executable:'/fixture/bin/claude'},{id:'pi',name:'Pi',available:false,executable:null},{id:'cursor',name:'Cursor',available:true,executable:'/fixture/bin/agent'}],
      calls:{status:0,start:[],poll:[],cancel:[],checkpoint:0},jobs:[],latest:null,
      failStatus:0,failStart:0,failPoll:0,failCancel:0,holdStart:false,holdStatus:false,holdCancel:false,
    };
    fixture.finish=(id,status,report,saved=false)=>{
      const job=fixture.jobs.find(j=>j.id===id);
      Object.assign(job,{status,updated_at:new Date().toISOString(),result:{knowledge_id:'fixture-knowledge',report_key:'maintenance/recommendations',runner:job.runner,source_count:fixture.sourceCount,report,saved,report_path:'/fixture/Knowledge/maintenance/recommendations.md'}});
      if(status==='failed')job.error='保存失败：只读目录 '+injection;
      if(saved)fixture.report=report;
    };
    api.state=async()=>({repos:[{repo_id:'knowledge-fixture',common_dir:root+'/.git',project:{name:'项目知识整理验收'},worktrees:{main:{path:root,status:'active',branch:'refs/heads/main'}}}]});
    api.approveDir=async()=>({});
    api.knowledge=async body=>{
      if(body.root!==root)throw Error('Unexpected fixture root');
      if(body.action==='inspect')return {git:null};
      if(body.action==='status')return {initialized:fixture.initialized,files:12,location:fixture.initialized?{path:'/fixture/Knowledge'}:null,default_path:'/fixture/Knowledge'};
      if(body.action==='checkpoint'){fixture.calls.checkpoint++;return {};}
      if(body.action==='organization-status'){
        fixture.calls.status++;
        if(fixture.holdStatus)await new Promise(resolve=>fixture.releaseStatus=resolve);
        if(fixture.failStatus-- >0)throw Error('读取整理状态失败 '+injection);
        return structuredClone({knowledge_id:'fixture-knowledge',source_count:fixture.sourceCount,report_key:'maintenance/recommendations',report_path:'/fixture/Knowledge/maintenance/recommendations.md',report:fixture.report,runners:fixture.runners,latest_job:fixture.latest});
      }
      if(body.action==='organize'){
        fixture.calls.start.push(body);
        if(fixture.holdStart)await new Promise(resolve=>fixture.releaseStart=resolve);
        if(fixture.failStart-- >0)throw Error('启动工具失败 '+injection);
        const id='knowledge-job-'+fixture.calls.start.length;
        const job={id,kind:'knowledge_organize',root,runner:body.runner,status:'queued',progress:['等待执行'],created_at:new Date().toISOString(),updated_at:new Date().toISOString()};
        fixture.jobs.unshift(job);fixture.latest=job;
        return {job_id:id};
      }
      throw Error('Unexpected knowledge fixture action: '+body.action);
    };
    api.job=async id=>{
      fixture.calls.poll.push(id);
      if(fixture.failPoll-- >0)throw Error('进度连接暂时断开 '+injection);
      const job=fixture.jobs.find(j=>j.id===id);if(!job)throw Error('Unknown job');
      return structuredClone(job);
    };
    api.cancelJob=async id=>{
      fixture.calls.cancel.push(id);
      if(fixture.holdCancel)await new Promise(resolve=>fixture.releaseCancel=resolve);
      if(fixture.failCancel-- >0)throw Error('取消请求失败 '+injection);
      fixture.finish(id,'cancelled',null);return {};
    };
    api.jobs=async()=>structuredClone({jobs:fixture.jobs});
    sessionStorage.removeItem('ailoom-return-after-settings');sessionStorage.removeItem('ailoom-directory-view');
    location.hash='#/projects/knowledge-fixture/knowledge';
  })()`);

  async function assert(expression,message) {
    if(!await evaluate(expression))throw Error(message);
  }
  async function detect() {
    const before=await evaluate('window.knowledgeOrganization.calls.status');
    await evaluate("document.querySelector('[data-organize-detect]').click()");
    await waitFor(`window.knowledgeOrganization.calls.status===${before+1} && !document.querySelector('[data-organize-detect]').disabled`);
  }
  async function visitKnowledge() {
    await evaluate("location.hash='#/projects/knowledge-fixture/knowledge'");
    await waitFor("!!document.querySelector('[data-organize-start]') && !!document.querySelector('[data-organize-count]')");
  }
  async function visitHistory() {
    await evaluate("location.hash='#/tasks'");
    await waitFor("!!document.querySelector('.tasks-page [data-job]')");
  }
  async function start() {
    const before=await evaluate('window.knowledgeOrganization.calls.start.length');
    await evaluate("document.querySelector('[data-organize-start]').click()");
    await waitFor(`window.knowledgeOrganization.calls.start.length===${before+1} && !document.querySelector('[data-organize-cancel]').hidden`);
    return await evaluate('window.knowledgeOrganization.latest.id');
  }
  async function noInjection() {
    await assert("!window.knowledgeInjection && !document.querySelector('.knowledge-organization img,.tasks-page img')",'Report, progress or error text was interpreted as HTML');
  }
  const layouts=[];
  async function capture(name,width,anchor=null) {
    await send('Emulation.setDeviceMetricsOverride',{width,height:width<=700?844:1000,deviceScaleFactor:1,mobile:false});
    await evaluate('window.scrollTo(0,0)');
    await new Promise(resolve=>setTimeout(resolve,120));
    const layout=await evaluate(`(()=>{
      const rect=e=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom};};
      const visible=e=>e.getClientRects().length&&getComputedStyle(e).visibility!=='hidden';
      return {width:innerWidth,scroll:document.documentElement.scrollWidth,controls:[...document.querySelectorAll('.knowledge-organization button,.knowledge-organization select,.knowledge-organization textarea,.tasks-page [data-job-open]')].filter(visible).map(e=>({text:e.textContent.trim(),...rect(e)})),reports:[...document.querySelectorAll('.knowledge-organization-report')].filter(visible).map(e=>({...rect(e),scroll:e.scrollWidth,client:e.clientWidth,text:e.textContent}))};
    })()`);
    if(layout.scroll>width+1)throw Error(name+' overflows '+width+': '+JSON.stringify(layout));
    if(layout.controls.some(c=>c.x<0||c.right>width||c.width<1||c.height<(width<=700?44:30)))throw Error(name+' has clipped or undersized controls: '+JSON.stringify(layout));
    if(layout.reports.some(r=>r.x<0||r.right>width||r.width<80||r.scroll>r.client+1))throw Error(name+' has unreadable report layout: '+JSON.stringify(layout));
    if(anchor)await evaluate(`document.querySelector(${JSON.stringify(anchor)}).scrollIntoView({block:'start'})`);
    await screenshot(output+'-'+name+'-'+width+'.png');layouts.push({name,...layout});
  }

  await waitFor("document.querySelector('[data-organize-count]')?.textContent==='3 篇知识文档可整理'");
  await assert("document.querySelector('[data-organize-runner]').value==='codex' && !document.querySelector('[data-organize-start]').disabled",'Codex is not the available default runner');
  await assert("document.querySelectorAll('[data-organize-runner] option').length===4 && document.querySelector('[data-organize-runner] option[value=pi]').disabled && !document.querySelector('[data-organize-runner] option[value=claude]').disabled && !document.querySelector('[data-organize-runner] option[value=cursor]').disabled",'Runner detection does not expose installed alternatives only');
  await assert("!document.querySelector('.knowledge-location').open && document.querySelector('.knowledge-location summary').textContent==='保存位置与恢复配置'",'Location and recovery settings are not collapsed below organization');
  await assert("document.querySelector('[data-organize-report] pre').textContent===window.knowledgeOrganization.report && !document.querySelector('[data-organize-report]').hidden",'Previously saved report is missing');
  await noInjection();
  for(const width of [1440,390,320])await capture('ready',width);

  // Re-detection preserves user input, and multiple requests cannot overlap.
  await evaluate("document.querySelector('[data-organize-runner]').parentElement.querySelector('[role=combobox]').click()");
  await waitFor("!!document.querySelector('.knowledge-organization .select-menu:popover-open')");
  await assert("document.querySelector('.knowledge-organization [role=option][data-index=\"2\"]').getAttribute('aria-disabled')==='true'",'Unavailable Pi option remains selectable');
  await evaluate("document.querySelector('.knowledge-organization [role=option][data-index=\"1\"]').click()");
  await evaluate(`{const f=window.knowledgeOrganization;f.holdStatus=true;const focus=document.querySelector('[data-organize-focus]');focus.value='优先整理排障经验 '+f.injection;focus.dispatchEvent(new Event('input'));const detect=document.querySelector('[data-organize-detect]');detect.click();detect.click();void detect.onclick();}`);
  await waitFor("window.knowledgeOrganization.calls.status===2 && document.querySelector('[data-organize-start]').disabled && document.querySelector('[data-organize-detect]').disabled");
  await evaluate('window.knowledgeOrganization.holdStatus=false;window.knowledgeOrganization.releaseStatus()');
  await waitFor("!document.querySelector('[data-organize-detect]').disabled");
  await assert("document.querySelector('[data-organize-runner]').value==='claude' && document.querySelector('[data-organize-focus]').value==='优先整理排障经验 '+window.knowledgeOrganization.injection",'Re-detection lost runner or focus');
  await noInjection();

  // Keep creation pending to exercise native double clicks and the handler guard.
  await evaluate("{const f=window.knowledgeOrganization;f.holdStart=true;const b=document.querySelector('[data-organize-start]');b.click();b.click();void b.onclick();}");
  await waitFor("window.knowledgeOrganization.calls.start.length===1 && document.querySelector('[data-organize-start]').disabled");
  await assert("document.querySelector('[data-organize-runner]').disabled && document.querySelector('[data-organize-focus]').disabled && document.querySelector('[data-organize-detect]').disabled",'Task creation permits editing or another detection');
  await assert("(()=>{const f=window.knowledgeOrganization,b=f.calls.start[0];return b.root===f.root&&b.action==='organize'&&b.runner==='claude'&&b.focus==='优先整理排障经验 '+f.injection&&/^[a-f0-9-]{36}$/.test(b.idempotency_key)})()",'Start omitted runner, focus, root or idempotency token');
  await evaluate('window.knowledgeOrganization.holdStart=false;window.knowledgeOrganization.releaseStart()');
  await waitFor("window.knowledgeOrganization.calls.poll.length>=1 && !document.querySelector('[data-organize-cancel]').hidden");
  const first=await evaluate('window.knowledgeOrganization.latest.id');
  await evaluate("window.knowledgeOrganization.latest.status='running';window.knowledgeOrganization.latest.progress=['正在提炼仓库约定 '+window.knowledgeOrganization.injection];window.knowledgeOrganization.failPoll=1");
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('暂时无法读取进度')");
  await assert("document.querySelector('[data-organize-start]').disabled && !document.querySelector('[data-organize-cancel]').hidden",'Polling failure hides the running task or allows another start');
  await noInjection();
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('正在提炼仓库约定')");

  // Route remount recovers an active task from organization-status and resumes polling.
  await visitHistory();
  await assert("document.querySelector('[data-job]').textContent.includes('整理知识库') && document.querySelector('[data-job]').textContent.includes('项目知识整理验收') && document.querySelector('[data-job]').textContent.includes('执行中') && document.querySelector('[data-undo-actions]').hidden",'History does not describe the active knowledge task');
  const polled=await evaluate('window.knowledgeOrganization.calls.poll.length');
  await visitKnowledge();
  await waitFor(`window.knowledgeOrganization.calls.poll.length>${polled} && document.querySelector('[data-organize-start]').disabled && !document.querySelector('[data-organize-progress]').hidden`);
  await evaluate("document.querySelector('.knowledge-location').open=true;document.querySelector('[data-checkpoint]').click()");
  await waitFor("window.knowledgeOrganization.calls.checkpoint===1 && document.querySelector('.knowledge-location [data-message]').textContent.includes('恢复配置已保存')");
  await assert("document.querySelector('[data-organize-start]').disabled && document.querySelector('[data-organize-runner]').disabled && document.querySelector('[data-organize-focus]').disabled && !document.querySelector('[data-organize-cancel]').disabled",'Saving a recovery checkpoint overrides active-task controls');
  await evaluate("document.querySelector('.knowledge-location').open=false");
  await capture('running',390);
  await evaluate(`window.knowledgeOrganization.finish(${JSON.stringify(first)},'success',${JSON.stringify('# 本次已保存的维护建议\n\n1. 保留出处并补充回归测试。\n')}+window.knowledgeOrganization.injection+'\\n'+('VeryLongUnbrokenRecommendation'.repeat(8)),true)`);
  await waitFor("document.querySelector('[data-organize-report] pre')?.textContent===window.knowledgeOrganization.report && document.querySelector('[data-organize-message]').textContent.includes('整理完成') && !document.querySelector('[data-organize-start]').disabled");
  await assert("document.querySelector('[data-organize-cancel]').hidden && document.querySelector('[data-organize-progress]').hidden && document.querySelector('[data-organize-draft]').hidden",'Successful task retained running controls or an unsaved draft');
  await noInjection();
  for(const width of [1440,390,320])await capture('success',width);
  await capture('success-report',320,'[data-organize-report]');

  // Start and re-detect failures remain retryable and keep the saved report.
  await evaluate("window.knowledgeOrganization.failStart=1;document.querySelector('[data-organize-start]').click()");
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('启动工具失败') && !document.querySelector('[data-organize-start]').disabled");
  await assert("document.querySelector('[data-organize-report] pre').textContent===window.knowledgeOrganization.report",'Start failure discarded the saved report');
  await evaluate('window.knowledgeOrganization.failStatus=1');await detect();
  await assert("document.querySelector('[data-organize-message]').textContent.includes('读取整理状态失败') && !document.querySelector('[data-organize-start]').disabled",'Status failure cannot recover');
  await detect();await noInjection();

  // Failure exposes its unsaved draft separately and never replaces the saved report.
  const failed=await start();
  await evaluate(`window.knowledgeOrganization.finish(${JSON.stringify(failed)},'failed',${JSON.stringify('# 未保存的草稿\n\n')}+window.knowledgeOrganization.injection)`);
  await waitFor("!document.querySelector('[data-organize-draft]').hidden && document.querySelector('[data-organize-message]').textContent.includes('整理失败') && !document.querySelector('[data-organize-start]').disabled");
  await assert("document.querySelector('[data-organize-report] pre').textContent===window.knowledgeOrganization.report && document.querySelector('[data-organize-draft] pre').textContent.includes('未保存的草稿')",'Failed draft replaced the stable saved report');
  await evaluate("document.querySelector('[data-organize-draft]').open=true");
  await noInjection();await capture('failed-draft',390,'[data-organize-draft]');

  // Cancellation can retry after failure and sends only one request while pending.
  const cancelled=await start();
  await evaluate("window.knowledgeOrganization.failCancel=1;document.querySelector('[data-organize-cancel]').click()");
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('取消请求失败') && !document.querySelector('[data-organize-cancel]').disabled");
  await evaluate("{const f=window.knowledgeOrganization;f.holdCancel=true;const b=document.querySelector('[data-organize-cancel]');b.click();b.click();void b.onclick();}");
  await waitFor("window.knowledgeOrganization.calls.cancel.length===2 && document.querySelector('[data-organize-cancel]').disabled");
  await evaluate('window.knowledgeOrganization.holdCancel=false;window.knowledgeOrganization.releaseCancel()');
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('已取消') && document.querySelector('[data-organize-cancel]').hidden && !document.querySelector('[data-organize-start]').disabled");
  await assert(`window.knowledgeOrganization.calls.cancel.every(id=>id===${JSON.stringify(cancelled)}) && document.querySelector('[data-organize-report] pre').textContent===window.knowledgeOrganization.report && document.querySelector('[data-organize-draft]').hidden`,'Cancellation changed the saved report or targeted another task');

  // An initial status error has a usable retry button.
  await visitHistory();
  await evaluate("window.knowledgeOrganization.failStatus=1;location.hash='#/projects/knowledge-fixture/knowledge'");
  await waitFor("!!document.querySelector('[data-organize-retry]')");
  await assert("document.querySelector('[data-organize-message]').textContent.includes('读取整理状态失败')",'Initial status error is hidden');
  await evaluate("document.querySelector('[data-organize-retry]').click()");
  await waitFor("!!document.querySelector('[data-organize-start]') && !document.querySelector('[data-organize-start]').disabled");

  // Checkpoints cannot bypass the zero-document or missing-runner start gates.
  await evaluate('window.knowledgeOrganization.sourceCount=0');await detect();
  await assert("document.querySelector('[data-organize-start]').disabled && document.querySelector('.knowledge-organization').textContent.includes('配置恢复副本和资源快照不参与整理')",'Empty knowledge can start a task or counts recovery snapshots as notes');
  await evaluate("document.querySelector('.knowledge-location').open=true;document.querySelector('[data-checkpoint]').click()");
  await waitFor("window.knowledgeOrganization.calls.checkpoint===2 && document.querySelector('.knowledge-location [data-message]').textContent.includes('恢复配置已保存')");
  await assert("document.querySelector('[data-organize-start]').disabled",'Saving a recovery checkpoint enabled organization with zero knowledge documents');
  await evaluate('window.knowledgeOrganization.sourceCount=3;window.knowledgeOrganization.runners.forEach(r=>r.available=false)');await detect();
  await assert("document.querySelector('[data-organize-start]').disabled && [...document.querySelectorAll('[data-organize-runner] option')].every(o=>o.disabled)",'Missing local CLIs do not disable organization');
  await evaluate("document.querySelector('[data-checkpoint]').click()");
  await waitFor("window.knowledgeOrganization.calls.checkpoint===3 && document.querySelector('.knowledge-location [data-message]').textContent.includes('恢复配置已保存')");
  await assert("document.querySelector('[data-organize-start]').disabled",'Saving a recovery checkpoint enabled organization with no available CLI');
  await capture('no-cli',320);
  await evaluate('window.knowledgeOrganization.runners.find(r=>r.id===\'cursor\').available=true');await detect();
  await assert("document.querySelector('[data-organize-runner]').value==='cursor' && !document.querySelector('[data-organize-start]').disabled",'Re-detection cannot select an available alternative when Codex is absent');

  // Reports in history remain readable and disclose whether they were saved.
  await visitHistory();
  await evaluate(`document.querySelector('[data-job-open="${first}"]').click()`);
  await assert("document.querySelector('[data-detail-body]').textContent.includes('已保存到知识库') && document.querySelector('[data-detail-body] .knowledge-organization-report').textContent===window.knowledgeOrganization.report && document.querySelector('[data-undo-actions]').hidden",'Saved report is missing or offers an unrelated undo action');
  await evaluate(`document.querySelector('[data-job-open="${failed}"]').click()`);
  await assert("document.querySelector('[data-detail-body]').textContent.includes('本次结果尚未保存') && document.querySelector('[data-detail-body] .knowledge-organization-report').textContent.includes('未保存的草稿') && document.querySelector('[data-detail-body]').textContent.includes('失败')",'History hides failed drafts or their save status');
  await noInjection();
  for(const width of [1440,390,320])await capture('history-draft',width);
  await capture('history-report',390,'[data-detail-body] .knowledge-organization-report');
  await writeFile(output+'-layout.json',JSON.stringify(layouts,null,2)+'\n');
  const calls=await evaluate('window.knowledgeOrganization.calls');
  console.log(JSON.stringify({checks:'runner/focus, duplicate guards, detection, polling errors/remount, saved report refresh, unsaved failed draft, cancellation retry, initial/status/start errors, empty notes/missing CLI, escaped text, history reports',startRequests:calls.start.length,cancelRequests:calls.cancel.length,statusRequests:calls.status,layouts:layouts.length}));
}
