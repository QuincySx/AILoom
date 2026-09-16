//! 控制台前端（AIL-047/048/049）：单页应用，无构建步骤。
//! 六步 onboarding 向导（选目录 → 确认仓库/工作树 → 选宿主与能力 → 预览 → 应用
//! → 验证），草稿经 /api/draft 服务端保存（重连恢复输入，不自动重放执行动作）。
//! 界面明确区分「已保存 / 已部署 / 宿主已加载 / 需新会话 / 不支持」。

pub fn index_html(token: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="zh"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>AILoom 本地控制台</title>
<style>
body{{font-family:system-ui;max-width:1080px;margin:1.5rem auto;padding:0 1rem;color:#1a1a2e;background:#f7f7fb}}
h1{{font-size:1.3rem}} h2{{font-size:1.05rem;margin:.4rem 0}}
.badge{{display:inline-block;padding:1px 8px;border-radius:10px;font-size:.75rem;margin:0 2px;background:#e5e7f0}}
.ok{{background:#d1f2d8}}.warn{{background:#fdeeca}}.bad{{background:#f8d3d3}}
.step{{border:1px solid #d8d8e4;border-radius:10px;padding:12px 16px;margin:10px 0;background:#fff}}
.step.active{{border-color:#5b6ee1;box-shadow:0 0 0 2px #5b6ee122}}
button{{border:1px solid #5b6ee1;background:#eef0ff;color:#2c3a9e;border-radius:8px;padding:5px 14px;cursor:pointer;margin:2px}}
button:hover{{background:#dfe3ff}} button:disabled{{opacity:.4;cursor:default}}
input,textarea,select{{border:1px solid #c9c9d8;border-radius:6px;padding:5px 8px;font:inherit;width:70%}}
textarea{{width:100%;min-height:130px;font-family:ui-monospace,monospace}}
table{{border-collapse:collapse;width:100%;font-size:.85rem;margin:6px 0}}
td,th{{border:1px solid #e2e2ec;padding:4px 8px;text-align:left}}
.log{{background:#11131f;color:#c9d1e8;border-radius:8px;padding:10px;font-family:ui-monospace,monospace;font-size:.78rem;white-space:pre-wrap;max-height:280px;overflow:auto}}
.muted{{color:#6b6b80;font-size:.82rem}}
.hidden{{display:none}}
.nav{{display:flex;gap:6px;margin:10px 0}}
.nav button.on{{background:#5b6ee1;color:#fff}}
</style></head><body>
<h1>AILoom 本地控制台 <span class="muted">仅本机可访问</span></h1>
<div class="nav">
<button data-page="onboarding" class="on">首次设置</button>
<button data-page="scopes">仓库与作用域</button>
<button data-page="library">资源库与流程</button>
</div>
<div id="app"></div>
<script>
const TOKEN = {token:?};
const H = {{ 'X-AILoom-Session': TOKEN, 'Content-Type': 'application/json' }};
const $ = sel => document.querySelector(sel);
const esc = s => {{ const d = document.createElement('div'); d.textContent = String(s ?? ''); return d.innerHTML; }};
async function api(verb, path, body) {{
  const r = await fetch(path, {{ method: verb, headers: H, body: body ? JSON.stringify(body) : undefined }});
  const v = await r.json().catch(() => ({{}}));
  if (!r.ok) throw Object.assign(new Error(v.error || r.statusText), {{ status: r.status, data: v }});
  return v;
}}
let draft = {{ step: 1, approvedRoot: null, repo: null, hosts: {{}}, skills: {{}}, planJob: null, applyJob: null }};
let draftRev = 0;
async function saveDraft() {{ try {{ const r = await api('PUT', '/api/draft', {{ base_revision: draftRev, draft }}); draftRev = r.revision; }} catch (e) {{ if (e.status === 409) {{ draftRev = e.data.current_revision; }} }} }}

// ---------- 页面切换 ----------
let page = 'onboarding';
document.querySelectorAll('.nav button').forEach(b => b.onclick = () => {{
  page = b.dataset.page;
  document.querySelectorAll('.nav button').forEach(x => x.classList.toggle('on', x === b));
  render();
}});

function stateBadge(s) {{
  const map = {{ 'active': 'ok', 'missing': 'bad', 'bare': 'warn', 'detached': 'warn' }};
  return `<span class="badge ${{map[s]||''}}">${{esc(s)}}</span>`;
}}
function hostStateBadge(s) {{
  const map = {{ 'needs-new-session': ['warn','需新会话'], 'needs-approval': ['warn','需宿主批准'],
    'host-unverified': ['bad','宿主未验证'], 'deployed': ['ok','已部署'], 'missing': ['bad','缺失'] }};
  const [cls, label] = map[s] || ['warn', s];
  return `<span class="badge ${{cls}}">${{esc(label)}}</span>`;
}}

// ---------- Onboarding 向导 ----------
function render() {{
  if (page === 'onboarding') renderOnboarding();
  if (page === 'scopes') renderScopes();
  if (page === 'library') renderLibrary();
}}
function stepBox(n, title, inner, done) {{
  return `<div class="step ${{draft.step===n?'active':''}}"><h2>${{done?'✅':n}} ${{title}}</h2>${{inner}}</div>`;
}}
async function renderOnboarding() {{
  let serverInfo = {{}};
  try {{ serverInfo = await api('GET', '/api/server-info'); }} catch (e) {{}}
  const steps = [];
  // Step 1：目录
  steps.push(stepBox(1, '选择工作目录', `
    <p class="muted">浏览器无权读取本机任意目录：需要你明确提供路径并批准（服务只在批准的根内读文件）。</p>
    <input id="dirInput" value="${{esc(draft.approvedRoot || serverInfo.cwd || '')}}" style="width:85%">
    <button onclick="approveDir()">批准此目录</button> <span id="dirMsg" class="muted"></span>`,
    !!draft.approvedRoot));
  // Step 2：仓库归属
  steps.push(stepBox(2, '确认仓库与工作树', `
    <button onclick="discoverRepo()" ${{draft.approvedRoot?'':'disabled'}}>识别当前目录</button>
    <div id="repoView">${{draft.repo ? repoHtml(draft.repo) : ''}}</div>`,
    !!draft.repo));
  // Step 3：宿主与能力
  steps.push(stepBox(3, '选择宿主与能力', `
    <div>已安装宿主：<button onclick="detectHosts()">探测本机宿主（只读 --version）</button>
    <span id="hostInfo">${{draft.hostInfo ? esc(JSON.stringify(draft.hostInfo)) : ''}}</span></div>
    <div>宿主启用：
      <label><input type="checkbox" id="h-claude" ${{draft.hosts.claude?'checked':''}}> Claude Code</label>
      <label><input type="checkbox" id="h-codex" ${{draft.hosts.codex?'checked':''}}> Codex CLI</label>
    </div>
    <div style="margin-top:6px">导入本地 skill 目录：
      <input id="skillDir" placeholder="例如 /Users/me/my-skills/my-skill" style="width:55%">
      <button onclick="importSkill(0)" ${{draft.approvedRoot?'':'disabled'}}>预览</button>
      <button onclick="importSkill(1)" ${{draft.approvedRoot?'':'disabled'}}>导入</button>
      <span class="muted">预览不复制不执行；脚本仅复制、绝不运行</span>
      <div id="importView"></div>
    </div>
    <div id="libView">${{draft.libSummary ? draft.libSummary : ''}}</div>
    <button onclick="saveCapabilities()">保存能力选择（仅保存，不写仓库）</button>
    <span id="capMsg" class="muted">已保存 ≠ 已部署</span>`,
    !!draft.capSaved));
  // Step 4：预览
  steps.push(stepBox(4, '预览将要做的改动', `
    <button onclick="runPlan()" ${{draft.capSaved?'':'disabled'}}>生成预览</button>
    <span class="muted">默认只影响当前工作树；公司已跟踪文件不会被写入</span>
    <div id="planView">${{draft.planView || ''}}</div>`,
    !!draft.planJob));
  // Step 5：应用
  steps.push(stepBox(5, '应用到当前工作树', `
    <button onclick="runApply()" ${{draft.planJob?'':'disabled'}}>应用</button>
    <div id="applyView">${{draft.applyView || ''}}</div>`,
    !!draft.applyJob));
  // Step 6：验证
  steps.push(stepBox(6, '在宿主里真实验证', `
    <div id="verifyView">${{draft.applyView ? draft.applyView : '<p class="muted">应用后这里给出验证动作</p>'}}</div>
    <button onclick="undoApply()" ${{draft.applyJob?'':'disabled'}}>撤销本次改动</button>
    <span class="muted">只回滚本次工具写入的文件；公司文件从不在写入范围内</span>`,
    false));
  $('#app').innerHTML = steps.join('');
}}

window.approveDir = async function() {{
  const path = $('#dirInput').value.trim();
  try {{
    await api('POST', '/api/fs/approve', {{ path }});
    draft.approvedRoot = path; await saveDraft(); render();
  }} catch (e) {{ $('#dirMsg').textContent = '失败：' + e.message; }}
}};
window.discoverRepo = async function() {{
  try {{
    const v = await api('POST', '/api/repo/discover', {{ path: draft.approvedRoot }});
    draft.repo = v; await saveDraft(); render();
  }} catch (e) {{
    $('#repoView').innerHTML = `<p class="badge bad">非 Git 或识别失败：${{esc(e.message)}}</p><p class="muted">非 Git 文件夹将以路径模式记录（部分能力受限）；Git 探测错误不会伪装成非 Git。</p>`;
  }}
}};
function repoHtml(r) {{
  if (r.kind === 'nongit') return `<p class="badge warn">非 Git 目录：${{esc(r.root)}}</p>`;
  const wts = (r.worktrees||[]).map(w => `<tr><td>${{esc(w.path)}}</td><td>${{w.is_bare?stateBadge('bare'):stateBadge('active')}}${{w.is_detached?stateBadge('detached'):''}}${{w.locked_reason?stateBadge('locked：'+w.locked_reason):''}}</td><td>${{esc(w.branch||'')}}</td></tr>`).join('');
  return `<p>仓库 <b>${{esc(r.repo_id)}}</b> 根：${{esc(r.repo_root)}} ${{r.origin?'<span class="muted">origin '+esc(r.origin)+'</span>':'<span class="badge">无远端（仍用 Git 身份）</span>'}}</p>
  <table><tr><th>工作树</th><th>状态</th><th>分支</th></tr>${{wts}}</table>`;
}}
window.detectHosts = async function() {{
  const v = await api('POST', '/api/hosts/detect', {{}});
  draft.hostInfo = v; await saveDraft();
  $('#hostInfo').textContent = `Claude: ${{v.claude.installed ? v.claude.version : '未安装'}}；Codex: ${{v.codex.installed ? v.codex.version : '未安装'}}`;
}};
window.importSkill = async function(execute) {{
  try {{
    const v = await api('POST', '/api/library/import', {{ dir: $('#skillDir').value.trim(), execute: !!execute }});
    if (!execute) {{
      const p = v.preview;
      $('#importView').innerHTML = `<p>技能名 <b>${{esc(p.skill_name)}}</b>（${{p.files.length}} 个文件）脚本：${{esc(p.scripts.join(', ')||'无')}} 冲突：${{esc(p.conflicts.join('; ')||'无')}} 将补元数据：${{esc(p.metadata_to_add.join(', ')||'无')}}</p>`;
    }} else {{
      $('#importView').innerHTML = `<p class="badge ok">已导入：${{esc(v.report.skill_id)}}（脚本未执行）</p>`;
      // 导入即按用户意图在仓库默认层启用该技能（onboarding 第 3 步的能力选择）
      await api('POST', '/api/profile/select', {{ resource: v.report.skill_id, state: 'enable' }});
      await refreshLib();
    }}
  }} catch (e) {{
    $('#importView').innerHTML = `<span class="badge bad">${{esc(e.message)}}</span>`;
  }}
}};
async function refreshLib() {{
  try {{
    const v = await api('GET', '/api/library/list');
    draft.libSummary = (v.entries||[]).map(e => `<span class="badge">${{esc(e.id)}}</span>`).join(' ') || '<span class="muted">个人库为空</span>';
    await saveDraft();
  }} catch (e) {{}}
}}
window.saveCapabilities = async function() {{
  draft.hosts = {{ claude: $('#h-claude').checked, codex: $('#h-codex').checked }};
  for (const h of ['claude','codex']) {{
    if (draft.hosts[h]) await api('POST', '/api/profile/select', {{ host: h, state: 'enable' }});
  }}
  draft.capSaved = true; await saveDraft();
  $('#capMsg').textContent = '已保存到个人配置（仓外）。部署 = 下一步预览+应用。';
  render();
}};
async function waitJob(id, viewSel, label) {{
  for (let i = 0; i < 300; i++) {{
    const j = await api('GET', '/api/jobs/' + id);
    if (j.status !== 'queued' && j.status !== 'running') return j;
    if (i % 4 === 0) $(viewSel).textContent = label + '：' + (j.progress||[]).slice(-1)[0] + '…';
    await new Promise(r => setTimeout(r, 250));
  }}
  throw new Error('任务超时');
}}
window.runPlan = async function() {{
  try {{
    const j = await api('POST', '/api/jobs/plan', {{ root: draft.repo.repo_root }});
    draft.planJob = j.job_id;
    const done = await waitJob(j.job_id, '#planView', '预览');
    if (done.status !== 'success') {{ draft.planView = `<span class="badge bad">预览失败：${{esc(done.error||'')}}</span>`; }}
    else {{
      const r = done.result;
      const skipped = (r.skipped_company_files||[]).map(s => `<tr><td>${{esc(s.path)}}</td><td>${{esc(s.reason)}}</td></tr>`).join('');
      draft.planView = `<pre class="log">${{esc(r.summary||'(无改动)')}}</pre>
        ${{skipped?`<p>公司文件保护（已跳过）：</p><table><tr><th>路径</th><th>原因</th></tr>${{skipped}}</table>`:''}}
        <p class="muted">无改动时不触发应用任务。</p>`;
      draft.planViewData = r;
    }}
    await saveDraft(); render();
  }} catch (e) {{ $('#planView').textContent = '失败：' + e.message; }}
}};
window.runApply = async function() {{
  try {{
    const j = await api('POST', '/api/jobs/apply', {{ plan_job_id: draft.planJob }});
    draft.applyJob = j.job_id;
    const done = await waitJob(j.job_id, '#applyView', '应用');
    if (done.status !== 'success') {{ draft.applyView = `<span class="badge bad">应用失败：${{esc(done.error||'')}}</span>`; }}
    else {{
      const items = (done.result.verification.items||[]).map(i =>
        `<tr><td>${{esc(i.tool)}}</td><td>${{esc(i.path)}}</td><td>${{hostStateBadge(i.host_state)}}</td><td class="muted">${{esc(i.note)}}</td></tr>`).join('');
      draft.applyView = `<p class="badge ok">已写入 ${{(done.result.applied||[]).length}} 项</p>
        <table><tr><th>宿主</th><th>目标</th><th>状态</th><th>说明</th></tr>${{items}}</table>
        <p class="muted">${{esc(done.result.next||'')}}</p>
        <p>下一步动作：在选定的宿主里<b>新开会话</b>，让它调用你刚启用的技能；回复内容与技能预期一致即验证通过。</p>`;
    }}
    await saveDraft(); render();
  }} catch (e) {{ $('#applyView').textContent = '失败：' + e.message; }}
}};
window.undoApply = async function() {{
  try {{
    const v = await api('POST', '/api/jobs/undo', {{ id: draft.applyJob }});
    alert('撤销完成：恢复 ' + (v.restored||[]).length + ' 项' + ((v.conflicts||[]).length ? ('；冲突保留 ' + v.conflicts.length + ' 项') : ''));
    render();
  }} catch (e) {{ alert('撤销失败：' + e.message); }}
}};

// ---------- 仓库与作用域（日常管理） ----------
async function renderScopes() {{
  const state = await api('GET', '/api/state');
  const eff = await api('GET', '/api/effective').catch(e => ({{ error: e.message }}));
  const repos = (state.repos||[]).map(r => {{
    const wts = Object.entries(r.worktrees||{{}}).map(([wtId, w]) =>
      `<tr><td>${{esc(w.id)}}</td><td>${{esc(w.path)}}</td><td>${{stateBadge(w.status)}}</td><td>${{esc(w.branch||'')}}</td><td>${{esc(w.first_seen||'')}}${{w.status==='missing'?` <button onclick=\"relinkWt('${{esc(r.repo_id)}}','${{esc(wtId)}}\")">重关联</button>`:''}}</td></tr>`).join('');
    return `<div class="step"><h2>${{esc(r.repo_id)}} ${{r.origin?`<span class="muted">${{esc(r.origin)}}</span>`:'<span class="badge">无远端</span>'}}</h2>
      <table><tr><th>登记 id</th><th>路径</th><th>状态</th><th>分支</th><th>首次登记</th></tr>${{wts}}</table>
      <p class="muted">仓库默认配置对全部工作树生效；每个工作树可单独覆盖。修改仓库默认前请先在「首次设置」第 4 步预览影响范围。</p></div>`;
  }}).join('') || '<p class="muted">还没有登记仓库：先走「首次设置」。</p>';
  const res = eff.resources ? Object.entries(eff.resources).map(([k, v]) =>
    `<tr><td>${{esc(k)}}</td><td>${{v.deployed?'<span class="badge ok">部署</span>':'<span class="badge">不部署</span>'}}</td><td>${{esc(v.origin?originLabel(v.origin):'未设置')}}</td></tr>`).join('') : '';
  const hosts = eff.hosts ? Object.entries(eff.hosts).map(([k, v]) =>
    `<tr><td>${{esc(k)}}</td><td>${{v.enabled?'<span class="badge ok">启用</span>':'<span class="badge">停用</span>'}}</td><td>${{esc(originLabel(v.origin))}}</td></tr>`).join('') : '';
  const caps = await api('GET', '/api/capabilities');
  const capRows = (caps.capabilities||[]).map(c =>
    `<tr><td>${{esc(c.tool)}}</td><td>${{esc(c.kind)}}</td><td>${{esc(c.scope)}}</td>
     <td>${{c.support==='native'?'<span class="badge ok">原生</span>':c.support==='generated'?'<span class="badge">生成入口</span>':c.support==='unsupported'?'<span class="badge bad">不支持</span>':'<span class="badge warn">未知</span>'}}</td>
     <td class="muted">${{esc(c.load_mode)}}</td><td>${{c.requires_new_session?'<span class="badge warn">需新会话</span>':'—'}}</td></tr>`).join('');
  $('#app').innerHTML = `
    <div class="step"><h2>作用域选择（子项目模板 / 工作树覆盖 / 恢复继承）</h2>
    <p class="muted">资源 ID 形如 personal/skill/personal/xxx；子项目为仓库内相对路径。恢复继承 = 将该层选择改为 inherit（回到下层值）。</p>
    <p>资源 <input id="selResource" style="width:36%" placeholder="完整资源 ID">
       状态 <select id="selState"><option>enable</option><option>disable</option><option>inherit</option></select>
       子项目 <input id="selSub" style="width:16%" placeholder="web（可空）">
       <label><input type="checkbox" id="selWt"> 仅当前工作树</label>
       <button onclick="applySelection()">写入选择（仅配置，不写仓库）</button>
       <span id="selMsg" class="muted"></span></p></div>
    <div class="step"><h2>有效配置（当前作用域，含来源）</h2>
    <table><tr><th>资源</th><th>有效值</th><th>来源</th></tr>${{res}}</table>
    <table><tr><th>宿主</th><th>有效值</th><th>来源</th></tr>${{hosts}}</table>
    <p class="muted">个人层只影响 AILoom 的部署期望；宿主从全局/祖先目录加载的能力不受本工具控制——界面区分「本工具未部署」与「宿主已禁用」。实际生效状态见下方能力矩阵与「需新会话」标记。</p></div>
    <div class="step"><h2>宿主能力矩阵（实际生效方式）</h2>
    <table><tr><th>宿主</th><th>资源</th><th>作用域</th><th>支持级别</th><th>加载方式</th><th>生效时机</th></tr>${{capRows}}</table></div>
    <div class="step"><h2>仓库默认变更影响预览</h2>
    <button onclick="previewRepoDefault()">预览各工作树影响（无写入）</button>
    <div id="previewView" class="muted">修改仓库默认前先预览；默认只应用当前工作树，不写所有发现的工作树。</div></div>
    ${{repos}}`;
}}
window.applySelection = async function() {{
  try {{
    const body = {{ state: $('#selState').value }};
    if ($('#selResource').value.trim()) body.resource = $('#selResource').value.trim();
    if ($('#selSub').value.trim()) body.subproject = $('#selSub').value.trim();
    if ($('#selWt').checked) body.worktree = true;
    const v = await api('POST', '/api/profile/select', body);
    $('#selMsg').textContent = `已写入 ${{v.scope}}（保存 ≠ 部署；部署走预览/应用）`;
    renderScopes();
  }} catch (e) {{ $('#selMsg').textContent = '失败：' + e.message; }}
}};
window.relinkWt = async function(repoId, wtId) {{
  const p = prompt('失联工作树的新绝对路径：');
  if (!p) return;
  try {{
    await api('POST', '/api/repo/relink', {{ repo_id: repoId, wt_id: wtId, new_path: p }});
    renderScopes();
  }} catch (e) {{ alert('重关联失败：' + e.message); }}
}};
window.previewRepoDefault = async function() {{
  try {{
    const v = await api('POST', '/api/preview/repo-default', {{}});
    const rows = (v.worktrees||[]).map(w =>
      `<tr><td>${{esc(w.worktree||'')}}</td><td>${{esc(w.branch||'')}}</td><td>${{w.pending||0}}</td><td>${{esc((w.error||w.summary||'(无改动)').trim().split('\n')[0]||'')}}</td></tr>`).join('');
    $('#previewView').innerHTML = `<table><tr><th>工作树</th><th>分支</th><th>待执行</th><th>摘要首行</th></tr>${{rows}}</table><p class="muted">${{esc(v.note||'')}}</p>`;
  }} catch (e) {{
    $('#previewView').textContent = '预览失败：' + e.message;
  }}
}};
function originLabel(o) {{
  if (typeof o === 'string') return o;
  const key = Object.keys(o||{{}})[0] || '';
  const inner = (o||{{}})[key];
  return key + (inner && inner.path ? `（${{inner.path}}）` : '');
}}

// ---------- 资源库与流程 ----------
async function renderLibrary() {{
  const lib = await api('GET', '/api/library/list');
  const items = (lib.entries||[]).map(e =>
    `<tr><td>${{esc(e.id)}}</td><td>${{esc(e.kind)}}</td><td>${{esc(e.description||'')}}</td>
     <td><button onclick="openResource('${{esc(e.id)}}')">编辑</button>
     <button onclick="deleteResource('${{esc(e.id)}}')">删除</button></td></tr>`).join('');
  const runs = (await api('GET', '/api/workflows').catch(() => ({{runs:[]}}))).runs||[];
  const runList = runs.map(r => `<option value="${{esc(r.id)}}">${{esc(r.id)}} ${{esc(r.name)}}${{r.downstream_needs_review?' ⚠需复核':''}}</option>`).join('');
  $('#app').innerHTML = `
    <div class="step"><h2>个人资源库 <span class="muted">${{esc(lib.path)}}（仓外）</span></h2>
    <p class="muted">远程团队源默认订阅只读；修改进入个人副本。保存 ≠ 应用 ≠ 贡献：保存只改库内文件，不部署到仓库，也不创建任何 commit/PR。</p>
    <table><tr><th>资源 ID</th><th>类型</th><th>说明</th><th></th></tr>${{items}}</table>
    <div id="editor" class="hidden"><h2>编辑 <span id="editId"></span></h2>
      <textarea id="editBox"></textarea><br>
      <button onclick="saveResource()">保存（指纹校验，外部改动会拒绝覆盖）</button>
      <span id="editMsg" class="muted"></span></div></div>
    <div class="step"><h2>流程包：对齐 → 规格 → 票据 → 实现</h2>
      <input id="wfName" placeholder="流程名，例如 登录重构" style="width:40%">
      <button onclick="wfNew()">新建流程</button>
      <select id="wfSel"><option value="">${{runList?'':'（无流程）'}}${{runList}}</option></select>
      <button onclick="wfShow()">打开</button>
      <div id="wfView"></div></div>`;
}}
window.deleteResource = async function(id) {{
  try {{
    const preview = await api('POST', '/api/library/delete', {{ id }});
    const scopes = (preview.affected_scopes||[]).join('; ') || '（无作用域引用）';
    if (!confirm(`删除 ${{id}}？受影响作用域：${{scopes}}`)) return;
    await api('POST', '/api/library/delete', {{ id, execute: true }});
    renderLibrary();
  }} catch (e) {{ alert('删除失败：' + e.message); }}
}};
window.exportArtifact = async function(runId, artId) {{
  const target = prompt('导出目标文件（绝对路径，将写入你指定位置）：');
  if (!target) return;
  try {{
    const preview = await api('POST', '/api/workflows/export', {{ id: runId, artifact_id: artId, target }});
    const note = preview.exists ? (preview.same_content ? '（内容一致）' : '（将覆盖现有文件）') : '（新文件）';
    if (!confirm('写入 ' + preview.target + '？' + note)) return;
    await api('POST', '/api/workflows/export', {{ id: runId, artifact_id: artId, target, execute: true }});
    alert('已导出（未提交到 Git；是否纳入版本控制由你决定）');
  }} catch (e) {{ alert('导出失败：' + e.message); }}
}};
window.openResource = async function(id) {{
  const v = await api('GET', '/api/library/resource?id=' + encodeURIComponent(id));
  $('#editor').classList.remove('hidden');
  $('#editId').textContent = id;
  $('#editBox').value = v.content;
  $('#editBox').dataset.fp = v.fingerprint;
  $('#editMsg').textContent = '';
}};
window.saveResource = async function() {{
  try {{
    await api('PUT', '/api/library/resource', {{ id: $('#editId').textContent, content: $('#editBox').value, base_fingerprint: $('#editBox').dataset.fp }});
    $('#editMsg').textContent = '已保存（未部署；部署走「首次设置」第 4-5 步）';
    window._latestFp = null;
    const v = await api('GET', '/api/library/resource?id=' + encodeURIComponent($('#editId').textContent));
    $('#editBox').dataset.fp = v.fingerprint;
  }} catch (e) {{
    $('#editMsg').textContent = e.status === 409 ? '文件已被外部修改：' + e.message : '失败：' + e.message;
  }}
}};
window.wfNew = async function() {{
  const r = await api('POST', '/api/workflows/new', {{ name: $('#wfName').value.trim() || '未命名流程' }});
  await renderLibrary();
  $('#wfSel').value = r.id;
  window.wfShow();
}};
window.wfShow = async function() {{
  const id = $('#wfSel').value;
  if (!id) return;
  const r = await api('GET', '/api/workflows/show?id=' + encodeURIComponent(id));
  const pack = (r.pack||[]).map(b => `<tr><td>${{esc(b.stage)}}</td><td>${{esc(b.resource_id||'未绑定')}}</td><td>${{esc(b.status)}}</td></tr>`).join('');
  const arts = (r.artifacts||[]).map(a =>
    `<tr><td>${{esc(a.stage)}}</td><td>${{esc(a.title)}}</td><td>v${{a.version}}</td><td>${{a.needs_review?'<span class="badge warn">需复核</span>':''}}</td>
     <td><button onclick="wfOpenArt('${{esc(r.id)}}','${{esc(a.id)}}')">查看</button>
     <button onclick="exportArtifact('${{esc(r.id)}}','${{esc(a.id)}}')">导出</button></td></tr>`).join('');
  $('#wfView').innerHTML = `
    <p>规格版本 v${{r.spec_version}} ${{r.downstream_needs_review?'<span class="badge warn">下游产物需复核</span>':''}}
    <button onclick="wfReviewed('${{esc(r.id)}}')">标记已复核</button></p>
    <table><tr><th>阶段</th><th>绑定 skill</th><th>状态</th></tr>${{pack}}</table>
    <table><tr><th>阶段</th><th>产物</th><th>版本</th><th></th><th></th></tr>${{arts}}</table>
    <p class="muted">绑定校验走真实资源身份：缺项显示缺失，不会从未知位置静默复制。规格变更后票据/实现/验收标记需复核，需人工确认。</p>
    <div>新增产物：阶段 <select id="wfStage">${{['align','spec','tickets','implementation','acceptance'].map(s=>`<option>${{s}}</option>`).join('')}}</select>
    标题 <input id="wfTitle" style="width:30%"> <button onclick="wfPut('${{esc(r.id)}}')">写入</button></div>
    <textarea id="wfContent" placeholder="产物正文（保存在机器数据区，默认不进公司仓库）"></textarea>
    <div id="wfArt" class="log hidden"></div>`;
}};
window.wfPut = async function(id) {{
  await api('POST', '/api/workflows/artifact', {{ id, stage: $('#wfStage').value, title: $('#wfTitle').value.trim() || '未命名', content: $('#wfContent').value }});
  window.wfShow();
}};
window.wfOpenArt = async function(id, artId) {{
  const v = await api('GET', '/api/workflows/artifact?id=' + encodeURIComponent(id) + '&artifact_id=' + encodeURIComponent(artId));
  $('#wfArt').classList.remove('hidden');
  $('#wfArt').textContent = v.content;
}};
window.wfReviewed = async function(id) {{
  await api('POST', '/api/workflows/reviewed', {{ id }});
  window.wfShow();
}};

// ---------- 启动：恢复草稿（不自动重放任何执行动作） ----------
(async function init() {{
  try {{
    const d = await api('GET', '/api/draft');
    draftRev = d.revision || 0;
    if (d.draft) draft = Object.assign(draft, d.draft);
  }} catch (e) {{}}
  render();
}})();
</script></body></html>"#,
        token = token
    )
}
