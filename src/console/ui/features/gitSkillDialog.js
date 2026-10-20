import {api, esc} from '../services/api.js';
import {Dialog} from '../components/dialog.js';
import {notify} from '../state/store.js';
import {Item} from '../components/item.js';

export function GitSkillDialog(container, {onChanged} = {}) {
  const body = document.createElement('div');
  body.className = 'git-skill-picker';
  body.innerHTML = `<form data-git-form>
    <label>Git 仓库地址<div class="input-action"><input data-git-url required placeholder="https://github.com/owner/skills.git"><button data-git-scan type="submit">扫描仓库</button></div></label>
    <details class="git-version"><summary>指定分支 / 标签</summary><input data-git-ref aria-label="分支或标签" placeholder="默认分支"></details>
  </form><div class="git-picker-toolbar" hidden><input data-git-search type="search" aria-label="搜索仓库中的 Skills" placeholder="搜索名称、用途或路径…"></div>
  <div class="git-picker-selection" hidden><label><input data-git-all type="checkbox">全选当前可安装项</label><span data-git-visible class="muted"></span></div>
  <p data-git-message class="inline-status muted" role="status" aria-live="polite"></p>
  <div data-git-list class="git-picker-list"></div>
  <footer class="dialog-actions"><span data-git-count class="muted">安装到资源库，再添加到项目或全局。</span><button data-git-cancel type="button">取消</button><button data-git-install type="button" class="primary" disabled>安装所选</button></footer>`;
  const q = selector => body.querySelector(selector);
  let alive = true, loading = false, saving = false, generation = 0, preview = null;
  let selected = new Set();
  const modal = Dialog(container, {title:'从 Git 仓库安装 Skills', content:body, open:false, keepMounted:true,
    canClose:()=>!saving, onClose:()=>{++generation;loading=false;clear();}});
  function message(text, error=false) {
    q('[data-git-message]').textContent=text;
    q('[data-git-message]').classList.toggle('field-error',error);
    q('[data-git-message]').setAttribute('role',error?'alert':'status');
  }
  function clear() { preview=null;selected.clear();q('[data-git-list]').innerHTML='';q('.git-picker-toolbar').hidden=q('.git-picker-selection').hidden=true;sync(); }
  function visible() {
    const search=q('[data-git-search]').value.trim().toLowerCase();
    return (preview?.skills||[]).filter(s=>!search||`${s.name} ${s.description} ${s.repo_path}`.toLowerCase().includes(search));
  }
  const selectable = skill => !skill.installed && !skill.conflict;
  function sync() {
    modal.update({canClose:()=>!saving});
    q('[data-git-scan]').disabled=loading||saving;
    q('[data-git-url]').disabled=q('[data-git-ref]').disabled=loading||saving;
    q('[data-git-search]').disabled=saving;
    q('[data-git-cancel]').disabled=saving;
    q('[data-git-install]').disabled=loading||saving||!selected.size;
    q('[data-git-install]').textContent=saving?'正在安装…':selected.size?`安装所选（${selected.size}）`:'安装所选';
    const hiddenSelected=preview?selected.size-visible().filter(s=>selected.has(s.id)).length:0;
    q('[data-git-count]').textContent=preview?`已选 ${selected.size} 个${hiddenSelected?`（筛选外 ${hiddenSelected} 个）`:''}`:'安装到资源库，再添加到项目或全局。';
    q('[data-git-visible]').textContent=preview?`显示 ${visible().length} / ${preview.skills.length} 项`:'';
    const available=visible().filter(selectable);
    const picked=available.filter(s=>selected.has(s.id)).length;
    q('[data-git-all]').checked=available.length>0&&picked===available.length;
    q('[data-git-all]').indeterminate=picked>0&&picked<available.length;
    q('[data-git-all]').disabled=saving||!available.length;
    q('[data-git-list]').querySelectorAll('[data-git-item]').forEach(box=>{
      box.disabled=saving||!selectable(preview.skills.find(s=>s.id===box.value));
    });
  }
  function render() {
    const skills=visible();
    q('.git-picker-toolbar').hidden=!preview;
    q('.git-picker-selection').hidden=!preview;
    q('[data-git-list]').innerHTML=skills.length?skills.map(s=>Item({tag:'label',className:'picker-row',title:s.name,description:s.description,meta:s.repo_path||'仓库根目录',leading:`<input data-git-item type="checkbox" value="${esc(s.id)}" ${selected.has(s.id)?'checked':''} ${selectable(s)?'':'disabled'}>`,actions:s.installed?'<span class="badge">已安装</span>':s.conflict?'<span class="badge warn">需处理</span>':'',feedback:!s.installed&&s.conflict?`<span class="field-error">${esc(s.conflict)}</span>`:''})).join(''):'<p class="muted">没有匹配的 Skill。</p>';
    q('[data-git-list]').querySelectorAll('[data-git-item]').forEach(box=>{box.onchange=()=>{box.checked?selected.add(box.value):selected.delete(box.value);sync();};});
    sync();
  }
  async function scan() {
    if(loading||saving)return;
    clear();
    const url=q('[data-git-url]').value.trim(), ref=q('[data-git-ref]').value.trim()||undefined;
    if(!url){message('请填写 Git 仓库地址。',true);return;}
    const turn=++generation;loading=true;sync();message('正在扫描仓库中的 Skills…');
    try {
      if(url.startsWith('/')||url.startsWith('file://'))await api.approveDir(url.replace(/^file:\/\//,''));
      const result=await api.gitSkillsPreview(url,ref);
      if(!alive||turn!==generation)return;
      preview=result;render();
      const installed=result.skills.filter(s=>s.installed).length;
      message(`找到 ${result.skills.length} 个 Skill · ${installed} 个已安装 · ${result.skills.filter(selectable).length} 个可安装`);
    } catch(e) {if(alive&&turn===generation)message(e.message,true);}
    finally {if(alive&&turn===generation){loading=false;sync();}}
  }
  q('[data-git-form]').onsubmit=event=>{event.preventDefault();return scan();};
  q('[data-git-form]').oninput=()=>{++generation;clear();message('');};
  q('[data-git-search]').oninput=render;
  q('[data-git-all]').onchange=()=>{for(const skill of visible().filter(selectable))q('[data-git-all]').checked?selected.add(skill.id):selected.delete(skill.id);render();};
  q('[data-git-cancel]').onclick=()=>modal.close();
  q('[data-git-install]').onclick=async()=>{
    if(saving||loading||!preview||!selected.size)return;
    if(selected.size>100){message('一次最多安装 100 个 Skill，请减少勾选。',true);return;}
    saving=true;sync();message('正在安装所选 Skills…');
    try {
      const result=await api.gitSkillsInstall(preview.preview_id,[...selected]);
      if(!alive)return;
      for(const item of result.imported||[]){const skill=preview.skills.find(s=>s.id===item.id);if(skill)skill.installed=true;}
      selected=new Set((result.failed||[]).map(item=>item.id));
      let refreshError='';
      if(result.imported?.length){
        notify(`已安装 ${result.imported.length} 个 Skill，可在项目或全局中添加使用。`);
        try{await onChanged?.();}catch(e){refreshError=`列表刷新失败：${e.message}。已安装项已保存，可重新打开资源库查看。`;}
      }
      if(!alive)return;
      if(result.failed?.length){message(`已安装 ${result.imported.length} 个；${result.failed.length} 个失败：${result.failed.map(s=>`${s.name||s.id}（${s.error}）`).join('；')}。失败的勾选已保留，可重试。${refreshError}`,true);render();}
      else if(refreshError){message(`已安装 ${result.imported.length} 个。${refreshError}`,true);render();}
      else modal.close();
    } catch(e) {if(alive)message(`安装失败：${e.message}。勾选已保留。`,true);}
    finally {saving=false;if(alive)sync();}
  };
  return {show:({url='',ref=''}={})=>{clear();message('');q('[data-git-url]').value=url;q('[data-git-ref]').value=ref||'';q('[data-git-search]').value='';modal.show();if(url)scan();},destroy:()=>{alive=false;++generation;modal.destroy();}};
}
