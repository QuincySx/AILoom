// Browser-only fixtures for automatic collection: no real repository or CLI is accessed.
import {writeFile} from 'node:fs/promises';

export async function verifyKnowledgeIngestion({evaluate,waitFor,send,screenshot,output}) {
  await evaluate(String.raw`(async()=>{
    const {api}=await import('/ui/services/api.js');
    const root='/fixture/projects/automatic-knowledge-with-a-long-directory-name';
    const injection='<img src=x onerror="window.ingestionInjection=true">';
    const fixture=window.knowledgeIngestion={root,injection,count:0,calls:[],approved:[],job:null,
      worktrees:[{label:'主工作目录 · main',path:root},{label:'feature/decision-history '+injection,path:'/fixture/worktrees/development-documents-with-a-very-long-directory-name'}],
    };
    api.state=async()=>({repos:[{repo_id:'ingestion-fixture',common_dir:root+'/.git',project:{name:'自动整理开发知识'},worktrees:{main:{path:root,status:'active',branch:'refs/heads/main'}}}]});
    api.approveDir=async path=>{fixture.approved.push(path);return {};};
    api.knowledge=async body=>{
      fixture.calls.push(structuredClone(body));
      if(body.action==='status')return {initialized:true,files:4,location:{path:'/fixture/Knowledge'},default_path:'/fixture/Knowledge'};
      if(body.action==='inspect')return {git:null};
      if(body.action==='organization-status')return structuredClone({knowledge_id:'automatic-knowledge',source_count:fixture.count,worktrees:fixture.worktrees,report:null,runners:[{id:'codex',name:'Codex',available:true}],latest_job:fixture.job});
      if(body.action==='organize'){
        fixture.job={id:'auto-knowledge-job',kind:'knowledge_organize',status:'running',progress:['正在读取工作目录资料：feature/decision-history'],result:{source_count:0}};
        return {job_id:fixture.job.id};
      }
      throw Error('Automatic flow must not ask for manual ingestion: '+body.action);
    };
    api.job=async()=>structuredClone(fixture.job);
    api.jobs=async()=>structuredClone({jobs:[fixture.job]});
    sessionStorage.removeItem('ailoom-return-after-settings');sessionStorage.removeItem('ailoom-directory-view');
    location.hash='#/projects/ingestion-fixture/knowledge';
  })()`);
  const assert=async(expression,message)=>{if(!await evaluate(expression))throw Error(message);};
  await waitFor("!!document.querySelector('[data-organize-sources]') && !document.querySelector('[data-organize-start]').disabled");
  await assert("!document.querySelector('[data-ingest-scan],[data-ingest-source],[data-ingest-document],[data-ingest-save],.knowledge-ingestion')",'Manual scanning or document selection remains in the primary flow');
  await assert("document.querySelector('[data-organize-count]').textContent.includes('已有 0 篇知识资料') && document.querySelector('[data-organize-count]').textContent.includes('2 个工作目录')",'The empty library does not show automatic collection scope');
  await assert("!document.querySelector('[data-organize-sources]').open && !document.querySelector('[data-organize-options]').open && document.querySelector('[data-organize-focus]').value===''",'Default organization needs extra input');
  await assert("document.querySelector('[data-organize-sources]').textContent.includes(window.knowledgeIngestion.injection) && !window.ingestionInjection && !document.querySelector('.knowledge-organization img')",'Source labels were interpreted as HTML');
  const layouts=[];
  async function capture(name,width) {
    await send('Emulation.setDeviceMetricsOverride',{width,height:width<=700?844:1000,deviceScaleFactor:1,mobile:false});
    await evaluate('window.scrollTo(0,0)');
    await new Promise(resolve=>setTimeout(resolve,100));
    const layout=await evaluate(`(()=>{
      const visible=e=>e.getClientRects().length&&getComputedStyle(e).visibility!=='hidden';
      return {width:innerWidth,scroll:document.documentElement.scrollWidth,controls:[...document.querySelectorAll('.knowledge-organization button,.knowledge-organization [role=combobox]')].filter(visible).map(e=>{const r=e.getBoundingClientRect();return {text:e.textContent.trim(),x:r.x,right:r.right,width:r.width,height:r.height};})};
    })()`);
    if(layout.scroll>width+1||layout.controls.some(r=>r.x<0||r.right>width||r.width<1||r.height<(width<=700?44:30)))throw Error('Unusable '+name+' layout: '+JSON.stringify(layout));
    await screenshot(output+'-'+name+'-'+width+'.png');layouts.push({name,...layout});
  }
  for(const width of [1440,390,320])await capture('automatic-ready',width);
  await evaluate("document.querySelector('[data-organize-sources]').open=true");
  for(const width of [1440,390,320])await capture('source-scope',width);
  await evaluate("document.querySelector('[data-organize-start]').click()");
  await waitFor("document.querySelector('[data-organize-message]').textContent.includes('正在读取工作目录资料')");
  await assert("window.knowledgeIngestion.calls.filter(c=>c.action==='organize').length===1 && window.knowledgeIngestion.calls.find(c=>c.action==='organize').focus===''",'A single default action did not start automatic collection');
  await assert("!window.knowledgeIngestion.calls.some(c=>c.action.startsWith('ingestion-'))",'Automatic organization still calls a manual ingestion step');
  await assert("window.knowledgeIngestion.worktrees.every(s=>window.knowledgeIngestion.approved.includes(s.path))",'Start did not approve its displayed worktree scope');
  await assert("document.querySelector('[data-organize-start]').disabled && !document.querySelector('[data-organize-cancel]').hidden",'Collection is not treated as part of the running task');
  await evaluate("{const f=window.knowledgeIngestion;f.count=4;f.job.status='success';f.job.progress=['整理完成'];f.job.result={source_count:4,saved:true};}");
  await waitFor("!document.querySelector('[data-organize-start]').disabled && document.querySelector('[data-organize-count]').textContent.includes('已有 4 篇知识资料')");
  await assert("!document.querySelector('[data-organize-options]').open && !document.querySelector('[data-ingest-document]')",'Successful automatic collection introduced a manual choice');
  await capture('automatic-complete',390);
  await evaluate("{const f=window.knowledgeIngestion;f.count=0;f.job.status='failed';f.job.error='没有找到可整理的资料';f.job.result={source_count:0,collection:{worktree_count:2,documents_read:0,skipped:[{directory:'main',path:'docs/'+f.injection+'.md',reason:'不是 UTF-8 文本 '+f.injection}]}};document.querySelector('[data-organize-detect]').click();}");
  await waitFor("document.querySelector('[data-organize-collection]').textContent.includes('不是 UTF-8 文本')");
  await assert("!document.querySelector('[data-organize-start]').disabled && !window.ingestionInjection && !document.querySelector('.knowledge-collection img')",'Empty-input diagnostics are lost, unsafe, or prevent retry');
  await evaluate("location.hash='#/tasks'");
  await waitFor("document.querySelector('[data-detail-body] .knowledge-collection')?.textContent.includes('不是 UTF-8 文本')");
  await assert("!window.ingestionInjection && !document.querySelector('.tasks-page img')",'History diagnostics were interpreted as HTML');
  await evaluate("document.querySelector('[data-detail-body] .knowledge-collection').open=true");
  await capture('empty-input-diagnostics',320);
  await writeFile(output+'-layout.json',JSON.stringify(layouts,null,2)+'\n');
  console.log(JSON.stringify({checks:'zero-note start, automatic worktree scope, no manual scan/selection/import, default prompt, task progress, refreshed source count, escaped source labels',layouts:layouts.length}));
}
