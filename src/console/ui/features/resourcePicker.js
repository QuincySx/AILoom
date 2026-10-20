// AIL-101/113：资源选择 Dialog —— 按来源分组、搜索、勾选、已添加禁选、默认不全选。
// 只负责「选哪些资源」；保存动作由调用方提供（onSubmit 返回 Promise）：
//  - 全部成功：resolve 后关闭；
//  - 部分失败：resolve({failed:[{id,error}]})，仅失败项保留勾选并如实报告；
//  - 整体失败：reject，保留全部勾选与错误信息以便重试；取消零写入。
// AIL-113：无库存也可打开，并提供「＋导入」连续流程（importResources 可选）。
import { esc } from '../services/api.js';
import { Dialog } from '../components/dialog.js';
import { Item, resourcePath } from '../components/item.js';

const KIND_LABEL = {skill:'技能',mcp:'工具连接（MCP）',rule:'规则',agent:'子代理',doc:'文档',learning:'经验',env:'环境',hook:'Hook',package:'依赖包'};

export function ResourcePicker(container, { title, description, entries, isAdded, onSubmit, importResources, createResource, initialSelection = [], initialSearch = '', submitLabel = '添加所选', allowEmpty = false }) {
  const body = document.createElement('div');
  body.innerHTML = `
    ${description?`<p class="muted">${esc(description)}</p>`:''}
    <div class="toolbar picker-toolbar"><input type="search" data-picker-search aria-label="搜索资源" placeholder="搜索名称或描述"><select data-picker-kind aria-label="能力类型"><option value="">全部类型</option>${[...new Set(entries.map(e=>e.kind))].map(kind=>`<option value="${esc(kind)}">${esc(KIND_LABEL[kind]||kind)}</option>`).join('')}</select><select data-picker-source aria-label="能力来源"><option value="">全部来源</option>${[...new Set(entries.map(e=>e.source_name||'其他来源'))].map(source=>`<option>${esc(source)}</option>`).join('')}</select>${createResource ? '<button type="button" data-variant="ghost" data-picker-create>新建</button>' : ''}${importResources ? '<button type="button" data-variant="ghost" data-picker-import>导入</button>' : ''}</div>
    <div data-picker-list class="picker-list" tabindex="0" aria-label="可选资源"></div>
    <p data-picker-error class="field-error" role="alert"></p>
    <footer class="dialog-actions"><span data-picker-count class="muted" role="status" aria-live="polite"></span><button type="button" data-variant="ghost" data-picker-only>仅看已选</button><button type="button" data-variant="ghost" data-picker-clear>清空</button><button type="button" data-picker-cancel>取消</button><button type="submit" class="primary" data-picker-submit disabled>添加所选</button></footer>`;
  const q = s => body.querySelector(s);
  let items = entries;
  let selected = new Set();
  q('[data-picker-search]').value=initialSearch;
  let saving = false, onlySelected=false;
  let disposed = false;
  const modal = Dialog(container, {
    title,
    content: body,
    open: false, // 仅在点击「添加」时打开；否则会在切页时自动弹出
    canClose: () => !saving,
    onClose: () => { disposed = true; },
  });
  function render() {
    const query=q('[data-picker-search]').value.trim().toLowerCase(),kind=q('[data-picker-kind]').value,source=q('[data-picker-source]').value;
    const filtered=items.filter(e=>(!kind||e.kind===kind)&&(!source||(e.source_name||'其他来源')===source)&&(!onlySelected||selected.has(e.id))&&`${e.name||e.id} ${e.description||''} ${e.source_name||''}`.toLowerCase().includes(query));
    const map=new Map();
    for(const entry of filtered){const source=entry.source_name||'其他来源';if(!map.has(source))map.set(source,[]);map.get(source).push(entry);}
    q('[data-picker-list]').innerHTML=filtered.length?[...map.entries()].map(([source,list])=>`<fieldset class="picker-group"><legend>${esc(source)}</legend>${list.map(e=>{
      const added=isAdded(e.id),na=e.unavailable;
      return Item({tag:'label',title:e.name||e.id,description:e.description||KIND_LABEL[e.kind]||e.kind,meta:e.hint||'',path:resourcePath(e),className:'picker-row',density:'regular',leading:`<input type="checkbox" data-picker-item value="${esc(e.id)}" ${added||na||saving?'disabled':''} ${selected.has(e.id)?'checked':''}>`,actions:added?'<span class="badge ok">已添加</span>':'',feedback:na?`<span class="field-error">${esc(na)}</span>`:''});
    }).join('')}</fieldset>`).join(''):`<p class="muted">${items.length?'没有匹配的资源。':'还没有可选资源。'}</p>`;
    q('[data-picker-list]').querySelectorAll('[data-picker-item]').forEach(box=>{box.onchange=()=>{box.checked?selected.add(box.value):selected.delete(box.value);syncFooter();};});
    syncFooter();
  }
  function syncFooter() {
    q('[data-picker-submit]').textContent = selected.size ? `${submitLabel}（${selected.size}）` : submitLabel;
    q('[data-picker-submit]').disabled = saving || (!allowEmpty && !selected.size);
    q('[data-picker-count]').textContent = `已选 ${selected.size} 项`;
    q('[data-picker-only]').setAttribute('aria-pressed',String(onlySelected));
  }
  q('[data-picker-search]').oninput=()=>{render();};
  q('[data-picker-kind]').onchange=q('[data-picker-source]').onchange=()=>{render();};
  q('[data-picker-only]').onclick=()=>{if(saving)return;onlySelected=!onlySelected;render();};
  q('[data-picker-clear]').onclick=()=>{if(saving)return;selected.clear();render();};
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
    body.querySelectorAll('input,select,[data-picker-clear],[data-picker-only]').forEach(el=>el.disabled=true);
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
        body.querySelectorAll('input,select,[data-picker-clear],[data-picker-only]').forEach(el=>el.disabled=false);
        render();
      }
    }
  };
  render();
  return {
    show: () => { selected = new Set(initialSelection.filter(id=>!isAdded(id)&&!items.find(e=>e.id===id)?.unavailable));onlySelected=false; render(); modal.show(); },
    destroy: () => { disposed = true; modal.destroy(); },
  };
}
