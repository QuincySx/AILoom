// 2026-09-21 UX 成熟度第 1 轮：旧六步向导（批准目录/Worktree/逐条保存）与「新建项目」
// 主流程两套概念并存，且内部术语直出。改为单一三步引导页：讲清链路 + 直达入口，
// 不再承载配置动作本身（登记走项目列表、入库走资源库、生效走项目页）。

import { api } from '../services/api.js';
import { Button } from '../components/button.js';

export function mount(container, ctx) {
  const root = document.createElement('div');
  container.appendChild(root);

  async function render() {
    let repos = [];
    try { repos = (await api.state()).repos || []; } catch { /* 引导页读不到状态也照常展示 */ }
    const hasRepos = repos.length > 0;

    root.innerHTML = `
      <header class="page-head"><div>
        <h1>开始使用</h1>
        <p>为项目选择 AI 能力。</p>
      </div></header>
      ${hasRepos ? `<p class="muted">已添加 ${repos.length} 个项目。</p>` : ''}
      <div class="wizard">
        <section class="step ${hasRepos ? '' : 'active'}">
          <h2>① 添加项目</h2>
          <p class="muted">选择本地文件夹或 Git 仓库。</p>
          <div data-go="#/projects/manage" data-primary="${hasRepos ? '' : '1'}"></div>
        </section>
        <section class="step">
          <h2>② 导入能力</h2>
          <p class="muted">支持本地文件夹、Git 仓库和 skills.sh。</p>
          <div data-go="#/library"></div>
        </section>
        <section class="step ${hasRepos ? 'active' : ''}">
          <h2>③ 添加并应用</h2>
          <p class="muted">选择项目和 AI 工具，添加能力，查看并应用改动，然后新开 AI 会话。</p>
          <div data-go="#/projects"></div>
        </section>
      </div>
`;

    root.querySelectorAll('[data-go]').forEach((slot) => {
      Button(slot, {
        label: slot.dataset.go === '#/projects/manage' ? (slot.dataset.primary ? '添加项目' : '管理项目')
          : slot.dataset.go === '#/library' ? '打开资源库' : '打开我的目录',
        variant: slot.dataset.primary ? 'default' : 'outline',
        onPress: () => { location.hash = slot.dataset.go; },
      });
    });
  }

  render();
  return { destroy() { root.remove(); } };
}
