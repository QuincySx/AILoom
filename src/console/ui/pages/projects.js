import { api, esc } from '../services/api.js';
import { setTarget } from '../state/target.js';
import { ProjectDialog } from '../features/projectDialog.js';
import { PageHeader } from '../components/layout.js';
import { Item, paginate, Pagination, bindPagination } from '../components/item.js';
import { icon } from '../components/icons.js';

// 项目首页；配置入口始终由用户明确选择，目录内配置交给 workspace。
const projectName = r => r.project?.name || Object.values(r.worktrees || {})[0]?.path?.split('/').filter(Boolean).pop() || r.repo_id;

export function mount(container) {
  const root = document.createElement('div');
  root.className='projects-page';
  container.append(root);
  let disposed = false, listPage = 1;
  const dialogs = [];
  const error = e => { if (!disposed) (root.querySelector('dialog[open] [data-form-error]') || root.querySelector('[data-message]')).textContent = e.message; };
  root.innerHTML = '<p data-message role="status">正在读取项目…</p>';
  async function load() {
    try {
      const state = await api.state();
      if (disposed) return;
      // 文件夹项目没有 Git Worktree；后端已按 common_dir 合成 local（含失联状态），
      // 这里仅对缺失字段的数据兜底，不覆盖后端的 active/missing 判定。
      state.repos = state.repos.map(r => r.repo_id.startsWith('nongit-') && !Object.keys(r.worktrees || {}).length
        ? {...r, worktrees:{local:{path:r.common_dir, status:'active'}}} : r);
      list(state);
    } catch (e) { error(e); }
  }
  function list(state) {
    setTarget(null);
    if(!state.repos.length){
      root.innerHTML=`<section class="project-start"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 7h7l2-3h9v16H3z"/></svg><h1>给项目配好 AI</h1><p>选择你正在工作的文件夹。</p><button data-new data-add-project class="primary">添加第一个项目</button><ol class="welcome-steps"><li><span>1</span>选择项目</li><li><span>2</span>选择工具与能力</li><li><span>3</span>预览并应用</li></ol><a href="#/library">先准备可复用能力 →</a><p data-message role="status"></p></section>`;
      root.querySelector('[data-new]').onclick=()=>dialogs.push(ProjectDialog(root,{onCreated:id=>{location.hash='#/projects/'+encodeURIComponent(id);}}));
      return;
    }
    root.innerHTML = `${PageHeader({title:'项目',description:'选择已有本机项目，不会自动启用资源。',actions:'<button data-new data-add-project data-variant="default">添加项目</button>'})}
      <p data-message role="status"></p><div class="toolbar project-toolbar"><input data-search type="search" aria-label="搜索项目" placeholder="搜索名称、路径或分类"><select data-filter aria-label="项目分类"><option value="">全部分类</option></select><select data-kind aria-label="项目类型"><option value="">全部类型</option><option value="git">Git 项目</option><option value="nongit">文件夹项目</option></select></div><div class="project-grid" data-list></div><div data-pagination></div>`;
    root.querySelector('[data-new]').onclick = () => { dialogs.push(ProjectDialog(root,{onCreated:id=>{location.hash='#/projects/'+encodeURIComponent(id);}})); };
    const filter = root.querySelector('[data-filter]');
    const categories = [...new Set(state.repos.map(r => r.project?.category || '未分类'))].sort();
    filter.innerHTML += categories.map(c => `<option>${esc(c)}</option>`).join('');
    const render = () => {
      const query = root.querySelector('[data-search]').value.trim().toLowerCase();
      const kind = root.querySelector('[data-kind]').value;
      const repos = state.repos.filter(r => (!filter.value || (r.project?.category || '未分类') === filter.value)
        && (!kind || (r.repo_id.startsWith('nongit-') ? 'nongit' : 'git') === kind)
        && `${projectName(r)} ${r.origin_normalized || ''} ${Object.values(r.worktrees || {}).map(w => w.path).join(' ')}`.toLowerCase().includes(query));
      const page=paginate(repos.sort((a,b)=>projectName(a).localeCompare(projectName(b))),listPage);listPage=page.page;
      root.querySelector('[data-list]').innerHTML=page.items.length?page.items.map(r=>{
        const worktrees=Object.values(r.worktrees||{}),active=worktrees.find(w=>w.status!=='missing'),base='#/projects/'+encodeURIComponent(r.repo_id),folder=r.repo_id.startsWith('nongit-');
        const path=(active||worktrees[0])?.path||r.common_dir;
        return Item({title:projectName(r),primaryAction:{href:base,label:'打开项目 '+projectName(r),attributes:'data-project-open'},description:path?.split('/').filter(Boolean).slice(-2).join(' / '),meta:!active?'目录失联':'',leading:icon('folder'),trailing:`<span class="project-kind">${folder?'文件夹项目':`${worktrees.length} 个工作目录`}</span>`,className:'project-card',density:'regular'});
      }).join(''):'<section class="empty-state"><h2>没有匹配的项目</h2><button data-clear-filters>清除筛选</button></section>';
      root.querySelector('[data-pagination]').innerHTML=Pagination(page);
      bindPagination(root.querySelector('[data-pagination]'),next=>{listPage=next;render();});
      root.querySelector('[data-clear-filters]')?.addEventListener('click',()=>{root.querySelector('[data-search]').value='';filter.value='';root.querySelector('[data-kind]').value='';filter.dispatchEvent(new Event('change',{bubbles:true}));root.querySelector('[data-kind]').dispatchEvent(new Event('change',{bubbles:true}));render();});
      root.querySelector('[data-first]')?.addEventListener('click',()=>root.querySelector('[data-new]').click());
    };
    root.querySelector('[data-search]').oninput = ()=>{listPage=1;render();};
    filter.onchange = ()=>{listPage=1;render();};
    root.querySelector('[data-kind]').onchange = ()=>{listPage=1;render();};
    render();
  }
  load();
  return {isDirty:() => false, destroy() {disposed = true; dialogs.forEach(c => c.destroy()); setTarget(null); root.remove();}};
}
