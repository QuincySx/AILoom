// AIL-093：六步首次设置向导 —— 选择目录→确认归属→选宿主/资源→预览→应用→验证。
// 只有当前步骤展开编辑，已完成步骤为摘要可返回；第3步保存不 apply；
// 第4步无变更不建空任务；第5步只执行当前绑定的有效计划；第6步未真实调用不标全部可用。

import { api, esc } from '../services/api.js';
import { Button } from '../components/button.js';
import { Field } from '../components/field.js';
import { StatusBadge } from '../components/badge.js';
import { draft, patchDraft, saveDraft } from '../state/draft.js';
import { setTarget, currentTarget, currentGeneration, shouldApply } from '../state/target.js';
import { notify } from '../state/store.js';
import { PlanPreview, JobPanel, VerificationPanel } from '../features/planPreview.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);
  let d = draft();
  let destroyers = [];
  let stepEl = null;

  const STEPS = ['目录', '仓库', '能力', '预览', '应用', '验证'];

  async function render() {
    d = draft();
    destroyers.forEach((fn) => fn());
    destroyers = [];
    root.innerHTML = '';
    const tabs = document.createElement('div');
    tabs.className = 'nav';
    STEPS.forEach((nm, i) => {
      const b = document.createElement('button');
      b.textContent = `${i + 1} ${nm}`;
      if (d.step === i + 1) b.className = 'on';
      b.onclick = () => { patchDraft({ step: i + 1 }); saveDraft().then(render); };
      tabs.appendChild(b);
    });
    root.appendChild(tabs);

    const mkStep = (title, done) => {
      const el = document.createElement('div');
      el.className = 'step' + (d.step === STEPS.indexOf(title) + 1 ? ' active' : '');
      const h = document.createElement('h2');
      h.textContent = (done ? '✅ ' : '') + title;
      el.appendChild(h);
      root.appendChild(el);
      return el;
    };

    renderStep1(mkStep('选择工作目录', !!d.approvedRoot));
    renderStep2(mkStep('确认仓库与工作树', !!d.repo));
    renderStep3(mkStep('选择宿主与能力', d.capSaved));
    renderSteps456();
  }

  // ---- Step 1 目录 ----
  function renderStep1(el) {
    const p = document.createElement('p');
    p.className = 'muted';
    p.textContent = '浏览器无权读取本机任意目录：需要你明确提供路径并批准（服务只在批准根内读文件）。';
    const field = Field(el, { label: '工作目录', width: '85%', value: d.approvedRoot ?? '' });
    Button(el, {
      label: '批准此目录',
      onPress: async () => {
        const path = field.value().trim();
        try {
          await api.approveDir(path);
          if (d.approvedRoot !== path) {
            // U02：目录变化 → 下游失效
            patchDraft({ repo: null, planJob: null, planView: null, applyJob: null, applyView: null, capSaved: false, capEffective: null });
            setTarget(null);
          }
          patchDraft({ approvedRoot: path, step: Math.max(d.step, 2) });
          await saveDraft();
          render();
        } catch (e) {
          notify('批准失败：' + e.message);
        }
      },
    });
    el.appendChild(p);
  }

  // ---- Step 2 归属 ----
  function renderStep2(el) {
    const btnSlot = document.createElement('p');
    el.appendChild(btnSlot);
    Button(btnSlot, {
      label: '识别当前目录',
      disabled: !d.approvedRoot,
      onPress: async () => {
        try {
          const v = await api.repoDiscover(d.approvedRoot);
          patchDraft({ repo: v, step: 3 });
          if (v.kind === 'nongit') setTarget({ repo_id: v.repo_id, wt_id: 'root', path: v.root, kind: 'nongit' });
          else setTarget({ repo_id: v.repo_id, wt_id: null, path: v.current_worktree, kind: 'git' });
          await saveDraft();
          render();
        } catch (e) {
          notify('识别失败：' + e.message + '（Git 探测错误不会伪装成非 Git）');
        }
      },
    });
    const view = document.createElement('div');
    el.appendChild(view);
    if (d.repo) {
      const r = d.repo;
      if (r.kind === 'nongit') {
        view.innerHTML = `<p class="badge warn">非 Git 目录（路径模式 ${esc(r.repo_id ?? '')}）：${esc(r.root)}</p>`;
      } else {
        const wts = (r.worktrees ?? []).map((w) => {
          const b = document.createElement('span');
          const td = document.createElement('td');
          const badge = document.createElement('span');
          badge.className = 'badge' + (w.is_bare ? ' warn' : ' ok');
          badge.textContent = w.is_bare ? 'bare' : 'active';
          td.appendChild(badge);
          if (w.is_detached) { const bd = document.createElement('span'); bd.className = 'badge warn'; bd.textContent = 'detached'; td.appendChild(bd); }
          b.appendChild(td);
          return `<tr><td>${esc(w.path)}</td><td>${td.innerHTML}</td><td>${esc(w.branch ?? '')}</td>
            <td><button data-pick="${esc(w.path)}">设为目标</button></td></tr>`;
        }).join('');
        view.innerHTML = `<p>仓库 <b>${esc(r.repo_id)}</b> 根：${esc(r.repo_root)}
          ${r.origin ? `<span class="muted">origin ${esc(r.origin)}</span>` : '<span class="badge">无远端（仍用 Git 身份）</span>'}</p>
          <table><tr><th>工作树</th><th>状态</th><th>分支</th><th></th></tr>${wts}</table>`;
        view.querySelectorAll('[data-pick]').forEach((b) => {
          b.onclick = () => {
            setTarget({ repo_id: r.repo_id, wt_id: null, path: b.dataset.pick, kind: 'git' });
            notify('操作目标：' + b.dataset.pick);
          };
        });
      }
    }
  }

  // ---- Step 3 能力 ----
  function renderStep3(el) {
    const detect = document.createElement('p');
    el.appendChild(detect);
    const hostInfo = document.createElement('span');
    hostInfo.className = 'muted';
    hostInfo.textContent = d.hostInfo
      ? `Claude: ${d.hostInfo.claude?.installed ? d.hostInfo.claude.version : '未安装'}；Codex: ${d.hostInfo.codex?.installed ? d.hostInfo.codex.version : '未安装'}`
      : '';
    Button(detect, {
      label: '探测本机宿主（只读 --version）',
      onPress: async () => {
        const v = await api.detectHosts();
        patchDraft({ hostInfo: v, hosts: { claude: !!v.claude?.installed, codex: !!v.codex?.installed } });
        await saveDraft();
        render();
      },
    });
    detect.appendChild(hostInfo);

    const checks = document.createElement('p');
    checks.innerHTML = `
      <label><input type="checkbox" data-h="claude" ${d.hosts.claude ? 'checked' : ''}> Claude Code</label>
      <label><input type="checkbox" data-h="codex" ${d.hosts.codex ? 'checked' : ''}> Codex CLI</label>
      <span class="muted">取消勾选 = 明确禁用（保存时下发 disable）</span>`;
    el.appendChild(checks);

    const capSlot = document.createElement('p');
    el.appendChild(capSlot);
    const msg = document.createElement('span');
    msg.className = 'muted';
    capSlot.appendChild(msg);
    Button(capSlot, {
      label: '保存能力选择（仅保存，不写仓库）',
      disabled: !currentTarget()?.path,
      onPress: async () => {
        const hosts = {
          claude: checks.querySelector('[data-h="claude"]').checked,
          codex: checks.querySelector('[data-h="codex"]').checked,
        };
        patchDraft({ hosts });
        const target = currentTarget();
        for (const h of ['claude', 'codex']) {
          // U01：勾选/取消都提交显式三态
          await api.select({ host: h, state: hosts[h] ? 'enable' : 'disable', root: target.path });
        }
        const eff = await api.effective(target.path);
        patchDraft({ capEffective: eff.hosts ?? null, capSaved: true });
        await saveDraft();
        msg.textContent = '已保存（仓外）。部署 = 下一步预览+应用；生效状态见下表。';
        renderEffTable(el, eff.hosts ?? {});
      },
    });
    if (d.capEffective) renderEffTable(el, d.capEffective);

    const importHint = document.createElement('p');
    importHint.className = 'muted';
    importHint.textContent = '导入 skill 请到「资源库」页（支持本地目录 / GitHub / skills.sh 入口），导入后回到本页保存能力。';
    el.appendChild(importHint);
  }

  function renderEffTable(el, hosts) {
    let t = el.querySelector('[data-eff]');
    if (!t) {
      t = document.createElement('div');
      t.setAttribute('data-eff', '');
      el.appendChild(t);
    }
    const rows = Object.entries(hosts).map(([k, v]) =>
      `<tr><td>${esc(k)}</td><td>${v.enabled ? '<span class="badge ok">启用</span>' : '<span class="badge">停用</span>'}</td></tr>`).join('');
    t.innerHTML = `<p class="muted">生效状态（服务端回显，保存 ≠ 部署）：</p><table><tr><th>宿主</th><th>有效值</th></tr>${rows}</table>`;
  }

  // ---- Steps 4-6：计划/应用/验证（业务组件） ----
  function renderSteps456() {
    const accept = (gen) => shouldApply(gen);
    const p4 = document.createElement('div');
    root.appendChild(p4);
    destroyers.push(() => { p4.remove(); });
    PlanPreview(p4, {
      target: currentTarget(),
      targetGen: currentGeneration(),
      accept,
      view: d.planView,
      onPlanned: (jobId, result, gen) => {
        if (!shouldApply(gen)) return;
        patchDraft({ planJob: jobId, planView: result.summary ?? '', step: 5 });
        saveDraft().then(render);
      },
    });
    const p5 = document.createElement('div');
    root.appendChild(p5);
    JobPanel(p5, {
      planJob: d.planJob,
      onApplied: (applyJobId, done) => {
        patchDraft({ applyJob: applyJobId, applyView: '已应用', step: 6 });
        window._lastApply = done;
        saveDraft().then(render);
      },
    });
    const p6 = document.createElement('div');
    root.appendChild(p6);
    VerificationPanel(p6, {
      applyJob: d.applyJob,
      appliedCount: window._lastApply?.result?.applied?.length ?? 0,
      verification: window._lastApply?.result?.verification,
    });
  }

  render();
  return {
    destroy() {
      destroyers.forEach((fn) => fn());
      root.remove();
    },
  };
}
