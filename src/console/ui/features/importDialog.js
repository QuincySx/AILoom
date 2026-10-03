// AIL-113：可复用的导入 Dialog —— 从资源库的导入表单抽取，
// 供资源库与项目内「＋导入」共用同一套导入业务（本地/Git/skills.sh）。
// 预览默认、确认才入库；取消零写入。成功回调 onImported(结果)，取消回调 onCancelled()。
import { api, esc } from '../services/api.js';
import { Dialog } from '../components/dialog.js';
import { notify } from '../state/store.js';

let nextImportId = 0;

export function ImportDialog(container, { title = '导入资源', onChanged, onImported, onCancelled } = {}) {
  const root = document.createElement('div');
  container.append(root);
  const formId = `inline-import-${++nextImportId}`;
  let alive = true, busy = false, candidate = null, imported = false, nativePicker = false;
  root.innerHTML = `
    <form data-form id="${formId}">
      <fieldset data-fields><div class="form-grid">
        <label>来源类型<select data-provider><option value="github">GitHub 仓库</option><option value="gitlab">GitLab / 自建 GitLab</option><option value="git">其他 Git 服务</option><option value="local">本地 Skill 文件夹</option><option value="entry">skills.sh</option></select></label>
        <label class="full"><span data-url-label>仓库地址</span><div class="input-action"><input data-url required placeholder="https://github.com/owner/skills.git"><button type="button" data-pick hidden>选择文件夹…</button></div></label>
        <details class="full"><summary>名称与版本选项（可选）</summary><div class="form-grid"><label data-name-label>显示名称<input data-name placeholder="例如：我的开发工具"></label><label data-ref-label>分支 / 标签<input data-ref placeholder="默认分支"></label></div></details>
      </div><p data-help class="muted">Skill 文件夹需包含 SKILL.md。</p>
      </fieldset>
    </form><p data-import-msg class="inline-status" role="status"></p><div data-candidate></div>
    <footer class="dialog-actions"><button type="button" data-close>取消</button><button type="submit" form="${formId}" class="primary" data-preview>预览内容</button><button type="button" class="primary" data-confirm hidden>确认导入</button></footer>`;
  const q = s => root.querySelector(s);
  const modal = Dialog(container, { title, content: root, open: false, keepMounted: true, canClose: () => !busy, onClose: () => { invalidate(); if (!imported) onCancelled?.(); } });
  function invalidate() { candidate = null; q('[data-candidate]').innerHTML = '';q('[data-preview]').hidden=false;q('[data-confirm]').hidden=true; }
  function message(text,error=false) {
    if(!alive)return;
    const msg=q('[data-import-msg]');
    msg.textContent=text;
    msg.classList.toggle('field-error',error);
    msg.setAttribute('role',error?'alert':'status');
  }
  async function run(fn) {
    if (busy) return;
    busy = true;
    q('[data-fields]').disabled = true;
    q('[data-preview]').disabled = q('[data-confirm]').disabled = q('[data-close]').disabled = true;
    try { await fn(); } catch (e) { message(e.message,true); }
    finally { busy = false; if (alive) { q('[data-fields]').disabled = false; q('[data-preview]').disabled = q('[data-confirm]').disabled = q('[data-close]').disabled = false; } }
  }
  const provider = () => q('[data-provider]').value;
  function providerChanged() {
    invalidate();
    const p = provider(), copy = p === 'local' || p === 'entry';
    // 本地目录名可能是中文等非法存储名：给 local 提供 ASCII 存储名称输入（AIL-113 实测 E3002）。
    q('[data-name-label]').hidden = p === 'entry';
    q('[data-name-label]').firstChild.textContent = p === 'local' ? '名称（可选，小写字母、数字或连字符）' : '显示名称';
    q('[data-name]').placeholder = p === 'local' ? '如 retrieval-tips；目录名已合法可留空' : '例如：我的开发工具';
    q('[data-ref-label]').hidden = copy;
    q('[data-url-label]').textContent = p === 'local' ? '本机 Skill 文件夹路径' : p === 'entry' ? 'skills.sh 链接' : 'Git 仓库克隆地址';
    q('[data-url]').value = '';
    q('[data-url]').placeholder = ({ github:'https://github.com/owner/skills.git', gitlab:'https://gitlab.com/group/skills.git', git:'https://git.example.com/team/skills.git', local:'/Users/me/my-skill', entry:'https://skills.sh/owner/repo/skill' })[p];
    q('[data-preview]').textContent = '预览内容';
    q('[data-help]').textContent = p === 'local' ? '选择包含 SKILL.md 的文件夹。' : '';
    // 原生文件夹选择器仅桌面端可用；无头环境与其他平台保持手填路径。
    q('[data-pick]').hidden = p !== 'local' || !nativePicker;
  }
  api.state().then(v => {
    nativePicker = !!v?.native_picker;
    q('[data-pick]').hidden = provider() !== 'local' || !nativePicker;
  }).catch(() => {});
  q('[data-pick]').onclick = async () => {
    const pick = q('[data-pick]');
    pick.disabled = true;
    try { const r = await api.pickDirectory(); if (alive && r.path) { q('[data-url]').value = r.path; invalidate(); } }
    catch (e) { message(e.message,true); }
    finally { if (alive) pick.disabled = false; }
  };
  q('[data-provider]').onchange = providerChanged;
  q('[data-form]').addEventListener('input', invalidate);
  q('[data-close]').onclick = () => modal.close();
  q('[data-form]').onsubmit = event => { event.preventDefault(); run(async () => {
    invalidate(); message('正在读取来源并检查资源…');
    const p = provider(), url = q('[data-url]').value.trim();
    if (!url) throw new Error('请填写来源地址。');
    if (p === 'github' && !/^(https?:\/\/github\.com\/|git@github\.com:|ssh:\/\/git@github\.com\/)/.test(url)) throw new Error('请填写 GitHub 仓库地址，其他域名请选择“其他 Git 服务”。');
    let v;
    if (p === 'local') { await api.approveDir(url); v = await api.libraryImport(url, q('[data-name]').value.trim() || undefined, false); }
    else if (p === 'entry') { v = await api.libraryImportEntry(url, undefined, false); v = v.result ?? v; }
    else {
      // 本机路径形式的 Git 源同样要先显式批准目录（与 local 导入同一边界）。
      if (url.startsWith('/') || url.startsWith('file://')) await api.approveDir(url);
      v = await api.collectionPreview({ name: q('[data-name]').value.trim() || url.split('/').pop().replace(/\.git$/, ''), url, ref: q('[data-ref]').value.trim() || undefined });
    }
    if (!alive) return;
    candidate = { provider:p, url, value:v };
    const resources = v.resources ?? [];
    const preview = v.preview ?? v;
    if (v.error || preview.error) throw new Error(v.error || preview.error);
    q('[data-candidate]').innerHTML = `<div class="resource-row"><h3>确认导入内容</h3>
      <p class="path">${esc(url)}</p><p>${resources.length ? resources.length + ' 项资源' : esc(preview.skill_name || 'Skill 副本')}</p>
      ${resources.length ? '<ul>' + resources.map(r => '<li>' + esc(({skill:'技能',mcp:'工具连接',rule:'规则',agent:'子代理'})[r.kind]||r.kind) + ' · ' + esc(r.name) + '</li>').join('') + '</ul>' : ''}
      </div>`;
    q('[data-preview]').hidden=true;q('[data-confirm]').hidden=false;
    q('[data-confirm]').onclick = () => run(async () => {
      const c = candidate; if (!c) return;
      let result;
      if (c.provider === 'local') result = await api.libraryImport(c.url, q('[data-name]').value.trim() || undefined, true);
      else if (c.provider === 'entry') result = await api.libraryImportEntry(c.url, undefined, true);
      else result = await api.collectionApply(c.value.preview_id);
      imported = true;
      invalidate(); modal.close();
      // 对话框已关闭，成功提示放到页面级 toast，否则用户看不到（AIL-140）。
      notify('已加入资源库，可在项目中添加使用。');
      onChanged?.();
      onImported?.(result);
    });
    message('');
  }); };
  return {
    show: () => { message(''); modal.show(); },
    destroy: () => { alive = false; modal.destroy(); root.remove(); },
  };
}
