import {api,esc} from '../services/api.js';

const states={new:'新资料',updated:'有更新',context_updated:'来源有更新',duplicate:'复用原文',unchanged:'已保存'};

export function KnowledgeIngestion(container,{rootPath,onSaved}) {
  const root=document.createElement('section');root.className='knowledge-ingestion';container.append(root);
  let alive=true,state=null,preview=null,busy=false,sourceId='';
  const q=s=>root.querySelector(s);
  const request=body=>api.knowledge({root:rootPath,...body});
  const message=text=>{if(alive)q('[data-ingest-message]').textContent=text;};
  const selected=()=>[...root.querySelectorAll('[data-ingest-document]:checked')].map(e=>e.value);
  function controls() {
    root.querySelectorAll('button,select,input').forEach(e=>e.disabled=busy);
    const scan=q('[data-ingest-scan]');if(scan)scan.disabled=busy||!state?.sources?.length;
    const save=q('[data-ingest-save]');if(save){const count=selected().length;save.disabled=busy||!count;save.textContent=`保存选中 ${count} 篇`;}
  }
  function sourceInfo() {
    const source=state.sources.find(s=>s.id===sourceId);
    q('[data-ingest-path]').textContent=source?.path||'';
    const inside=source&&(state.knowledge_path===source.worktree||state.knowledge_path?.startsWith(source.worktree+'/'));
    q('[data-ingest-storage]').textContent=inside?'知识库位于这个工作目录内；删除整个工作目录前，请先迁移知识库。':'';
  }
  function renderPreview() {
    const slot=q('[data-ingest-preview]');
    if(!preview){slot.textContent='';return;}
    const origin=preview.origin;
    slot.innerHTML=`<p class="muted">${esc(origin.branch||'无分支')} · ${esc(origin.commit?.slice(0,12)||'尚无提交')}${origin.worktree_dirty?' · 工作目录有未提交内容':''}</p>
      ${preview.documents.length?`<fieldset><legend>选择开发文档</legend><div class="knowledge-ingestion-documents">${preview.documents.map(d=>`<label><input type="checkbox" data-ingest-document value="${esc(d.path)}" ${d.state==='unchanged'?'':'checked'}><span><span class="path">${esc(d.path)}</span><small class="muted">${esc(states[d.state]||d.state)} · ${esc(d.title)}</small></span></label>`).join('')}</div></fieldset><div class="actions"><button type="button" data-ingest-all>全选</button><button type="button" data-ingest-none>清空选择</button><button type="button" class="primary" data-ingest-save>保存选中文档</button></div>`:'<p>这个目录没有可采集的 Markdown 文档。</p>'}
      ${preview.skipped.length?`<details><summary>${preview.skipped.length} 篇文档未读取</summary><ul>${preview.skipped.map(d=>`<li>${esc(d.path)}：${esc(d.reason)}</li>`).join('')}</ul></details>`:''}`;
    slot.querySelectorAll('[data-ingest-document]').forEach(e=>e.onchange=controls);
    if(q('[data-ingest-all]'))q('[data-ingest-all]').onclick=()=>{slot.querySelectorAll('[data-ingest-document]').forEach(e=>e.checked=true);controls();};
    if(q('[data-ingest-none]'))q('[data-ingest-none]').onclick=()=>{slot.querySelectorAll('[data-ingest-document]').forEach(e=>e.checked=false);controls();};
    if(q('[data-ingest-save]'))q('[data-ingest-save]').onclick=save;
    controls();
  }
  function render() {
    if(!state.sources.some(s=>s.id===sourceId))sourceId=state.sources.find(s=>s.path===rootPath)?.id||state.sources[0]?.id||'';
    root.innerHTML=`<h2>采集开发文档</h2><p class="muted">从主工作目录、其他 Worktree 和子目录逐批汇入 Markdown 开发资料。相同内容复用，修订保留历史。</p><p data-ingest-count>已采集 ${esc(state.document_count)} 份当前原文 · ${esc(state.revision_count)} 条来源版本记录</p>
      <div class="knowledge-source-options"><label>资料来源<select data-ingest-source aria-label="开发文档的来源目录">${state.sources.map(s=>`<option value="${esc(s.id)}" ${s.id===sourceId?'selected':''}>${esc(s.label)}</option>`).join('')}</select></label><button type="button" data-ingest-scan>扫描文档</button><button type="button" data-ingest-refresh>刷新目录</button></div>
      <p class="path muted" data-ingest-path></p><p class="muted" data-ingest-storage></p><div data-ingest-preview></div><p data-ingest-message role="status" aria-live="polite"></p>`;
    q('[data-ingest-source]').onchange=()=>{sourceId=q('[data-ingest-source]').value;preview=null;sourceInfo();renderPreview();message('');};
    q('[data-ingest-scan]').onclick=scan;q('[data-ingest-refresh]').onclick=()=>perform(async()=>{preview=null;renderPreview();await load();});
    sourceInfo();renderPreview();controls();
  }
  async function load(){const next=await request({action:'ingestion-status'});if(alive){state=next;render();}}
  async function perform(fn) {
    if(busy)return;busy=true;controls();
    try{await fn();}catch(e){message(e.message);}finally{busy=false;if(alive)controls();}
  }
  async function scan() {
    await perform(async()=>{
      const source=state.sources.find(s=>s.id===sourceId);if(!source)return;
      preview=null;renderPreview();
      message('正在扫描 Markdown 开发文档…');
      await api.approveDir(source.path);
      const next=await request({action:'ingestion-preview',source_id:sourceId});
      if(alive){preview=next;renderPreview();message(`扫描到 ${preview.documents.length} 篇，勾选后保存原文，再在下方整理知识。`);}
    });
  }
  async function save() {
    await perform(async()=>{
      const result=await request({action:'ingestion-apply',source_id:sourceId,expected:preview.expected,documents:selected()});
      if(!alive)return;
      preview=null;renderPreview();
      let refreshError='';
      try{await load();if(alive)await onSaved?.();}catch(e){refreshError=` 状态刷新失败，请重新读取：${e.message}`;}
      message(`已保存 ${result.saved} 篇：新增 ${result.new_documents} 份原文，复用 ${result.reused_documents} 份。现在可以整理知识。${refreshError}`);
    });
  }
  root.innerHTML='<p data-ingest-message role="status">正在读取资料来源…</p>';
  load().catch(e=>{if(alive){root.innerHTML='<p data-ingest-message role="status"></p><button type="button" data-ingest-retry>重新读取资料来源</button>';message(e.message);q('[data-ingest-retry]').onclick=()=>perform(load);}});
  return {isDirty:()=>busy,destroy(){alive=false;root.remove();}};
}
