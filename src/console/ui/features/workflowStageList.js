// AIL-091：WorkflowStageList —— 流程阶段、skill 绑定（真实资源身份）、产物列表/编辑、
// 输入版本记录；缺项显式缺失、需复核可见。绑定/保存等动作经注入回调走 api。

import { api, esc } from '../services/api.js';

export function WorkflowStageList(container, props) {
  const wrap = document.createElement('div');
  container.appendChild(wrap);
  let cur = props;
  let dirty = false;

  function render(p) {
    cur = p;
    const run = p.run;
    if (!run) { wrap.innerHTML = ''; return; }
    const skills = (p.skills ?? []);
    const skillOpts = skills.map((s) => `<option value="${esc(s.id)}">${esc(s.id)}</option>`).join('');
    const pack = (run.pack ?? []).map((b) => `<tr>
      <td>${esc(b.stage)}</td><td>${esc(b.resource_id ?? '未绑定')}</td>
      <td>${esc(b.status).startsWith('missing') || b.status === 'missing' ? `<span class="badge bad">${esc(b.status)}</span>` : `<span class="badge ok">${esc(b.status)}</span>`}</td>
      <td><select data-bind="${esc(b.stage)}"><option value="">（选择 skill 绑定）</option>${skillOpts}</select>
      <button data-do="bind" data-stage="${esc(b.stage)}">绑定</button></td></tr>`).join('');
    const inputs = (run.inputs ?? []).map((i) =>
      `<tr><td>${esc(i.identity)}</td><td class="muted">${esc(i.digest.slice(0, 16))}…</td></tr>`).join('');
    const arts = (run.artifacts ?? []).map((a) => `<tr>
      <td>${esc(a.stage)}</td><td>${esc(a.title)}</td>
      <td>v${a.version}${a.needs_review ? ' <span class="badge warn">需复核</span>' : ''}</td>
      <td>
        <button data-do="open" data-art="${esc(a.id)}">查看</button>
        <button data-do="edit" data-art="${esc(a.id)}" data-title="${esc(a.title)}" data-ver="${a.version}">编辑</button>
        <button data-do="rename" data-art="${esc(a.id)}">改名</button>
        <button data-do="export" data-art="${esc(a.id)}">导出</button>
      </td></tr>`).join('');
    wrap.innerHTML = `
      <p>规格版本 v${run.spec_version} ${run.downstream_needs_review ? '<span class="badge warn">下游产物需复核</span>' : ''}
        <button data-do="reviewed">标记已复核</button>
        <input data-input-res placeholder="记录输入版本：资源 ID" style="width:26%">
        <button data-do="record">记录输入</button></p>
      ${inputs ? `<table><tr><th>输入版本</th><th>摘要</th></tr>${inputs}</table>` : ''}
      <table><tr><th>阶段</th><th>绑定 skill</th><th>状态</th><th>绑定操作</th></tr>${pack}</table>
      <table><tr><th>阶段</th><th>产物</th><th>版本</th><th>操作</th></tr>${arts}</table>
      <p class="muted">绑定校验走真实资源身份：缺项显示缺失，不从未知位置静默复制。规格变更后下游需复核。产物保存带版本前置。</p>
      <div>新增产物：阶段 <select data-stage>${['align', 'spec', 'tickets', 'implementation', 'acceptance'].map((x) => `<option>${x}</option>`).join('')}</select>
      标题 <input data-title style="width:26%"> 或导入现有文件 <input data-import style="width:26%" placeholder="已批准目录内路径">
      <button data-do="put">写入</button><button data-do="import">从文件导入</button></div>
      <textarea data-content placeholder="产物正文（保存在机器数据区，默认不进公司仓库）"></textarea>
      <div data-art class="log hidden"></div>`;
    wire(run);
    for (const selector of ['[data-content]', '[data-title]', '[data-stage]']) {
      wrap.querySelector(selector).addEventListener('input', () => { dirty = true; });
    }
  }

  function wire(run) {
    wrap.querySelectorAll('[data-do]').forEach((b) => {
      b.onclick = async () => {
        const act = b.dataset.do;
        const q = (sel) => wrap.querySelector(sel);
        if (dirty && !['put', 'open', 'export'].includes(act) && !confirm('当前产物有未保存修改，确定放弃后继续？')) return;
        try {
          if (act === 'bind') {
            const stage = b.dataset.stage;
            const rid = q(`[data-bind="${CSS.escape(stage)}"]`).value;
            if (!rid) return;
            await api.workflowBind(run.id, [[stage, rid]]);
            notify(`已绑定 ${stage} → ${rid}`);
          } else if (act === 'record') {
            const rid = q('[data-input-res]').value.trim();
            if (rid) { await api.workflowRecordInput(run.id, rid); notify('输入版本已记录'); }
          } else if (act === 'reviewed') {
            await api.workflowReviewed(run.id);
          } else if (act === 'put') {
            const base = q('[data-content]').dataset.baseVersion;
            await api.workflowArtifact(run.id, q('[data-stage]').value, q('[data-title]').value.trim() || '未命名', q('[data-content]').value, base ? parseInt(base, 10) : undefined);
            delete q('[data-content]').dataset.baseVersion;
            dirty = false;
          } else if (act === 'import') {
            const path = q('[data-import]').value.trim();
            if (!path) return;
            const f = await api.fsRead(path);
            await api.workflowArtifact(run.id, q('[data-stage]').value, q('[data-title]').value.trim() || path.split('/').pop(), f.content);
            notify('已导入为产物');
          } else if (act === 'open') {
            const v = await api.workflowArtifactRead(run.id, b.dataset.art);
            const slot = q('[data-art]');
            slot.classList.remove('hidden');
            slot.textContent = v.content;
            return;
          } else if (act === 'edit') {
            const v = await api.workflowArtifactRead(run.id, b.dataset.art);
            const show = await api.workflowShow(run.id);
            const art = (show.artifacts ?? []).find((a) => a.id === b.dataset.art);
            q('[data-stage]').value = art?.stage ?? q('[data-stage]').value;
            q('[data-title]').value = b.dataset.title;
            const content = q('[data-content]');
            content.value = v.content;
            content.dataset.baseVersion = b.dataset.ver;
            dirty = false;
            notify(`已载入 ${b.dataset.title} v${b.dataset.ver}；写入带版本前置`);
            return;
          } else if (act === 'rename') {
            const t = prompt('产物新名称（关联按 ID 保持）：');
            if (t) await api.workflowRename(run.id, b.dataset.art, t);
          } else if (act === 'export') {
            const target = prompt('导出目标文件（必须位于已批准目录内）：');
            if (!target) return;
            const pv = await api.exportPreview(run.id, b.dataset.art, target);
            const note = pv.exists ? (pv.same_content ? '（内容一致）' : '（将覆盖现有文件）') : '（新文件）';
            if (!confirm('写入 ' + pv.target + '？' + note)) return;
            await api.exportExecute(run.id, b.dataset.art, target, pv.target_fingerprint);
            notify('已导出（旧版本已备份；未提交 Git）');
            return; // 导出已有版本不丢弃正在编辑的正文。
          }
          dirty = false;
          cur.onChanged?.();
        } catch (e) {
          notify((e.kind === 'conflict' ? '冲突：' : '失败：') + e.message);
        }
      };
    });
  }

  render(props);
  return { isDirty: () => dirty, update(next) { if (!dirty) render({ ...cur, ...next }); }, destroy() { wrap.remove(); } };
}
