import { api, esc } from '../services/api.js';

const labels = {queued:'等待开始',running:'正在整理',success:'整理完成',failed:'整理失败',cancelled:'已取消',interrupted:'整理已中断'};
const running = job => job && ['queued','running'].includes(job.status);

export function KnowledgeOrganization(container, {rootPath}) {
  const root=document.createElement('section');root.className='knowledge-organization';container.append(root);
  let alive=true, state=null, job=null, timer=null, busy=false, checking=false, runner='codex', focus='';
  const q=s=>root.querySelector(s);
  const request=body=>api.knowledge({root:rootPath,...body});
  const message=text=>{if(alive)q('[data-organize-message]').textContent=text;};
  const remember=()=>{runner=q('[data-organize-runner]')?.value||runner;focus=q('[data-organize-focus]')?.value||'';};
  function controls() {
    const pending=busy||running(job);
    q('[data-organize-start]').disabled=checking||pending||!state?.source_count||!state.runners?.some(r=>r.id===runner&&r.available);
    q('[data-organize-runner]').disabled=checking||pending;
    q('[data-organize-focus]').disabled=pending;
    q('[data-organize-detect]').disabled=checking||pending;
    q('[data-organize-cancel]').hidden=!running(job);
    q('[data-organize-cancel]').disabled=busy||job?.cancel_requested;
    q('[data-organize-progress]').hidden=!running(job);
  }
  function render() {
    const tools=state.runners||[];
    if(!tools.some(r=>r.id===runner&&r.available))runner=tools.find(r=>r.available)?.id||'codex';
    const report=state.report;
    root.innerHTML=`<h2>整理知识</h2><p class="muted">合并零散经验，提炼仓库约定、排障方法和维护建议，保留原始记录与出处。</p>
      <p data-organize-count>${esc(state.source_count||0)} 篇知识文档可整理</p>
      <div class="knowledge-organization-options"><label>执行工具<select data-organize-runner aria-label="整理知识的执行工具">${tools.map(r=>`<option value="${esc(r.id)}" ${r.id===runner?'selected':''} ${r.available?'':'disabled'}>${esc(r.name)}${r.available?'':'（未找到 CLI）'}</option>`).join('')}</select></label><button type="button" data-organize-detect>重新检测</button></div>
      <label>整理重点（可选）<textarea data-organize-focus rows="2" maxlength="2000" placeholder="例如：硬件连接的排障经验、待改进的模块和回归测试建议">${esc(focus)}</textarea></label>
      <p class="muted">调用所选工具的本机配置和登录状态，使用其模型服务。整理结果会保存到本项目知识库，后续检索可用。</p>
      ${!state.source_count?'<p class="muted">先保存工作经验，再进行整理。配置恢复副本和资源快照不参与整理。</p>':''}
      <div class="actions"><button type="button" class="primary" data-organize-start>开始整理</button><button type="button" data-organize-cancel hidden>取消整理</button><a href="#/tasks">查看整理记录</a></div>
      <progress data-organize-progress aria-label="知识整理进度" hidden></progress><p data-organize-message role="status" aria-live="polite"></p>
      <details data-organize-report ${report?'open':''} ${report?'':'hidden'}><summary>仓库维护知识与建议</summary><pre class="knowledge-organization-report">${esc(report||'')}</pre></details>
      <details data-organize-draft hidden><summary>本次未保存的整理草稿</summary><pre class="knowledge-organization-report"></pre></details>`;
    q('[data-organize-runner]').onchange=()=>{remember();controls();};
    q('[data-organize-focus]').oninput=remember;
    q('[data-organize-detect]').onclick=()=>{remember();load();};
    q('[data-organize-start]').onclick=start;
    q('[data-organize-cancel]').onclick=cancel;
    showJob();controls();
  }
  function showJob() {
    if(!job)return;
    const note=job.progress?.at(-1)||'';
    message((labels[job.status]||job.status)+(job.error?'：'+job.error:note?' · '+note:''));
    const draft=q('[data-organize-draft]');
    draft.hidden=!(job.result?.report&&!job.result.saved&&!running(job));
    if(!draft.hidden)draft.querySelector('pre').textContent=job.result.report;
  }
  async function load() {
    if(checking||!alive)return;
    checking=true;
    if(state)controls();
    try {
      const result=await request({action:'organization-status'});
      if(!alive)return;
      state=result;job=result.latest_job;
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
  async function start() {
    if(busy||running(job)||q('[data-organize-start]').disabled)return;
    remember();busy=true;controls();message('正在创建整理任务…');
    try {
      const result=await request({action:'organize',runner,focus,idempotency_key:crypto.randomUUID()});
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
