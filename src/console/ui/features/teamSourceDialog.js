import {api} from '../services/api.js';
import {Dialog} from '../components/dialog.js';

const localAddress=value=>value.startsWith('/')||value.startsWith('file://')||/^[a-z]:[\\/]/i.test(value);

export function TeamSourceDialog(container,{rootPath,onSaved}) {
  const form=document.createElement('form');
  form.className='team-source-form';
  form.innerHTML=`<fieldset data-source-fields disabled>
    <div class="team-source-methods" role="group" aria-label="如何获取团队资源">
      <label><input type="radio" name="source-method" data-source-git value="git"><span><strong>从 Git 获取</strong><small>填写 SSH / HTTPS 地址</small></span></label>
      <label><input type="radio" name="source-method" data-source-existing value="local"><span><strong>使用已有文件夹</strong><small>选择已克隆或本地开发的资源库</small></span></label>
    </div>
    <fieldset data-source-remote hidden disabled>
      <label>Git 仓库地址<input data-source-url required placeholder="git@github.com:owner/resources.git" autocomplete="off" spellcheck="false"></label>
      <label>下载目录<div class="input-action"><input data-source-cache-root required spellcheck="false"><button type="button" data-source-cache-pick>更改…</button><button type="button" data-source-cache-default data-variant="ghost">恢复默认</button></div></label>
      <p class="muted team-source-requirement">目录会自动创建，下载的仓库分别保存。</p>
      <details><summary>分支或标签（可选）</summary><label>分支 / 标签<input data-source-ref placeholder="留空使用默认分支"></label></details>
    </fieldset>
    <fieldset data-source-local hidden disabled>
      <label>资源库文件夹<div class="input-action"><input data-source-directory required placeholder="选择资源库根目录" spellcheck="false"><button type="button" data-source-pick>选择文件夹…</button></div></label>
    </fieldset>
    <div class="team-source-storage" data-source-reading><strong>读取位置</strong><div><code data-source-storage class="path"></code><p class="muted">直接使用这个文件夹中的内容。</p></div></div>
    <p class="muted team-source-requirement">资源库根目录需包含 <code>ailoom.toml</code>。</p>
    </fieldset>
    <div data-source-error-panel class="team-source-error" role="alert" hidden><strong data-source-error-title></strong><p data-source-error></p><details data-source-error-details hidden><summary>技术详情</summary><pre data-source-error-cause class="log"></pre></details></div>
    <footer class="dialog-actions"><button type="button" data-source-cancel>取消</button><button type="submit" data-source-save class="primary" disabled>连接资源库</button></footer>`;
  const q=s=>form.querySelector(s);
  let alive=true,busy=true,base=null,root=rootPath,mode='local',defaultCacheRoot='';
  const modal=Dialog(container,{title:'修改团队来源',content:form,canClose:()=>!busy,onClose:()=>{alive=false;}});
  function clearError(){q('[data-source-error-panel]').hidden=true;q('[data-source-error-details]').open=false;}
  function showError(error){
    if(!alive)return;
    const data=error.data||{},code=data.code;
    const cause=data.context?.cause;
    const detail=cause?.context?.stderr||data.context?.stderr||cause?.message||'';
    let title='未能连接资源库',text=error.message;
    if(code==='E2002'||code==='E2007'){
      title=mode==='git'?'无法从 Git 获取资源':'无法读取这个文件夹';
      if(/permission denied.*publickey/i.test(detail))text='SSH 身份验证失败。请检查此电脑使用的 SSH 身份，以及该身份是否有仓库权限。';
      else if(/could not resolve hostname|could not resolve host/i.test(detail))text='无法找到 Git 服务器。请检查网络，以及 SSH 地址中的主机名或 Host 别名。';
      else if(/repository not found|does not appear to be a git repository/i.test(detail))text='找不到这个 Git 仓库。请检查仓库地址和访问权限。';
      else text=[error.message,data.hint].filter(Boolean).join('。');
    }else if(code==='E3002'&&error.message.includes('缺少 ailoom.toml')){
      title='已读取来源，但没有找到 ailoom.toml';
      text=mode==='git'?'请使用 AILoom 团队资源仓库，并确认 ailoom.toml 已提交到所选分支。':'请选择包含 ailoom.toml 的资源库根目录，或先在此资源库中创建团队配置。';
    }else if(code==='E3004'){
      title='来源已读取，但项目或角色不匹配';
      text=error.message+'。请选择原团队资源库，或核对新资源库中的项目和角色配置。';
    }
    q('[data-source-error-title]').textContent=title;
    q('[data-source-error]').textContent=text;
    q('[data-source-error-details]').hidden=!detail;
    q('[data-source-error-cause]').textContent=detail;
    q('[data-source-error-panel]').hidden=false;
  }
  function controls(){
    q('[data-source-fields]').disabled=busy;
    q('[data-source-git]').checked=mode==='git';
    q('[data-source-existing]').checked=mode==='local';
    q('[data-source-remote]').hidden=mode!=='git';
    q('[data-source-remote]').disabled=busy||mode!=='git';
    q('[data-source-local]').hidden=mode!=='local';
    q('[data-source-local]').disabled=busy||mode!=='local';
    q('[data-source-save]').disabled=busy||!base;
    q('[data-source-save]').textContent=busy&&base?(mode==='git'?'正在获取并连接…':'正在检查并连接…'):'连接资源库';
    q('[data-source-cancel]').disabled=busy;
    q('[data-source-reading]').hidden=mode==='git';
    q('[data-source-storage]').textContent=q('[data-source-directory]').value.trim()||'请选择资源库文件夹';
    q('[data-source-cache-default]').hidden=q('[data-source-cache-root]').value.trim()===defaultCacheRoot;
    modal.update({});
  }
  function choose(next){if(busy)return;mode=next;clearError();controls();}
  q('[data-source-git]').onchange=()=>choose('git');
  q('[data-source-existing]').onchange=()=>choose('local');
  q('[data-source-directory]').oninput=()=>{clearError();controls();};
  q('[data-source-url]').oninput=()=>{q('[data-source-ref]').value='';clearError();};
  q('[data-source-cache-root]').oninput=()=>{clearError();controls();};
  q('[data-source-cache-default]').onclick=()=>{q('[data-source-cache-root]').value=defaultCacheRoot;clearError();controls();};
  q('[data-source-cancel]').onclick=()=>modal.close();
  async function pick(selector){
    if(busy)return;busy=true;clearError();controls();
    try{const selected=await api.pickDirectory(selector==='[data-source-cache-root]'?'resource-storage':'resource-source');if(alive&&selected.path)q(selector).value=selected.path;}
    catch(e){showError(e);}
    finally{busy=false;if(alive)controls();}
  }
  q('[data-source-pick]').onclick=()=>pick('[data-source-directory]');
  q('[data-source-cache-pick]').onclick=()=>pick('[data-source-cache-root]');
  form.onsubmit=async event=>{
    event.preventDefault();if(busy||!base)return;
    const raw=q(mode==='git'?'[data-source-url]':'[data-source-directory]').value.trim();
    const value=mode==='local'?raw.replace(/^file:\/\//,''):raw;if(!value)return;
    if(mode==='git'&&localAddress(value)){showError(new Error('这是文件夹路径，请选择“使用已有文件夹”。'));return;}
    if(mode==='local'&&!localAddress(value)){showError(new Error('请选择本机文件夹；SSH / HTTPS 地址请填在“从 Git 获取”中。'));return;}
    const cacheRoot=mode==='git'?q('[data-source-cache-root]').value.trim():undefined;
    if(mode==='git'&&(!cacheRoot||!localAddress(cacheRoot)||cacheRoot.startsWith('file://'))){showError(new Error('下载目录需要本机绝对路径，可选择文件夹或恢复默认。'));return;}
    busy=true;clearError();controls();
    try{
      if(mode==='local')await api.approveDir(value);
      if(mode==='git'&&cacheRoot!==defaultCacheRoot)await api.approveDir(cacheRoot,true);
      await api.projectSource({action:'save',root,type:mode,value,ref:mode==='git'?q('[data-source-ref]').value.trim():'',cache_root:cacheRoot,base_fingerprint:base});
      await onSaved?.();busy=false;modal.close();
    }catch(e){showError(e);}
    finally{busy=false;if(alive)controls();}
  };
  (async()=>{
    try{
      const result=await api.projectSource({action:'read',root:rootPath});if(!alive)return;
      base=result.base_fingerprint;root=result.root;defaultCacheRoot=result.default_cache_root;
      q('[data-source-cache-root]').value=result.cache_root;
      const value=result.source.url||result.source.path||'';
      mode=result.source.type==='git'&&!localAddress(value)?'git':'local';
      q(mode==='git'?'[data-source-url]':'[data-source-directory]').value=value;
      q('[data-source-ref]').value=mode==='git'?(result.source.ref||''):'';
    }catch(e){showError(e);}
    finally{busy=false;if(alive)controls();}
  })();
  return {destroy(){alive=false;modal.destroy();}};
}
