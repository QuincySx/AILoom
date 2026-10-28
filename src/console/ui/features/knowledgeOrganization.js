import { api, esc } from '../services/api.js';

const labels = {queued:'等待开始',running:'正在整理',success:'整理完成',failed:'整理失败',cancelled:'已取消',interrupted:'整理已中断'};
const running = job => job && ['queued','running'].includes(job.status);
const resumable = job => job?.result?.checkpoint && !job.result.saved && !job.result.unchanged;

export function collectionDetails(collection) {
  if(!collection)return '';
  const skipped=collection.skipped||[];
  return `<details class="knowledge-collection"><summary>原文采集情况${skipped.length?' · 存在未读取或未保存项':''}</summary><p>自动采集检查 ${esc(collection.worktree_count||0)} 个工作目录，读取 ${esc(collection.documents_read||0)} 篇候选资料；这不代表 AI 已逐篇研读。实际阅读、未采用与跳过原因见阶段稿和评估记录。</p>${skipped.length?`<ul>${skipped.map(s=>`<li>${esc([s.directory,s.path].filter(Boolean).join(' / '))}：${esc(s.reason)}</li>`).join('')}</ul>`:''}</details>`;
}

export function organizationOutcome(job) {
  if(job?.result?.unchanged)return '无需更新';
  if(job?.result?.needs_continuation)return '整理待继续';
  if(job?.result?.needs_revision)return '整理稿待修订';
  return labels[job?.status]||job?.status||'';
}

export function incrementalDetails(result) {
  const delta=result?.incremental;
  if(!delta)return '';
  return `<details class="knowledge-incremental"><summary>${delta.mode==='incremental'?'按变化维护':'建立首次增量基线'}</summary><p>资料及出处变化 ${esc(delta.source_changes??0)} 项，代码与本地文档变化 ${esc(delta.code_changes??0)} 项，Wiki 变化 ${esc(delta.wiki_changes??0)} 项。AI 判断受影响主题，并检查未读资料与待核实事项。</p>${delta.baseline_published_at?`<p>上次成功整理：${esc(delta.baseline_published_at)}</p>`:''}<p class="muted">${esc(delta.scope||'')}</p>${delta.uncertainties?.length?`<p>部分指纹无法比较，本轮不会直接沿用：</p><ul>${delta.uncertainties.map(text=>`<li>${esc(text)}</li>`).join('')}</ul>`:''}${result.unchanged?`<p>${esc(result.reuse_reason||'本轮无需更新')}。沿用上次评估，本轮未重新评估；已有 Wiki 未改写。</p>`:''}</details>`;
}

export function revisionDetails(result) {
  const history=result?.repair_history||[];
  return `${result?.warning?`<p class="muted" role="status">${esc(result.warning)}</p>`:''}${history.length?`<details class="knowledge-revisions"><summary>AI 修订记录（${history.length}）</summary>${history.map((entry,i)=>`<details><summary>第 ${i+1} 份草稿 · ${esc(entry.diagnostic?.message||'待核对')}</summary><pre class="knowledge-organization-report">${esc(entry.report)}</pre></details>`).join('')}</details>`:''}`;
}

export function assessmentDetails(result) {
  const history=result?.assessment_history||[],partial=result?.assessment_partial;
  if(!history.length&&!partial)return '';
  const label={pass:'通过',revise:'需修订',insufficient:'评估资料不足',unconfirmed:'结论待确认'};
  return `<details class="knowledge-assessments"><summary>独立 AI 评估 · ${history.length} 轮${partial?'，本轮尚未完成':''}</summary><p class="muted">评估针对对应候选稿及记录的证据范围，不代表人工批准或永久适用。</p>${history.map((entry,i)=>`<details><summary>第 ${i+1} 轮 · ${esc(label[entry.verdict]||'结论待确认')}</summary><pre class="knowledge-organization-report">${esc(entry.report)}</pre><details><summary>本轮对应的候选稿</summary><pre class="knowledge-organization-report">${esc(entry.candidate||'')}</pre></details></details>`).join('')}${partial?`<details><summary>本轮未完成的评估正文</summary><pre class="knowledge-organization-report">${esc(partial.report)}</pre></details>`:''}</details>`;
}

