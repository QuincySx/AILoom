// AIL-112：项目里扫描出的未托管 Skill 的「接管说明」与两步删除。
// 删除由服务端按项目根重新扫描复核；第一步预览影响，第二步输入目录名确认；取消全程零写入。
import {api,esc} from '../services/api.js';
import {Dialog} from '../components/dialog.js';

export function openSkillTakeover({path,name}) {
  const body=document.createElement('div');
  body.innerHTML=`<p>「${esc(name)}」由项目目录自行维护，AILoom 不会修改、移动或删除这里的文件。可以：</p>
    <p>1. 保持现状：AI 工具仍按原目录加载它，AILoom 只在扫描中展示。<br>
    2. 导入个人副本：在资源库导入后引用副本，原目录不动。<br>
    3. 不再需要时用「删除…」：双重确认，原目录移入本机归档，可恢复。</p>
    <p class="path">${esc(path)}</p>`;
  Dialog(document.body,{title:`接管说明 · ${name}`,content:body,actions:[{label:'关闭'}],canClose:()=>true});
}

export async function openSkillDelete({root,sub,path,name,onDone}) {
  let p;
  try { p=await api.projectDeletePreview(root,sub,path); }
  catch(e) { onDone?.({error:e.message}); return; }
  const size=(p.bytes||0)>=1048576?`${(p.bytes/1048576).toFixed(1)} MB`:`${Math.max(1,Math.round((p.bytes||0)/1024))} KB`;
  const body=document.createElement('div');
  body.innerHTML=`<p>准备删除未托管 Skill「${esc(name)}」的原目录，删除后 AI 工具不再从这里加载它。</p>
    <table><tr><th scope="row">目录</th><td class="path">${esc(p.path)}</td></tr>
    <tr><th scope="row">规模</th><td>${p.is_symlink?`符号链接 → ${esc(p.link_target||'')}：只摘除链接本身，不触碰目标`:`${p.files} 个文件 · 约 ${size}`}</td></tr>
    <tr><th scope="row">影响</th><td>资源库与其他项目不受影响；链接目标不会被删除。</td></tr>
    <tr><th scope="row">恢复</th><td>${p.is_symlink?'本机归档 project-archive 记录原链接目标，可按记录重建链接。':'目录移入本机归档 project-archive，可移回原路径恢复。'}</td></tr></table>`;
  const confirm=()=>{
    const form=document.createElement('div');
    form.innerHTML=`<p>请输入目录名 <code class="path">${esc(name)}</code> 确认删除：</p>
      <label>目录名称<input data-delete-name autocomplete="off" spellcheck="false"></label>
      <p class="field-error" data-delete-err role="alert"></p>`;
    Dialog(document.body,{title:`确认删除 · ${name}`,content:form,canClose:()=>true,actions:[
      {label:'确认删除',variant:'destructive',onAction:async()=>{
        const typed=form.querySelector('[data-delete-name]').value.trim();
        try { const r=await api.projectDeleteExecute(root,sub,p.token,typed); onDone?.({archived_to:r.archived_to}); }
        catch(e) { form.querySelector('[data-delete-err]').textContent=e.message; return false; }
      }},
      {label:'取消',returnValue:false},
    ]});
  };
  const first=Dialog(document.body,{title:`删除未托管 Skill · ${name}`,content:body,canClose:()=>true,actions:[
    {label:'继续：输入名称确认',onAction:()=>{first.close();confirm();}},
    {label:'取消',returnValue:false},
  ]});
}
