import {esc} from '../services/api.js';

export function projectNavigation({id,active='capabilities',settings=false}) {
  const base='#/projects/'+encodeURIComponent(id);
  const tabs=settings
    ? [['settings','基本信息',base+'/settings'],['instructions','项目说明',base+'/instructions'],['knowledge','知识库',base+'/knowledge']]
    : [['capabilities','AI 能力',base],['files','本地文件',base+'/files'],['settings','项目设置',base+'/settings']];
  return `<nav class="section-nav" aria-label="${settings?'项目设置':'项目导航'}">${tabs.map(([key,label,href])=>`<a href="${href}" ${key===active?'aria-current="page"':''}>${esc(label)}</a>`).join('')}</nav>`;
}

export function libraryNavigation(active='catalog') {
  const tabs=[['catalog','资源库','#/library'],['files','规则与子代理','#/library/rules']];
  if(active==='catalog'||active==='global')return '';
  return `<div class="library-context"><nav class="section-nav" aria-label="资源库导航">${tabs.map(([key,label,href])=>`<a href="${href}" ${active===key?'aria-current="page"':''}>${label}</a>`).join('')}</nav></div>`;
}
