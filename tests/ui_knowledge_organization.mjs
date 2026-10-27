// Browser-only API fixtures: never invoke a real CLI or write knowledge files.
import {writeFile} from 'node:fs/promises';

export async function verifyKnowledgeOrganization({evaluate,waitFor,send,screenshot,output}) {
  await evaluate(String.raw`(async()=>{
    const {api}=await import('/ui/services/api.js');
    const root='/fixture/projects/knowledge-organization-with-a-long-directory-name';
    const injection='<img src=x onerror="window.knowledgeInjection=true">';
    const fixture=window.knowledgeOrganization={
      root,injection,initialized:true,sourceCount:3,needsSeparation:false,path:'/fixture/Knowledge',
      report:'# 已保存的维护建议\n\n旧报告保留原始记录与出处。\n'+injection,
      runners:[{id:'codex',name:'Codex',available:true,executable:'/fixture/bin/codex'},{id:'claude',name:'Claude Code',available:true,executable:'/fixture/bin/claude'},{id:'pi',name:'Pi',available:false,executable:null},{id:'cursor',name:'Cursor',available:true,executable:'/fixture/bin/agent'}],
      calls:{status:0,start:[],poll:[],cancel:[],checkpoint:0,pages:[],separate:[],move:[]},jobs:[],latest:null,
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
      if(body.action==='ingestion-status')return {sources:[],document_count:0,revision_count:0,knowledge_path:'/fixture/Knowledge'};
      if(body.action==='status')return {initialized:fixture.initialized,files:12,location:fixture.initialized?{path:fixture.path}:null,needs_separation:fixture.needsSeparation,default_path:fixture.needsSeparation?'/fixture/IndependentKnowledge':'/fixture/Knowledge',...(fixture.repository?{repository_path:fixture.repository,directory_name:fixture.directoryName||'app-monorepo'}:{})};
      if(body.action==='move'){fixture.calls.move.push(body);return {preview:true,expected:'move-preview',to:body.repository_path?body.repository_path+'/'+(fixture.directoryName||'app-monorepo'):body.path,files:12,affected_projects:['knowledge-fixture']};}
      if(body.action==='separate'){fixture.calls.separate.push(body);if(!body.execute)return {preview:true,expected:'split-preview',to:body.path,files:12,affected_projects:['knowledge-fixture'],unassigned_sources:['unknown.md']};if(body.expected!=='split-preview')throw Error('Wrong preview token');fixture.needsSeparation=false;fixture.path=body.path;return {separated:true};}
      if(body.action==='checkpoint'){fixture.calls.checkpoint++;return {};}
      if(body.action==='organization-page'){fixture.calls.pages.push(body.page);return {path:body.page,body:'# 独立模块知识\n'+injection};}
      if(body.action==='organization-status'){
        fixture.calls.status++;
        if(fixture.holdStatus)await new Promise(resolve=>fixture.releaseStatus=resolve);
        if(fixture.failStatus-- >0)throw Error('读取整理状态失败 '+injection);
        return structuredClone({knowledge_id:'fixture-knowledge',source_count:fixture.sourceCount,report_key:'maintenance/recommendations',report_path:'/fixture/Knowledge/maintenance/recommendations.md',report:fixture.report,project_id:'app-project-id',project_name:'App '+injection,raw_path:'/fixture/Knowledge/imports/worktrees/app-project-id',pages:[{path:'wiki/app-project-id/editions/current/topics/topic-core.md',title:'核心模块 '+injection,kind:'topic'}],related_projects:[{repo_id:'sdk-project',name:'JS SDK '+injection,index_path:'/fixture/SDK/wiki/index.md',packages:[{package:'@example/sdk',required:'1.2.0'}]}],runners:fixture.runners,latest_job:fixture.latest});
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

  await waitFor("document.querySelector('[data-organize-count]')?.textContent.includes('已有 3 篇知识资料')");
    await waitFor("!!document.querySelector('[data-wiki-page]')");
  await assert("document.querySelector('[data-organize-project]').textContent.includes('App ') && !document.querySelector('[data-organize-related]')",'Project identity or dependency navigation is missing');
  await evaluate("document.querySelector('[data-wiki-page]').click()");
  await waitFor("document.querySelector('[data-organize-report] pre').textContent.includes('独立模块知识')");
  await assert("window.knowledgeOrganization.calls.pages.length===1 && !window.knowledgeInjection && !document.querySelector('.knowledge-organization img')",'Independent page is not loaded safely');
  await evaluate("document.querySelector('[data-wiki-index]').click()");
  await assert("document.querySelector('[data-organize-report] pre').textContent===window.knowledgeOrganization.report",'Project index is not restored');

  await assert("document.querySelector('[data-organize-runner]').value==='codex' && !document.querySelector('[data-organize-start]').disabled",'Codex is not the available default runner');
  await assert("document.querySelector('[data-organize-default]').textContent.includes('无需填写提示词或挑选文档') && !document.querySelector('[data-organize-options]').open && document.querySelector('[data-organize-focus]').value===''",'Default organization requires instructions or shows the supplemental input');
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
  await evaluate("document.querySelector('[data-organize-options] summary').click()");
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

  // Unresolved editorial issues retain every draft and show a review state, not success/failure.
  const pending=await start();
  await evaluate(String.raw`{const f=window.knowledgeOrganization;f.finish(${JSON.stringify(pending)},'success','# 待修订草稿\n有效知识仍保留。'+f.injection);Object.assign(f.latest.result,{needs_revision:true,warning:'引用仍待核对 '+f.injection,repair_attempts:2,repair_history:[{report:'# 原始草稿\n'+f.injection,diagnostic:{message:'具体引用需要核对 '+f.injection}}]});}`);
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('整理稿待修订') && !document.querySelector('[data-organize-start]').disabled");
  await assert("!document.querySelector('[data-organize-draft]').hidden && document.querySelector('[data-organize-report] pre').textContent===window.knowledgeOrganization.report && document.querySelector('[data-organize-revisions]').textContent.includes('原始草稿')",'Review state discarded the draft/history or replaced the saved report');
  await noInjection();
  await visitHistory();
  await evaluate(`document.querySelector('[data-job-open="${pending}"]').click()`);
  await assert("document.querySelector('[data-detail-body] .badge').textContent==='待修订' && document.querySelector('[data-detail-body] .badge').classList.contains('warn') && document.querySelector('[data-detail-body]').textContent.includes('引用仍待核对') && document.querySelector('[data-detail-body]').textContent.includes('原始草稿')",'Task history mislabels a retained draft or hides the diagnostic');
  await noInjection();
  await visitKnowledge();
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('整理稿待修订')");

  // Cancellation can retry after failure and sends only one request while pending.
  const cancelled=await start();
  await evaluate("window.knowledgeOrganization.failCancel=1;document.querySelector('[data-organize-cancel]').click()");
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('取消请求失败') && !document.querySelector('[data-organize-cancel]').disabled");
  await evaluate("{const f=window.knowledgeOrganization;f.holdCancel=true;const b=document.querySelector('[data-organize-cancel]');b.click();b.click();void b.onclick();}");
  await waitFor("window.knowledgeOrganization.calls.cancel.length===2 && document.querySelector('[data-organize-cancel]').disabled");
  await evaluate('window.knowledgeOrganization.holdCancel=false;window.knowledgeOrganization.releaseCancel()');
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('已取消') && document.querySelector('[data-organize-cancel]').hidden && !document.querySelector('[data-organize-start]').disabled");
  await assert(`window.knowledgeOrganization.calls.cancel.every(id=>id===${JSON.stringify(cancelled)}) && document.querySelector('[data-organize-report] pre').textContent===window.knowledgeOrganization.report && document.querySelector('[data-organize-draft]').hidden`,'Cancellation changed the saved report or targeted another task');

  const partial=await start();
  await evaluate(String.raw`{const f=window.knowledgeOrganization;f.finish(${JSON.stringify(partial)},'cancelled','# 中途收到的正文\n保留内容。'+f.injection);f.latest.result.report_partial=true;}`);
  await waitFor("!document.querySelector('[data-organize-draft]').hidden && !document.querySelector('[data-organize-start]').disabled");
  await assert("document.querySelector('[data-organize-draft] summary').textContent.includes('未完成') && document.querySelector('[data-organize-draft] pre').textContent.includes('中途收到的正文') && document.querySelector('[data-organize-report] pre').textContent===window.knowledgeOrganization.report",'Cancellation discarded received body or published an incomplete report');
  await assert("document.querySelector('[data-organize-runtime]').textContent.includes('先规划') && document.querySelector('[data-organize-runtime]').textContent.includes('120 分钟')",'Planning and runtime budget are not visible');
  await noInjection();
  await visitHistory();
  await evaluate(`document.querySelector('[data-job-open="${partial}"]').click()`);
  await assert("document.querySelector('[data-detail-body]').textContent.includes('未完成的正文草稿')",'History hides the incomplete status');
  await noInjection();await visitKnowledge();

  // Budget handoff is resumable and distinct from a failure or published knowledge.
  const paused=await start();
  await evaluate(String.raw`{const f=window.knowledgeOrganization;f.finish(${JSON.stringify(paused)},'success','# 已保留的阶段稿\n模块甲已有证据。'+f.injection);Object.assign(f.latest.result,{needs_continuation:true,report_partial:true,warning:'本轮已收尾 '+f.injection,checkpoint:{plan:'先甲后乙 '+f.injection,batches:[{title:'模块甲 '+f.injection,scope:'代码与来源'},{title:'模块乙',scope:'剩余资料'}],completed:['# 已返回阶段稿 '+f.injection],pending:['# 当前批次的部分成果 '+f.injection]}});}`);
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('待继续') && document.querySelector('[data-organize-start]').textContent==='继续整理' && !document.querySelector('[data-organize-fresh]').hidden");
  await assert("document.querySelector('.knowledge-stages').textContent.includes('已返回 1/2 批') && document.querySelector('.knowledge-stages').textContent.includes('模块乙') && document.querySelector('.knowledge-stages').textContent.includes('当前批次的部分成果') && document.querySelector('[data-organize-report] pre').textContent===window.knowledgeOrganization.report",'Budget handoff loses the plan, pending range or saved Wiki');
  await noInjection();await visitHistory();
  await evaluate(`document.querySelector('[data-job-open="${paused}"]').click()`);
  await assert("document.querySelector('[data-detail-body] .badge').textContent==='待继续' && document.querySelector('[data-detail-body] .badge').classList.contains('warn') && document.querySelector('[data-detail-body] .knowledge-stages').textContent.includes('模块乙')",'History mislabels a budget handoff');
  await noInjection();await visitKnowledge();
  const resumed=await start();
  await assert(`window.knowledgeOrganization.calls.start.at(-1).resume_job_id===${JSON.stringify(paused)}`,'Continue starts over instead of attaching the saved checkpoint');
  await evaluate("{const f=window.knowledgeOrganization;f.latest.status='running';f.latest.result={saved:false,checkpoint:{plan:'保留计划',batches:[{title:'模块乙',scope:'剩余范围'}],completed:[]}};}");
  await waitFor("!!document.querySelector('.knowledge-stages')");
  const beforeProgress=await evaluate("document.querySelector('.knowledge-stages').open=true;window.knowledgeOrganization.latest.result.checkpoint.partial='新返回的阶段正文';window.knowledgeOrganization.calls.poll.length");
  await waitFor(`window.knowledgeOrganization.calls.poll.length>${beforeProgress+1}`);
  await assert("document.querySelector('.knowledge-stages').open",'Progress polling closes the plan while the user reads it');
  await evaluate(`{const f=window.knowledgeOrganization;f.finish(${JSON.stringify(resumed)},'cancelled',null);f.latest.result.checkpoint={plan:'保留计划',batches:[],completed:[]};}`);
  await waitFor("!document.querySelector('[data-organize-fresh]').hidden");
  const beforeFresh=await evaluate('window.knowledgeOrganization.calls.start.length');
  await evaluate("document.querySelector('[data-organize-fresh]').click()");
  await waitFor(`window.knowledgeOrganization.calls.start.length===${beforeFresh+1}`);
  await assert("window.knowledgeOrganization.calls.start.at(-1).resume_job_id===undefined",'Replan reuses a checkpoint unexpectedly');
  await evaluate("window.knowledgeOrganization.finish(window.knowledgeOrganization.latest.id,'cancelled',null)");
  await waitFor("!document.querySelector('[data-organize-start]').disabled");

  // An initial status error has a usable retry button.
  await visitHistory();
  await evaluate("window.knowledgeOrganization.failStatus=1;location.hash='#/projects/knowledge-fixture/knowledge'");
  await waitFor("!!document.querySelector('[data-organize-retry]')");
  await assert("document.querySelector('[data-organize-message]').textContent.includes('读取整理状态失败')",'Initial status error is hidden');
  await evaluate("document.querySelector('[data-organize-retry]').click()");
  await waitFor("!!document.querySelector('[data-organize-start]') && !document.querySelector('[data-organize-start]').disabled");

  // Empty knowledge still starts automatic collection; checkpoints cannot bypass a missing runner.
  await evaluate('window.knowledgeOrganization.sourceCount=0');await detect();
  await assert("!document.querySelector('[data-organize-start]').disabled && document.querySelector('.knowledge-organization').textContent.includes('还没有入库资料也可以直接开始')",'Empty knowledge blocks automatic collection');
  await evaluate("document.querySelector('.knowledge-location').open=true;document.querySelector('[data-checkpoint]').click()");
  await waitFor("window.knowledgeOrganization.calls.checkpoint===2 && document.querySelector('.knowledge-location [data-message]').textContent.includes('恢复配置已保存')");
  await assert("!document.querySelector('[data-organize-start]').disabled",'Saving a recovery checkpoint disabled automatic collection');
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

  // A fresh visit can start and finish with the supplemental instructions untouched.
  await visitKnowledge();
  await assert("!document.querySelector('[data-organize-options]').open && document.querySelector('[data-organize-focus]').value==='' && !document.querySelector('[data-organize-start]').disabled",'A fresh visit does not offer a usable default organization action');
  const defaultJob=await start();
  await assert("window.knowledgeOrganization.calls.start.at(-1).focus===''",'Default action silently requires or fabricates user instructions');
  await evaluate(`window.knowledgeOrganization.finish(${JSON.stringify(defaultJob)},'success','# 默认规则整理结果\\n\\n保留出处、维护建议和待核实事项。',true)`);
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('整理完成') && !document.querySelector('[data-organize-start]').disabled");
  await assert("!document.querySelector('[data-organize-options]').open && document.querySelector('[data-organize-focus]').value===''",'Default completion opens or fills the optional input');
  await capture('default-complete',390);
  // An older shared library offers an explicit, repository-only split with a reviewable preview.
  await visitHistory();await evaluate('window.knowledgeOrganization.needsSeparation=true');await visitKnowledge();
  await evaluate("document.querySelector('.knowledge-location').open=true");
  await assert("document.querySelector('[data-submit]').textContent==='预览拆分' && document.querySelector('[name=path]').value==='/fixture/IndependentKnowledge'",'Legacy library does not offer an independent destination');
  await evaluate("document.querySelector('[data-form]').requestSubmit()");
  await waitFor("document.querySelector('[data-submit]').textContent==='拆分并切换'");
  await assert("document.querySelector('[data-preview]').textContent.includes('1 篇旧文档尚无明确归属') && window.knowledgeOrganization.calls.separate[0].execute===false",'Split preview omits ownership gaps or performs writes');
  for(const width of [1440,390,320])await capture('separation-preview',width,'.knowledge-location');
  await evaluate("document.querySelector('[data-form]').requestSubmit()");
  await waitFor("document.querySelector('.knowledge-location [data-message]').textContent.includes('已提取本项目的知识目录')");
  await assert("window.knowledgeOrganization.calls.separate.length===2 && window.knowledgeOrganization.calls.separate[1].expected==='split-preview' && window.knowledgeOrganization.calls.separate[1].execute===true",'Split confirmation lost the exact preview token');
  const directory='app-monorepo--11111111-1111-4111-8111-111111111111';
  await visitHistory();await evaluate(`window.knowledgeOrganization.repository='/fixture/agent-knowledge';window.knowledgeOrganization.directoryName='${directory}';window.knowledgeOrganization.path='/fixture/agent-knowledge/${directory}'`);await visitKnowledge();
  await evaluate("document.querySelector('.knowledge-location').open=true");
  await assert(`document.querySelector('[data-storage-mode]').value==='repository' && document.querySelector('[name=path]').value==='/fixture/agent-knowledge' && document.querySelector('[data-storage-note]').textContent.includes('${directory}/')`,'Shared repository choice does not show its project folder');
  for(const width of [1440,390])await capture('shared-repository',width,'.knowledge-location');
  await evaluate("document.querySelector('[name=path]').value='/fixture/shared-next';document.querySelector('[data-form]').requestSubmit()");
  await waitFor("document.querySelector('[data-submit]').textContent==='迁移并切换'");
  await assert(`window.knowledgeOrganization.calls.move.at(-1).repository_path==='/fixture/shared-next' && !window.knowledgeOrganization.calls.move.at(-1).path && document.querySelector('[data-preview]').textContent.includes('/fixture/shared-next/${directory}')`,'Shared placement is not scoped to the project folder');
  await evaluate("const mode=document.querySelector('[data-storage-mode]');mode.value='directory';mode.dispatchEvent(new Event('change'))");
  await assert(`document.querySelector('[name=path]').value==='/fixture/agent-knowledge/${directory}' && document.querySelector('[data-preview]').textContent===''`);
  await evaluate("document.querySelector('[name=path]').value='/fixture/standalone';document.querySelector('[data-form]').requestSubmit()");
  await waitFor("document.querySelector('[data-submit]').textContent==='迁移并切换'");
  await assert("window.knowledgeOrganization.calls.move.at(-1).path==='/fixture/standalone' && !window.knowledgeOrganization.calls.move.at(-1).repository_path",'Direct project directory choice was forced into the shared repository');
  await writeFile(output+'-layout.json',JSON.stringify(layouts,null,2)+'\n');
  const calls=await evaluate('window.knowledgeOrganization.calls');
  console.log(JSON.stringify({checks:'default rules without input, runner/focus, duplicate guards, detection, polling errors/remount, saved report refresh, unsaved failed draft, cancellation retry, initial/status/start errors, empty notes/missing CLI, escaped text, history reports',startRequests:calls.start.length,cancelRequests:calls.cancel.length,statusRequests:calls.status,layouts:layouts.length}));
}
