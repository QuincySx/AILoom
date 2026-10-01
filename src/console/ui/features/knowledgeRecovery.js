import { esc } from '../services/api.js';

export function recoveryFields(recovery) {
  if (!recovery) return '';
  const source = recovery.declaration.source;
  return `<div data-recovery><p>发现项目配置。${source.type === 'project' ? '将关联项目内的知识库。' : '请选择已同步的知识库目录。'}</p>
    ${source.type === 'git' ? `<p class="muted path">${esc(source.url)}${source.subdir !== '.' ? ' · '+esc(source.subdir) : ''}</p><label>恢复方式<select data-recovery-mode><option value="existing">关联已有目录</option><option value="clone">从 Git 克隆到新目录</option></select></label>` : ''}</div>`;
}

export function cloneDestination(recovery, parent) {
  const url=recovery?.declaration?.source?.url||'';
  let name=url.split(/[/:]/).pop().replace(/\.git$/, '').replace(/[^a-zA-Z0-9._-]/g, '-');
  if(!name||name==='.'||name==='..')name='knowledge';
  return parent.replace(/\/+$/, '')+'/'+name;
}
