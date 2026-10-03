import {esc} from '../services/api.js';

export function projectNavigation({id,name,active='capabilities'}) {
  const base='#/projects/'+encodeURIComponent(id);
  const tabs=[['capabilities','AI 能力',base],['instructions','项目说明',base+'/instructions'],['settings','设置',base+'/settings']];
  return `<div class="project-context"><div class="context-breadcrumb"><a href="#/projects">所有项目</a><span aria-hidden="true">/</span><strong>${esc(name)}</strong></div><nav class="section-nav" aria-label="项目导航">${tabs.map(([key,label,href])=>`<a href="${href}" ${key===active?'aria-current="page"':''}>${label}</a>`).join('')}</nav></div>`;
}

export function libraryNavigation(active='catalog') {
  return `<div class="library-context"><h1>资源库</h1><nav class="section-nav" aria-label="资源库导航"><a href="#/library" ${active==='catalog'?'aria-current="page"':''}>可复用能力</a><a href="#/library/global" ${active==='global'?'aria-current="page"':''}>跨项目配置</a></nav></div>`;
}
