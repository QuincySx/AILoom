// AIL-092：PlanPreview / JobPanel / VerificationPanel —— 计划、任务进度与验证结果。
// 无变化禁执行；任务轮询可取消；撤销冲突显式呈现。

import { api, esc } from '../services/api.js';
import { Button } from '../components/button.js';
import { StatusBadge } from '../components/badge.js';
import { waitJob } from '../state/jobs.js';
import { notify } from '../state/store.js';

export function PlanPreview(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  wrap.innerHTML = `
    <h2>预览将要做的改动</h2>
    <p><span data-btn></span> <span class="muted">默认只影响当前工作树；公司已跟踪文件不会被写入；变更目标/版本后旧计划失效。</span></p>
    <div data-view>${props.view ?? ''}</div>`;
  const view = wrap.querySelector('[data-view]');
  const btnSlot = wrap.querySelector('[data-btn]');
  let cur = props;

  const btn = Button(btnSlot, {
    label: '生成预览',
    variant: 'default',
    disabled: !cur.target?.path,
    onPress: async () => {
      const target = cur.target;
      if (!target?.path) { notify('先选择操作目标'); return; }
      const gen = cur.targetGen;
      try {
        const j = await api.plan(target.path, cur.scope);
        const done = await waitJob(j.job_id, { onProgress: (m) => { view.textContent = '预览：' + (m ?? '') + '…'; } });
        if (!cur.accept(gen)) return; // 期间切了目标：丢弃过期结果
        if (done.status !== 'success') {
          view.innerHTML = `<span class="badge bad">预览失败：${esc(done.error ?? '')}</span>`;
          return;
        }
        const r = done.result;
        const skipped = (r.skipped_company_files ?? [])
          .map((s) => `<tr><td>${esc(s.path)}</td><td>${esc(s.reason)}</td></tr>`).join('');
        view.innerHTML = `
          <p class="muted">仓库 ${esc(r.repo_id)} · 工作树 ${esc(r.worktree_id)} · 作用域 ${esc(r.active_rel ?? '(工作树根)')}</p>
          <pre class="log">${esc(r.summary || '(无改动)')}</pre>
          ${(r.notes ?? []).length ? `<p class="muted">${esc(r.notes.join('；'))}</p>` : ''}
          ${skipped ? `<p>公司文件保护（已跳过）：</p><table><tr><th>路径</th><th>原因</th></tr>${skipped}</table>` : ''}
          <p class="muted">${(r.actions ?? []).filter((a) => a.action !== 'noop').length ? '' : '无改动：不触发应用任务。'}</p>`;
        cur.onPlanned?.(j.job_id, r, gen);
      } catch (e) {
        view.innerHTML = `<span class="badge bad">${esc(e.message)}</span>`;
      }
    },
  });

  function render(p) { cur = p; btn.update({ disabled: !p.target?.path }); }
  render(props);
  return { update(next) { render({ ...cur, ...next }); }, destroy() { wrap.remove(); } };
}

export function JobPanel(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  wrap.innerHTML = `
    <h2>应用</h2>
    <p><span data-btn></span> <span class="muted">只执行当前绑定的有效计划；部分失败可恢复。</span></p>
    <div data-view>${props.view ?? ''}</div>`;
  const view = wrap.querySelector('[data-view]');
  const btnSlot = wrap.querySelector('[data-btn]');
  let cur = props;

  const btn = Button(btnSlot, {
    label: '应用',
    variant: 'default',
    disabled: !cur.planJob,
    onPress: async () => {
      try {
        const j = await api.apply(cur.planJob);
        const done = await waitJob(j.job_id, { onProgress: (m) => { view.textContent = '应用：' + (m ?? '') + '…'; } });
        if (done.status !== 'success') {
          view.innerHTML = `<span class="badge bad">应用失败：${esc(done.error ?? '')}</span>`;
          return;
        }
        if (done.result?.ok === false || (done.result?.skipped_conflicts ?? []).length) {
          view.innerHTML = `<p class="badge warn">未全部应用成功。冲突内容已保留，请先处理后重新预览。</p><pre class="log">${esc(JSON.stringify(done.result.failed || done.result.skipped_conflicts, null, 2))}</pre>`;
          return;
        }
        cur.onApplied?.(j.job_id, done);
      } catch (e) {
        if (e.kind === 'conflict' || e.kind === 'validation') {
          view.innerHTML = `<span class="badge bad">${esc(e.message)}</span>`;
        } else {
          view.innerHTML = `<span class="badge bad">${esc(e.message)}</span>`;
        }
      }
    },
  });

  function render(p) { cur = p; btn.update({ disabled: !p.planJob }); }
  render(props);
  return { update(next) { render({ ...cur, ...next }); }, destroy() { wrap.remove(); } };
}

export function VerificationPanel(container, props) {
  const wrap = document.createElement('div');
  wrap.className = 'step';
  container.appendChild(wrap);
  wrap.innerHTML = `
    <h2>在宿主里真实验证</h2>
    <div data-view>${props.view ?? '<p class="muted">应用后这里给出验证动作</p>'}</div>
    <p><span data-undo></span> <span class="muted">只回滚本次工具写入的文件；你事后修改过的项会冲突保留；已 tracked 的新建项撤销删除被拒。</span></p>`;
  const view = wrap.querySelector('[data-view]');
  const undoSlot = wrap.querySelector('[data-undo]');
  let cur = props;

  const undoBtn = Button(undoSlot, {
    label: '撤销本次改动',
    disabled: !cur.applyJob,
    onPress: async () => {
      try {
        const v = await api.undo(cur.applyJob);
        const conf = v.conflicts ?? [];
        notify(conf.length
          ? `撤销：恢复 ${v.restored.length} 项；冲突保留 ${conf.length} 项（不覆盖你的修改）`
          : `撤销完成：恢复 ${v.restored.length} 项`);
        view.innerHTML += conf.length ? `<p class="badge warn">冲突项：${esc(conf.join('; '))}</p>` : '';
      } catch (e) {
        notify('撤销失败：' + e.message);
      }
    },
  });

  function render(p) {
    cur = p;
    undoBtn.update({ disabled: !p.applyJob });
    if (p.verification) {
      const items = (p.verification.items ?? []).map((i) => `<tr>
        <td>${esc(i.tool)}</td><td>${esc(i.path)}</td>
        <td><span data-badge="${esc(i.host_state)}"></span></td>
        <td class="muted">${esc(i.note)}</td></tr>`).join('');
      view.innerHTML = `
        <p class="badge ok">已写入 ${(p.appliedCount ?? 0)} 项</p>
        <table><tr><th>宿主</th><th>目标</th><th>状态</th><th>说明</th></tr>${items}</table>
        <p>下一步：在选定宿主<b>新开会话</b>调用刚启用的技能；回复与技能预期一致即验证通过。
        文件已写入不代表宿主已加载；请以实际调用结果确认。</p>`;
      view.querySelectorAll('[data-badge]').forEach((el) => {
        StatusBadge(el, { domain: 'host', status: el.dataset.badge });
      });
    }
  }
  render(props);
  return { update(next) { render({ ...cur, ...next }); }, destroy() { wrap.remove(); } };
}