export function stageDetails(result) {
  const checkpoint=result?.checkpoint;
  if(!checkpoint)return '';
  const batches=checkpoint.batches||[],completed=checkpoint.completed||[];
  return `<details class="knowledge-stages"><summary>整理计划与阶段稿 · 已返回 ${completed.length}/${batches.length} 批</summary><p class="muted">阶段稿按实际阅读核对，不等于已发布知识或评估通过。</p><pre class="knowledge-organization-report">${esc(checkpoint.plan||'正在制定计划')}</pre>${batches.map((batch,i)=>`<details><summary>${esc(batch.title)} · ${i<completed.length?'已返回阶段稿':'待整理'}</summary><p>${esc(batch.scope)}</p>${i<completed.length?`<pre class="knowledge-organization-report">${esc(completed[i])}</pre>`:''}</details>`).join('')}${checkpoint.pending?.length?`<details><summary>当前批次的部分成果（${checkpoint.pending.length} 份），尚需继续</summary>${checkpoint.pending.map(body=>`<pre class="knowledge-organization-report">${esc(body)}</pre>`).join('')}</details>`:''}</details>`;
}

export function KnowledgeOrganization(container, {rootPath}) {
  const root=document.createElement('section');root.className='knowledge-organization';container.append(root);
  let alive=true, state=null, job=null, timer=null, busy=false, checking=false, runner='codex', focus='', pageRequest=0, stagesKey='',assessmentKey='';
  const q=s=>root.querySelector(s);
  const request=body=>api.knowledge({root:rootPath,...body});
  const message=text=>{if(alive)q('[data-organize-message]').textContent=text;};
  const remember=()=>{runner=q('[data-organize-runner]')?.value||runner;focus=q('[data-organize-focus]')?.value||'';};
  function controls() {
    const pending=busy||running(job);
    q('[data-organize-start]').disabled=checking||pending||!state?.runners?.some(r=>r.id===runner&&r.available);
    q('[data-organize-runner]').disabled=checking||pending;
    q('[data-organize-focus]').disabled=pending;
    q('[data-organize-detect]').disabled=checking||pending;
    q('[data-organize-cancel]').hidden=!running(job);
    q('[data-organize-cancel]').disabled=busy||job?.cancel_requested;
    q('[data-organize-progress]').hidden=!running(job);
    q('[data-organize-start]').textContent=resumable(job)?'继续整理':'开始整理';
    q('[data-organize-fresh]').hidden=!(resumable(job)&&!pending);
    q('[data-organize-fresh]').disabled=checking||pending;
  }
  function render() {
    stagesKey='';
    assessmentKey='';
    const tools=state.runners||[];
    if(!tools.some(r=>r.id===runner&&r.available))runner=tools.find(r=>r.available)?.id||'codex';
    const report=state.report;
    const worktrees=state.worktrees||[];
    root.innerHTML=`<h2>整理知识</h2><p class="muted" data-organize-default>直接开始，无需填写提示词或挑选文档。AI 会从本仓库各工作目录的资料和已有知识中识别业务规则、开发流程、模块约束、决策取舍与问题经验，去重整理，保留背景、演变和出处。</p>
      <p data-organize-count>已有 ${esc(state.source_count||0)} 篇知识资料 · 开始后自动收集 ${esc(worktrees.length)} 个工作目录及其子目录的资料</p>
      <p class="muted" data-organize-evidence>AI 会对照相关代码核对当前实现，注明分支与核对范围；过时文档保留历史背景，无法确认的结论标为待核实。生成候选稿后，另起独立上下文评估知识保留、资料处理和代码依据；未通过会保留草稿并修订，通过后再保存。</p>
      ${state.project_id?`<p data-organize-project>所属项目：<strong>${esc(state.project_name)}</strong></p><details><summary>原文与整理结果分别保存</summary><p>打开知识库根目录的 README.md 即可导航，wiki/ 中的 Markdown 可由你或 AI 直接维护，sources/ 保留原始资料与出处。</p><div class="path">原文：${esc(state.raw_path)}</div><div class="path">知识入口：${esc(state.report_path)}</div>${state.excluded_documents?.length?`<p>${esc(state.excluded_documents.length)} 篇旧文档缺少匹配的项目归属，本次未纳入。</p>`:''}</details>`:''}
      <details data-organize-sources><summary>查看整理范围</summary><ul>${worktrees.map(s=>`<li>${esc(s.label)}<div class="path muted">${esc(s.path)}</div></li>`).join('')}</ul><p class="muted">保留原文和各分支出处，AI 按内容判断是否值得纳入开发知识。无法读取或超出范围的资料会在结果中说明。</p></details>
      <div class="knowledge-organization-options"><label>执行工具<select data-organize-runner aria-label="整理知识的执行工具">${tools.map(r=>`<option value="${esc(r.id)}" ${r.id===runner?'selected':''} ${r.available?'':'disabled'}>${esc(r.name)}${r.available?'':'（未找到 CLI）'}</option>`).join('')}</select></label><button type="button" data-organize-detect>重新检测</button></div>
      <details data-organize-options ${focus.trim()?'open':''}><summary>补充整理要求（可选）</summary><label>额外关注的内容<textarea data-organize-focus rows="2" maxlength="2000" placeholder="留空即按默认规则全面整理，例如也可补充：优先整理硬件连接的排障经验">${esc(focus)}</textarea></label></details>
      <p class="muted">调用所选工具的本机配置和登录状态，使用其模型服务。整理结果会保存到本项目知识库，后续检索可用。</p>
      <p class="muted" data-organize-runtime>先比较资料、代码与人工 Wiki 的变化，由 AI 判断影响及未完成项，再按模块分批保存阶段稿，每批约 ${esc(state.runtime_policy?.batch_minutes??20)} 分钟。${esc(state.runtime_policy?.wrap_up_minutes??90)} 分钟进入收尾，总预算 ${esc(state.runtime_policy?.maximum_minutes??120)} 分钟；到预算边界保留进度，可继续整理。页面持续更新耗时与最近输出，可随时取消。</p>
      ${!state.source_count?'<p class="muted">还没有入库资料也可以直接开始，系统会自动读取工作目录内的文档。配置恢复副本和工具资源不参与整理。</p>':''}
      <div class="actions"><button type="button" class="primary" data-organize-start>开始整理</button><button type="button" data-organize-fresh hidden>重新规划整理</button><button type="button" data-organize-cancel hidden>取消整理</button><a href="#/tasks">查看整理记录</a></div>
      <progress data-organize-progress aria-label="知识整理进度" hidden></progress><p data-organize-message role="status" aria-live="polite"></p>
      <div data-organize-incremental></div><div data-organize-collection></div><div data-organize-stages></div><div data-organize-assessments></div><div data-organize-revisions></div>
      <details data-organize-report ${report?'open':''} ${report?'':'hidden'}><summary>仓库开发知识</summary>${state.pages?.length?`<nav class="knowledge-pages" aria-label="知识页面"><button type="button" data-wiki-index>知识首页</button>${state.pages.map((p,i)=>`<button type="button" data-wiki-page="${i}">${esc(p.title)}</button>`).join('')}</nav>`:''}<p data-wiki-path class="path">${esc(state.report_path||'')}</p><pre class="knowledge-organization-report">${esc(report||'')}</pre></details>
      <details data-organize-draft hidden><summary>本次未保存的整理草稿</summary><pre class="knowledge-organization-report"></pre></details>`;
    q('[data-organize-runner]').onchange=()=>{remember();controls();};
    q('[data-organize-focus]').oninput=remember;
    q('[data-organize-detect]').onclick=()=>{remember();load();};
    q('[data-organize-start]').onclick=start;
    q('[data-organize-fresh]').onclick=()=>start(true);
    q('[data-organize-cancel]').onclick=cancel;
    const index=q('[data-wiki-index]');if(index)index.onclick=()=>{pageRequest++;q('[data-organize-report] pre').textContent=state.report;q('[data-wiki-path]').textContent=state.report_path;};
    root.querySelectorAll('[data-wiki-page]').forEach(button=>button.onclick=async()=>{
      const page=state.pages[Number(button.dataset.wikiPage)],requestId=++pageRequest;
      try {const result=await request({action:'organization-page',page:page.path});if(alive&&requestId===pageRequest){q('[data-organize-report] pre').textContent=result.body;q('[data-wiki-path]').textContent=page.path;}}
      catch(e){if(alive&&requestId===pageRequest)message(e.message);}
    });
    showJob();controls();
  }
  function showJob() {
    if(!job)return;
    const note=job.progress?.at(-1)||'';
    message(organizationOutcome(job)+(job.error?'：'+job.error:note?' · '+note:''));
    q('[data-organize-incremental]').innerHTML=incrementalDetails(job.result);
    q('[data-organize-collection]').innerHTML=collectionDetails(job.result?.collection);
    q('[data-organize-revisions]').innerHTML=revisionDetails(job.result);
    const reviewKey=JSON.stringify([job.result?.assessment_history,job.result?.assessment_partial]);
    if(reviewKey!==assessmentKey){
      const reviews=q('[data-organize-assessments]'),opened=[...reviews.querySelectorAll('details')].map((d,i)=>d.open?i:-1);
      reviews.innerHTML=assessmentDetails(job.result);
      reviews.querySelectorAll('details').forEach((d,i)=>{if(opened.includes(i))d.open=true;});
      assessmentKey=reviewKey;
    }
    const stageKey=JSON.stringify(job.result?.checkpoint??null);
    if(stageKey!==stagesKey){
      const stages=q('[data-organize-stages]'),opened=[...stages.querySelectorAll('details')].map((d,i)=>d.open?i:-1);
      stages.innerHTML=stageDetails(job.result);
      stages.querySelectorAll('details').forEach((d,i)=>{if(opened.includes(i))d.open=true;});
      stagesKey=stageKey;
    }
    const draft=q('[data-organize-draft]');
    draft.hidden=!(job.result?.report&&!job.result.saved&&!running(job));
    if(!draft.hidden){draft.querySelector('summary').textContent=job.result.report_partial?'本次未完成的正文草稿':'本次未保存的整理草稿';draft.querySelector('pre').textContent=job.result.report;}
  }
  async function load() {
    if(checking||!alive)return;
    checking=true;
    if(state)controls();
    try {
      const result=await request({action:'organization-status'});
      if(!alive)return;
      pageRequest++;state=result;job=result.latest_job;
      checking=false;render();
      if(running(job))poll();
    } catch(e) {
      if(alive) {
        if(!state)root.innerHTML='<h2>整理知识</h2><p data-organize-message role="status"></p><button type="button" data-organize-retry>重新读取</button>';
        message(e.message);
        const retry=q('[data-organize-retry]');if(retry)retry.onclick=load;
      }
    } finally {checking=false;if(alive&&state)controls();}
  }
  async function start(fresh=false) {
    if(busy||running(job)||q('[data-organize-start]').disabled)return;
    remember();busy=true;controls();message('正在创建整理任务…');
    try {
      const scope=await request({action:'organization-status'});
      if(!alive)return;
      for(const source of scope.worktrees||[])await api.approveDir(source.path);
      if(!alive)return;
      const resume_job_id=fresh!==true&&resumable(job)?job.id:undefined;
      const result=await request({action:'organize',runner,focus,resume_job_id,idempotency_key:crypto.randomUUID()});
      if(!alive)return;
      job={id:result.job_id,status:'queued',progress:['等待本机 AI 工具开始整理']};
      showJob();poll();
    } catch(e) {message(e.message);}
    finally {busy=false;if(alive)controls();}
  }
  async function poll() {
    clearTimeout(timer);
    if(!alive||!running(job))return;
    const id=job.id;
    try {
      const result=await api.job(id);
      if(!alive||job?.id!==id)return;
      job=result;showJob();controls();
      if(!running(job)){remember();await load();return;}
    } catch(e) {message('任务仍可能在执行，暂时无法读取进度：'+e.message);}
    if(alive&&running(job))timer=setTimeout(poll,1000);
  }
  async function cancel() {
    if(busy||!running(job)||job.cancel_requested)return;
    busy=true;controls();
    try {await api.cancelJob(job.id);if(alive){job.cancel_requested=true;message('已请求取消，正在停止本次整理。');}}
    catch(e){message(e.message);}
    finally{busy=false;if(alive)controls();}
  }
  root.innerHTML='<h2>整理知识</h2><p data-organize-message role="status">正在读取知识文档与本机工具…</p>';
  load();
  return {refresh:load,destroy(){alive=false;clearTimeout(timer);root.remove();}};
}
