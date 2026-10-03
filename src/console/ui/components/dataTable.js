// AIL-083：DataTable —— rows/rowKey/columns，空/加载/失败态，稳定键渲染。
// columns: [{ key, label, render?(row) }]

import { esc } from '../services/api.js';

export function DataTable(container, props) {
  const wrap = document.createElement('div');
  container.appendChild(wrap);
  let cur = props;

  function render(p) {
    cur = p;
    if (p.loading && !Array.isArray(p.rows)) {
      wrap.innerHTML = '<p class="muted">加载中…</p>';
      return;
    }
    if (p.error) {
      wrap.innerHTML = `<p class="badge bad">${esc(p.error)}</p>`;
      return;
    }
    const rows = p.rows ?? [];
    if (!rows.length) {
      wrap.innerHTML = `<p class="muted">${esc(p.empty ?? '（空）')}</p>`;
      return;
    }
    const cols = p.columns ?? [];
    const thead = cols.map((c) => `<th scope="col">${esc(c.label)}</th>`).join('');
    const tbody = rows
      .map((row, index) => {
        const key = esc(p.rowKey ? p.rowKey(row) : JSON.stringify(row));
        const tds = cols
          .map((c, column) => {
            const content = c.render ? c.render(row) : esc(row[c.key]);
            return `<td data-label="${esc(c.label)}">${p.onSelect && column === 0 ? `<button type="button" class="table-row-action">${content}</button>` : content}</td>`;
          })
          .join('');
        return `<tr data-key="${key}" data-index="${index}">${tds}</tr>`;
      })
      .join('');
    wrap.innerHTML = `<table><thead><tr>${thead}</tr></thead><tbody>${tbody}</tbody></table>`;
    if (p.onSelect) {
      wrap.querySelectorAll('tbody tr').forEach((tr) => {
        tr.onclick = () => {
          const row = rows[Number(tr.dataset.index)];
          if (row) {
            wrap.querySelectorAll('tbody tr').forEach(r=>r.classList.toggle('selected',r===tr));
            p.onSelect(row);
          }
        };
      });
    }
  }
  render(props);
  return {
    update(next) { render({ ...cur, ...next }); },
    destroy() { wrap.remove(); },
  };
}
