import {NativeFiles} from '../features/nativeFiles.js';
import {GlobalSkills} from '../features/globalSkills.js';
import {confirmAction} from '../components/dialog.js';
import {libraryNavigation} from '../components/sectionNav.js';
// AIL-152：全局配置 = 全局 Skill + 用户级 Rules / Agent 原生文件
export function mount(container){
  const root=document.createElement('div');root.className='native-files-page';container.append(root);
  root.innerHTML=libraryNavigation('global')+'<header class="page-head"><div><span class="kicker">适用于所有项目</span><h2>跨项目配置</h2></div></header><nav class="project-tabs" aria-label="跨项目配置"><button data-tab="skills">技能 · Skill</button><button data-tab="files">规则与子代理</button></nav><div data-content></div>';
  let tab='skills',panel=null;
  const content=root.querySelector('[data-content]');
  function show(next){
    panel?.destroy();content.innerHTML='';tab=next;
    root.querySelectorAll('[data-tab]').forEach(b=>{const on=b.dataset.tab===tab;b.classList.toggle('on',on);b.setAttribute('aria-selected',String(on));});
    panel=tab==='skills'?GlobalSkills(content):NativeFiles(content);
  }
  root.querySelectorAll('[data-tab]').forEach(b=>{b.onclick=async()=>{
    if(b.dataset.tab===tab)return;
    if(panel?.isDirty?.()&&!await confirmAction('放弃未保存的修改？',{title:'切换栏目',confirmLabel:'放弃并切换'}))return;
    show(b.dataset.tab);
  };});
  show('skills');
  return {isDirty:()=>Boolean(panel?.isDirty?.()),destroy(){panel?.destroy();root.remove();}};
}
