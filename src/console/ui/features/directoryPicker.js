// AIL-122：目录选择 Dialog —— 浏览真实目录；只发选择事件，不保存配置。
// fsList 按需展开（不递归全盘）；搜索仅限已加载目录；「已添加」标注来自
// 真实配置记录（projectDirs），失联配置目录可查看但不可应用。

import { api, esc } from '../services/api.js';
import { Dialog } from '../components/dialog.js';

export function DirectoryPicker(container, { title, rootPath, rootLabel = '根目录', initialRelative = null, configuredDirs = [], onPicked }) {
  const body = document.createElement('div');
  body.innerHTML = `
    <input type="search" data-dir-search aria-label="搜索目录名称或相对路径" placeholder="搜索目录名称或相对路径（仅限已加载的目录）">
    <div data-dir-tree class="dir-tree" role="tree" aria-label="目录树"></div>
    <p data-dir-error class="field-error" role="alert"></p>
    <p class="muted" data-dir-selected></p>
    <footer class="dialog-actions"><button type="button" data-dir-cancel>取消</button><button type="button" class="primary" data-dir-use disabled>使用此目录</button></footer>`;
  const q = s => body.querySelector(s);
  const loaded = new Map();   // relDir -> [{name, dir}]
  const expanded = new Set(['']);
  let currentRelative = initialRelative ?? '';
  let alive = true;

  const modal = Dialog(container, { title, content: body, canClose: () => true, onClose: () => { alive = false; } });
  // Dialog 生成后立即关闭（open 未传 true 不会自动打开；这里手动控制时机）
  const configuredByParent = new Map();
  for (const d of configuredDirs) {
    const parent = d.path.includes('/') ? d.path.slice(0, d.path.lastIndexOf('/')) : '';
    if (!configuredByParent.has(parent)) configuredByParent.set(parent, []);
    configuredByParent.get(parent).push(d);
  }
  const configuredNote = rel => {
    const hit = configuredDirs.find(d => d.path === rel);
    if (!hit) return '';
    return hit.missing ? '已添加（目录失联）' : '已添加';
  };

  async function ensureLoaded(rel) {
    if (loaded.has(rel)) return loaded.get(rel);
    q('[data-dir-error]').textContent = '';
    try {
      const path = rel ? trimJoin(rootPath, rel) : rootPath;
      const v = await api.fsList(path);
      const dirs = (v.items || []).filter(i => i.dir).map(i => i.name);
      loaded.set(rel, dirs);
      return dirs;
    } catch (e) {
      q('[data-dir-error]').textContent = `读取目录失败：${e.message}`;
      loaded.set(rel, []);
      return [];
    }
  }

  function trimJoin(root, rel) { return root.replace(/\/+$/, '') + '/' + rel; }

  function filteredDirs(rel) {
    const dirs = loaded.get(rel) || [];
    const query = q('[data-dir-search]').value.trim().toLowerCase();
    if (!query) return dirs;
    // 搜索范围：已加载目录的相对路径（明确不递归全盘）
    const pool = [];
    for (const [parent, names] of loaded) {
      for (const n of names) pool.push(parent ? parent + '/' + n : n);
    }
    return [...new Set(pool)].filter(p => p.toLowerCase().includes(query)).sort();
  }

  async function render() {
    const query = q('[data-dir-search]').value.trim();
    const tree = q('[data-dir-tree]');
    if (query) {
      const hits = filteredDirs('');
      tree.innerHTML = hits.length ? `<div role="treeitem" aria-level="1">${hits.map(p => `
        <div class="dir-row" data-rel="${esc(p)}" role="treeitem" aria-selected="${p === currentRelative}" tabindex="0">
          <span class="dir-name">${esc(p.split('/').pop())}</span><span class="path dir-rel"><ailoom-path tabindex="-1" title="${esc(p)}">${esc(p)}</ailoom-path></span>
          ${configuredNote(p) ? `<span class="badge">${esc(configuredNote(p))}</span>` : ''}
        </div>`).join('')}</div>`
        : '<p class="muted">没有匹配的目录。可展开文件夹继续查找。</p>';
      bindRows();
      syncSelected();
      return;
    }
    const lines = [`<div class="dir-row" data-rel="" role="treeitem" aria-selected="${currentRelative === ''}" tabindex="0">
      <span class="dir-name">${esc(rootLabel)}</span>${configuredNote('') ? `<span class="badge">${esc(configuredNote(''))}</span>` : ''}
      <span class="muted dir-note">项目根目录</span></div>`];
    const renderLevel = async (rel, depth) => {
      if (!expanded.has(rel)) return;
      const dirs = await ensureLoaded(rel);
      for (const name of dirs) {
        const childRel = rel ? rel + '/' + name : name;
        const conf = configuredNote(childRel);
        lines.push(`<div class="dir-row" data-rel="${esc(childRel)}" role="treeitem" aria-selected="${childRel === currentRelative}" aria-level="${depth + 1}" tabindex="0" style="--dir-depth:${depth}">
          <span class="dir-twist" data-twist="${esc(childRel)}" role="button" aria-label="展开 ${esc(name)}" aria-expanded="${expanded.has(childRel)}">${expanded.has(childRel) ? '⌄' : '›'}</span>
          <span class="dir-name">${esc(name)}</span>
          ${conf ? `<span class="badge">${esc(conf)}</span>` : ''}
        </div>`);
        await renderLevel(childRel, depth + 1);
      }
    };
    await renderLevel('', 1);
    tree.innerHTML = lines.join('');
    bindRows();
    syncSelected();
  }

  function bindRows() {
    q('[data-dir-tree]').querySelectorAll('.dir-row').forEach(rowEl => {
      rowEl.onclick = event => {
        const rel = rowEl.dataset.rel;
        const twist = event.target.closest('[data-twist]');
        if (twist) {
          const childRel = twist.dataset.twist;
          if (expanded.has(childRel)) expanded.delete(childRel); else expanded.add(childRel);
          render();
          return;
        }
        currentRelative = rel;
        syncSelected();
        render();
      };
      rowEl.onkeydown = event => {
        if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); rowEl.click(); }
      };
    });
  }

  function syncSelected() {
    q('[data-dir-selected]').textContent = `选中：${currentRelative === '' ? (rootLabel + '（' + rootPath + '）') : rootPath + '/' + currentRelative}`;
    q('[data-dir-use]').disabled = false;
  }

  q('[data-dir-search]').oninput = render;
  q('[data-dir-cancel]').onclick = () => modal.close();
  q('[data-dir-use]').onclick = () => {
    const rel = currentRelative === '' ? null : currentRelative;
    modal.close();
    onPicked?.(rel);
  };

  render();
  return {
    dialog: modal,
    show: () => modal.show(),
    destroy: () => { alive = false; modal.destroy(); },
  };
}
