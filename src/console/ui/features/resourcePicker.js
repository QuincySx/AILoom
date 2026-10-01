// AIL-101/113：资源选择 Dialog —— 按来源分组、搜索、勾选、已添加禁选、默认不全选。
// 只负责「选哪些资源」；保存动作由调用方提供（onSubmit 返回 Promise）：
//  - 全部成功：resolve 后关闭；
//  - 部分失败：resolve({failed:[{id,error}]})，仅失败项保留勾选并如实报告；
//  - 整体失败：reject，保留全部勾选与错误信息以便重试；取消零写入。
// AIL-113：无库存也可打开，并提供「＋导入」连续流程（importResources 可选）。
import { esc } from '../services/api.js';
import { Dialog } from '../components/dialog.js';

export function ResourcePicker(container, { title, description, entries, isAdded, onSubmit, importResources, createResource }) {
  const body = document.createElement('div');
  body.innerHTML = `
    <input type="search" data-picker-search aria-label="搜索资源" placeholder="搜索名称、说明或来源…">
    <div data-picker-list class="picker-list" aria-label="可选资源"></div>
    <p data-picker-error class="field-error" role="alert"></p>
    <footer class="dialog-actions"><span data-picker-count class="muted"></span>${createResource ? '<button type="button" data-picker-create>＋新建</button>' : ''}${importResources ? '<button type="button" class="primary" data-picker-import>＋导入</button>' : ''}<button type="button" data-picker-cancel>取消</button><button type="submit" class="primary" data-picker-submit disabled>添加所选</button></footer>`;
  const q = s => body.querySelector(s);
  let items = entries;
  let selected = new Set();
  let saving = false;
  let disposed = false;
  const modal = Dialog(container, {
    title,
    content: body,
    open: false, // 仅在点击「添加」时打开；否则会在切页时自动弹出
    keepMounted: true, // 关闭后保留 DOM，同一 picker 可再次打开
    canClose: () => !saving,
    onClose: () => {},
  });
  const groups = () => {
    const query = q('[data-picker-search]').value.trim().toLowerCase();
    const map = new Map();
    for (const e of items) {
      // F03：字段缺失时回退可读（不显示 undefined）
      const label = e.name || e.id;
      if (query && !`${label} ${e.description || ''} ${e.source_name || ''}`.toLowerCase().includes(query)) continue;
      const source = e.source_name || '其他来源';
      if (!map.has(source)) map.set(source, []);
      map.get(source).push(e);
    }
    return map;
  };
  function render() {
    const map = groups();
    const total = [...map.values()].flat().length;
    q('[data-picker-list]').innerHTML = total ? [...map.entries()].map(([source, list]) => `
      <fieldset class="picker-group"><legend>${esc(source)} · ${list.length} 项</legend>
      ${list.map(e => {
        const added = isAdded(e.id);
        const na = e.unavailable;
        return `<label class="picker-row"><input type="checkbox" data-picker-item value="${esc(e.id)}" ${added || na ? 'disabled' : ''} ${selected.has(e.id) ? 'checked' : ''}>
          <span><strong>${esc(e.name || e.id)}</strong>${added ? ' <span class="badge">已添加</span>' : ''}${na ? ` <span class="badge warn">${esc(na)}</span>` : ''}<br>
          <span class="muted">${esc(e.description || '尚未提供说明')}</span>${e.hint?`<br><small class="muted">${esc(e.hint)}</small>`:''}<br>
          </span></label>`;
      }).join('')}</fieldset>`).join('')
      : `<p class="muted">${items.length ? '没有匹配的资源。' : '还没有可选资源。'}</p>`;
    q('[data-picker-list]').querySelectorAll('[data-picker-item]').forEach(box => {
      box.onchange = () => { box.checked ? selected.add(box.value) : selected.delete(box.value); syncFooter(); };
    });
    syncFooter();
  }
  function syncFooter() {
    q('[data-picker-submit]').textContent = selected.size ? `添加所选（${selected.size}）` : '添加所选';
    q('[data-picker-submit]').disabled = saving || !selected.size;
    q('[data-picker-count]').textContent = `已勾选 ${selected.size} 项`;
  }
  q('[data-picker-search]').oninput = render;
  q('[data-picker-cancel]').onclick = () => modal.close();
  if (createResource) q('[data-picker-create]').onclick = async () => {
    if(saving)return;
    const button=q('[data-picker-create]');button.disabled=true;
    try {const result=await createResource();if(disposed||!result)return;items=result.entries??items;for(const id of result.selectIds??[])selected.add(id);render();}
    catch(e){if(!disposed)q('[data-picker-error]').textContent=e.message;}
    finally {if(!disposed)button.disabled=false;}
  };
  if (importResources) {
    q('[data-picker-import]').onclick = async () => {
      const btn = q('[data-picker-import]');
      if (btn.disabled || saving) return;
      btn.disabled = true;
      try {
        const r = await importResources();
        if (disposed || !r) return;
        items = r.entries ?? items;
        for (const id of r.selectIds ?? []) selected.add(id);
        render();
      } finally { if (!disposed) btn.disabled = false; }
    };
  }
  q('[data-picker-submit]').onclick = async () => {
    const submit = q('[data-picker-submit]');
    if (submit.disabled) return;
    saving = true;
    submit.disabled = true;
    q('[data-picker-cancel]').disabled = true;
    q('[data-picker-search]').disabled = true;
    // 保存期间锁住勾选与搜索，避免批次中途变化（AIL-114/F05）。
    q('[data-picker-list]').querySelectorAll('[data-picker-item]').forEach(box => { box.disabled = true; });
    q('[data-picker-error]').textContent = '';
    try {
      const result = await onSubmit([...selected]);
      const failed = result?.failed ?? [];
      if (failed.length) {
        const okCount = selected.size - failed.length;
        selected = new Set(failed.map(f => f.id));
        q('[data-picker-error]').textContent =
          `已保存 ${okCount} 项；${failed.length} 项失败：${failed.map(f => `${f.id}（${f.error}）`).join('；')}。失败的勾选已保留，可重试。已入库/已保存的资源不回退。`;
      } else {
        modal.close();
      }
    } catch (e) {
      q('[data-picker-error]').textContent = `保存失败：${e.message}。勾选已保留，可直接重试。`;
    } finally {
      saving = false;
      if (!disposed) {
        submit.disabled = false;
        q('[data-picker-cancel]').disabled = false;
        q('[data-picker-search]').disabled = false;
        render();
      }
    }
  };
  render();
  return {
    show: () => { selected = new Set(); render(); modal.show(); },
    destroy: () => { disposed = true; modal.destroy(); },
  };
}
