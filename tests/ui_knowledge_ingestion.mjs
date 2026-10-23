// Browser-only API fixtures: never read source files, write knowledge, or invoke a CLI.
import {writeFile} from 'node:fs/promises';

export async function verifyKnowledgeIngestion({evaluate,waitFor,send,screenshot,output}) {
  await evaluate(String.raw`(async()=>{
    const {api}=await import('/ui/services/api.js');
    const root='/fixture/projects/knowledge-ingestion-with-a-long-directory-name';
    const injection='<img src=x onerror="window.ingestionInjection=true">';
    const fixture=window.knowledgeIngestion={
      root,injection,knowledgePath:root+'/Knowledge',documentCount:2,revisionCount:3,sourceCount:2,
      sources:[
        {id:'main',label:'主工作目录 · main',path:root,worktree:root},
        {id:'other',label:'其他 Worktree · feature/development-documents',path:'/fixture/worktrees/development-with-a-long-directory-name',worktree:'/fixture/worktrees/development-with-a-long-directory-name'},
        {id:'child',label:'托管子目录 · docs '+injection,path:root+'/managed/firmware-documents',worktree:root},
      ],
      calls:{status:0,preview:[],apply:[],approve:[],organization:0},
      failStatus:1,failPreview:0,failApply:0,holdStatus:false,holdPreview:false,holdApply:false,
    };
    fixture.preview=source=>({source,origin:{branch:'feature/docs '+injection,commit:'1234567890abcdef1234567890abcdef12345678',worktree_dirty:true},expected:'fixture-snapshot-'+source.id,
      documents:[
        {path:'docs/getting-started.md',title:'启动与调试说明',state:'new'},
        {path:'docs/architecture-with-a-very-long-unbroken-document-name-for-mobile.md',title:'硬件连接架构 '+injection,state:'updated'},
        {path:'docs/shared-notes.md',title:'已有原文的另一个来源',state:'duplicate'},
        {path:'docs/already-saved.md',title:'未变化的已保存资料',state:'unchanged'},
        {path:'docs/context-updated.md',title:'相同原文的新分支或提交出处',state:'context_updated'},
        {path:'docs/'+injection+'.md',title:'文本安全验收',state:'new'},
      ],skipped:[{path:'docs/private-'+injection+'.md',reason:'无法读取 '+injection}]});
    api.state=async()=>({repos:[{repo_id:'ingestion-fixture',common_dir:root+'/.git',project:{name:'开发资料采集验收'},worktrees:{main:{path:root,status:'active',branch:'refs/heads/main'}}}]});
    api.approveDir=async path=>{fixture.calls.approve.push(path);return {};};
    api.knowledge=async body=>{
      if(body.root!==root)throw Error('Unexpected ingestion fixture root');
      if(body.action==='inspect')return {git:null};
      if(body.action==='status')return {initialized:true,files:4,location:{path:fixture.knowledgePath},default_path:fixture.knowledgePath};
      if(body.action==='organization-status'){
        fixture.calls.organization++;
        return {knowledge_id:'ingestion-knowledge',source_count:fixture.sourceCount,report:null,runners:[{id:'codex',name:'Codex',available:true}],latest_job:null};
      }
      if(body.action==='ingestion-status'){
        fixture.calls.status++;
        if(fixture.holdStatus)await new Promise(resolve=>fixture.releaseStatus=resolve);
        if(fixture.failStatus-- >0)throw Error('读取资料来源失败 '+injection);
        return structuredClone({sources:fixture.sources,knowledge_path:fixture.knowledgePath,document_count:fixture.documentCount,revision_count:fixture.revisionCount});
      }
      if(body.action==='ingestion-preview'){
        fixture.calls.preview.push(structuredClone(body));
        if(fixture.holdPreview)await new Promise(resolve=>fixture.releasePreview=resolve);
        if(fixture.failPreview-- >0)throw Error('扫描暂时失败 '+injection);
        const source=fixture.sources.find(s=>s.id===body.source_id);if(!source)throw Error('Unknown fixture source');
        return fixture.preview(structuredClone(source));
      }
      if(body.action==='ingestion-apply'){
        fixture.calls.apply.push(structuredClone(body));
        if(fixture.holdApply)await new Promise(resolve=>fixture.releaseApply=resolve);
        if(fixture.failApply-- >0)throw Error('保存暂时失败 '+injection);
        if(body.expected!=='fixture-snapshot-'+body.source_id)throw Error('Unexpected snapshot token');
        fixture.documentCount++;fixture.revisionCount+=body.documents.length;fixture.sourceCount++;
        return {saved:body.documents.length,new_documents:1,reused_documents:body.documents.length-1};
      }
      throw Error('Unexpected ingestion fixture action: '+body.action);
    };
    sessionStorage.removeItem('ailoom-return-after-settings');sessionStorage.removeItem('ailoom-directory-view');
    location.hash='#/projects/ingestion-fixture/knowledge';
  })()`);

  async function assert(expression,message) {
    if(!await evaluate(expression))throw Error(message);
  }
  async function noInjection() {
    await assert("!window.ingestionInjection && !document.querySelector('.knowledge-ingestion img')",'Source, document, origin, skipped or error text was interpreted as HTML');
  }
  async function chooseSource(id) {
    await waitFor("!!document.querySelector('[data-ingest-source]')?.parentElement.querySelector('[role=combobox]')");
    await evaluate("document.querySelector('[data-ingest-source]').parentElement.querySelector('[role=combobox]').click()");
    await waitFor("!!document.querySelector('.knowledge-ingestion .select-menu:popover-open')");
    await evaluate(`{const s=document.querySelector('[data-ingest-source]');const index=[...s.options].findIndex(o=>o.value===${JSON.stringify(id)});s.parentElement.querySelector('[role=option][data-index="'+index+'"]').click();}`);
    await waitFor(`document.querySelector('[data-ingest-source]').value===${JSON.stringify(id)}`);
  }
  async function scan() {
    const before=await evaluate('window.knowledgeIngestion.calls.preview.length');
    await evaluate("document.querySelector('[data-ingest-scan]').click()");
    await waitFor(`window.knowledgeIngestion.calls.preview.length===${before+1} && !!document.querySelector('[data-ingest-save]') && !document.querySelector('[data-ingest-scan]').disabled`);
  }
  const layouts=[];
  async function capture(name,width,anchor='.knowledge-ingestion') {
    await send('Emulation.setDeviceMetricsOverride',{width,height:width<=700?844:1000,deviceScaleFactor:1,mobile:false});
    await evaluate(`document.querySelector(${JSON.stringify(anchor)}).scrollIntoView({block:'start'})`);
    if(width<=700)await evaluate('window.scrollBy(0,-72)');
    await new Promise(resolve=>setTimeout(resolve,100));
    const layout=await evaluate(`(()=>{
      const rect=e=>{const r=e.getBoundingClientRect();return {x:r.x,y:r.y,width:r.width,height:r.height,right:r.right,bottom:r.bottom};};
      const visible=e=>e.getClientRects().length&&getComputedStyle(e).visibility!=='hidden';
      const source=document.querySelector('[data-ingest-source]').parentElement.querySelector('[role=combobox]');
      return {width:innerWidth,scroll:document.documentElement.scrollWidth,source:rect(source),sourceButtons:[...document.querySelectorAll('[data-ingest-scan],[data-ingest-refresh]')].map(rect),
        controls:[...document.querySelectorAll('.knowledge-ingestion button')].filter(visible).map(e=>({text:e.textContent.trim(),...rect(e)})),
        checkboxes:[...document.querySelectorAll('[data-ingest-document]')].map(e=>({...rect(e),label:rect(e.closest('label'))})),
        documents:[...document.querySelectorAll('.knowledge-ingestion-documents')].map(e=>({...rect(e),scroll:e.scrollWidth,client:e.clientWidth}))};
    })()`);
    if(layout.scroll>width+1)throw Error(name+' overflows '+width+': '+JSON.stringify(layout));
    if(layout.controls.some(c=>c.x<0||c.right>width||c.width<1||c.height<(width<=700?44:30)))throw Error(name+' has clipped or undersized controls: '+JSON.stringify(layout));
    if(layout.checkboxes.some(c=>c.width<16||c.width>24||c.height<16||c.height>24||c.label.height<(width<=700?44:30)))throw Error(name+' has unsuitable checkbox sizing: '+JSON.stringify(layout));
    if(layout.documents.some(d=>d.x<0||d.right>width||d.scroll>d.client+1))throw Error(name+' has overflowing documents: '+JSON.stringify(layout));
    if(width>700&&layout.sourceButtons.some(b=>Math.abs(b.bottom-layout.source.bottom)>1))throw Error(name+' source select and buttons are not bottom-aligned: '+JSON.stringify(layout));
    if(Math.abs(layout.sourceButtons[0].bottom-layout.sourceButtons[1].bottom)>1)throw Error(name+' scan and refresh are not bottom-aligned: '+JSON.stringify(layout));
    await screenshot(output+'-'+name+'-'+width+'.png');layouts.push({name,...layout});
  }

  // A failed initial load exposes a retry and keeps server text inert.
  await waitFor("!!document.querySelector('[data-ingest-retry]')");
  await assert("document.querySelector('[data-ingest-message]').textContent.includes('读取资料来源失败')",'Initial source failure is hidden');
  await noInjection();
  await evaluate("{const f=window.knowledgeIngestion;f.holdStatus=true;const b=document.querySelector('[data-ingest-retry]');b.click();b.click();void b.onclick();}");
  await waitFor('window.knowledgeIngestion.calls.status===2');
  await evaluate('window.knowledgeIngestion.holdStatus=false;window.knowledgeIngestion.releaseStatus()');
  await waitFor("!!document.querySelector('[data-ingest-source]') && !document.querySelector('[data-ingest-scan]').disabled");
  await assert("document.querySelector('[data-ingest-count]').textContent==='已采集 2 份当前原文 · 3 条来源版本记录' && document.querySelector('[data-organize-count]').textContent==='2 篇知识文档可整理'",'Initial raw document/revision or organizer counts are wrong');
  await assert("[...document.querySelector('[data-ingest-source]').options].map(o=>o.value).join(',')==='main,other,child' && document.querySelector('[data-ingest-source]').value==='main'",'Main, other Worktree and managed child sources are unavailable or main is not the default');
  await assert("document.querySelector('[data-ingest-storage]').textContent.includes('删除整个工作目录前')",'Storage inside a worktree is missing its migration warning');
  await assert("!!(document.querySelector('.knowledge-ingestion').compareDocumentPosition(document.querySelector('.knowledge-organization'))&Node.DOCUMENT_POSITION_FOLLOWING)",'Ingestion is not before organization');
  for(const width of [1440,390,320])await capture('ready',width);

  // Use the actual HTML dropdown and ensure the chosen directory is approved/scanned.
  await chooseSource('other');
  await assert("document.querySelector('[data-ingest-path]').textContent===window.knowledgeIngestion.sources[1].path && !document.querySelector('[data-ingest-storage]').textContent",'Source path or storage warning did not follow the selection');
  await evaluate("{const f=window.knowledgeIngestion;f.holdPreview=true;const b=document.querySelector('[data-ingest-scan]');b.click();b.click();void b.onclick();}");
  await waitFor("window.knowledgeIngestion.calls.preview.length===1 && document.querySelector('[data-ingest-scan]').disabled");
  await assert("document.querySelector('[data-ingest-source]').disabled && document.querySelector('[data-ingest-refresh]').disabled && window.knowledgeIngestion.calls.preview[0].source_id==='other' && window.knowledgeIngestion.calls.approve.at(-1)===window.knowledgeIngestion.sources[1].path",'Scanning allows changes or omitted the selected source/approval');
  await evaluate('window.knowledgeIngestion.holdPreview=false;window.knowledgeIngestion.releasePreview()');
  await waitFor("!!document.querySelector('[data-ingest-save]') && !document.querySelector('[data-ingest-scan]').disabled");
  await assert("document.querySelectorAll('[data-ingest-document]').length===6 && document.querySelectorAll('[data-ingest-document]:checked').length===5 && ![...document.querySelectorAll('[data-ingest-document]')].find(e=>e.value==='docs/already-saved.md').checked && [...document.querySelectorAll('[data-ingest-document]')].find(e=>e.value==='docs/context-updated.md').checked && document.querySelector('[data-ingest-preview]').textContent.includes('来源有更新')",'Unchanged documents must default unchecked while new, updated, duplicate and changed-origin documents default checked');
  await assert("document.querySelector('[data-ingest-preview]').textContent.includes('工作目录有未提交内容') && document.querySelector('[data-ingest-preview]').textContent.includes('1234567890ab') && document.querySelector('[data-ingest-preview] details').textContent.includes('无法读取')",'Preview is missing origin or skipped-document details');
  await noInjection();
  for(const width of [1440,390,320])await capture('preview',width);
  await capture('selection',390,'.knowledge-ingestion-documents');

  await evaluate("document.querySelector('[data-ingest-none]').click()");
  await assert("!document.querySelectorAll('[data-ingest-document]:checked').length && document.querySelector('[data-ingest-save]').disabled",'Clearing selection does not disable save');
  await evaluate("document.querySelector('[data-ingest-all]').click()");
  await assert("document.querySelectorAll('[data-ingest-document]:checked').length===6 && !document.querySelector('[data-ingest-save]').disabled",'Select all omitted an unchanged document');
  await chooseSource('child');
  await assert("!document.querySelector('[data-ingest-save]') && !document.querySelector('[data-ingest-preview]').textContent && !document.querySelector('[data-ingest-message]').textContent",'Changing source retained a stale preview or selection');
  await assert("document.querySelector('[data-ingest-storage]').textContent.includes('删除整个工作目录前')",'Managed child omitted the containing worktree storage warning');
  await noInjection();

  // A scan failure leaves controls usable, and a retry renders a fresh preview.
  await evaluate("window.knowledgeIngestion.failPreview=1;document.querySelector('[data-ingest-scan]').click()");
  await waitFor("document.querySelector('[data-ingest-message]').textContent.includes('扫描暂时失败') && !document.querySelector('[data-ingest-scan]').disabled");
  await noInjection();await scan();
  await evaluate("document.querySelector('[data-ingest-none]').click();[...document.querySelectorAll('[data-ingest-document]')].filter(e=>['docs/getting-started.md','docs/shared-notes.md'].includes(e.value)).forEach(e=>e.click());window.knowledgeIngestion.failApply=1;document.querySelector('[data-ingest-save]').click()");
  await waitFor("window.knowledgeIngestion.calls.apply.length===1 && document.querySelector('[data-ingest-message]').textContent.includes('保存暂时失败') && !document.querySelector('[data-ingest-save]').disabled");
  await assert("document.querySelectorAll('[data-ingest-document]:checked').length===2",'Failed save lost the selection needed for retry');
  await noInjection();

  // Save reuses the preview token, sends only checked paths, and refreshes both counts.
  await evaluate("{const f=window.knowledgeIngestion;f.holdApply=true;const b=document.querySelector('[data-ingest-save]');b.click();b.click();void b.onclick();}");
  await waitFor("window.knowledgeIngestion.calls.apply.length===2 && document.querySelector('[data-ingest-save]').disabled");
  await assert("[...document.querySelectorAll('.knowledge-ingestion button,.knowledge-ingestion input,.knowledge-ingestion select')].every(e=>e.disabled)",'Saving permits a second action or selection change');
  await assert("(()=>{const f=window.knowledgeIngestion;return f.calls.apply.every(b=>b.root===f.root&&b.source_id==='child'&&b.expected==='fixture-snapshot-child'&&b.documents.join(',')==='docs/getting-started.md,docs/shared-notes.md')})()",'Save omitted expected token, selected documents, source or project root');
  await evaluate('window.knowledgeIngestion.holdApply=false;window.knowledgeIngestion.releaseApply()');
  await waitFor("document.querySelector('[data-ingest-message]').textContent.includes('已保存 2 篇') && document.querySelector('[data-ingest-count]').textContent==='已采集 3 份当前原文 · 5 条来源版本记录' && document.querySelector('[data-organize-count]').textContent==='3 篇知识文档可整理'");
  await assert("!document.querySelector('[data-ingest-save]') && document.querySelector('[data-ingest-source]').value==='child' && window.knowledgeIngestion.calls.organization===2",'Successful save retained preview, lost source or skipped organizer refresh');
  await capture('saved',390);await noInjection();

  // Directory refresh cannot overlap and invalidates a previous preview.
  await scan();
  const statusBefore=await evaluate('window.knowledgeIngestion.calls.status');
  await evaluate("{const f=window.knowledgeIngestion;f.holdStatus=true;const b=document.querySelector('[data-ingest-refresh]');b.click();b.click();void b.onclick();}");
  await waitFor(`window.knowledgeIngestion.calls.status===${statusBefore+1} && document.querySelector('[data-ingest-refresh]').disabled`);
  await evaluate('window.knowledgeIngestion.holdStatus=false;window.knowledgeIngestion.releaseStatus()');
  await waitFor("!document.querySelector('[data-ingest-refresh]').disabled && !document.querySelector('[data-ingest-save]')");
  await noInjection();
  await writeFile(output+'-layout.json',JSON.stringify(layouts,null,2)+'\n');
  const calls=await evaluate('window.knowledgeIngestion.calls');
  console.log(JSON.stringify({checks:'source choice/approval, initial retry, preview selection, expected payload, source invalidation, busy guards, scan/save retry, saved organizer refresh, escaped text, storage warning, responsive alignment/overflow/checkboxes',previewRequests:calls.preview.length,applyRequests:calls.apply.length,statusRequests:calls.status,layouts:layouts.length}));
}
