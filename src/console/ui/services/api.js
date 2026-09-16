// AIL-085：唯一网络入口与领域 API 方法。
// 约定：会话 token 只存在本模块内存中（bootstrap 注入），不写 localStorage/日志。
// 所有错误以 ApiError 抛出：conflict(409)/validation(400|422)/auth(401|403)/
// offline(网络失败)/server(5xx)；所有错误必须由调用方给出可见反馈。

const TOKEN = window.__AILOOM?.token ?? '';
const H = () => ({ 'X-AILoom-Session': TOKEN, 'Content-Type': 'application/json' });

export class ApiError extends Error {
  constructor(kind, status, data) {
    super(data?.error || `${kind} (${status ?? 'network'})`);
    this.kind = kind; // 'conflict'|'validation'|'auth'|'offline'|'server'
    this.status = status;
    this.data = data ?? {};
  }
}

async function request(verb, path, body) {
  let resp;
  try {
    resp = await fetch(path, {
      method: verb,
      headers: H(),
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (e) {
    throw new ApiError('offline', undefined, { error: '无法连接本地服务（可能已退出）' });
  }
  const data = await resp.json().catch(() => ({}));
  if (resp.ok) return data;
  const kind =
    resp.status === 409 ? 'conflict'
    : resp.status === 401 || resp.status === 403 ? 'auth'
    : resp.status === 400 || resp.status === 404 || resp.status === 422 ? 'validation'
    : 'server';
  throw new ApiError(kind, resp.status, data);
}

function qs(params) {
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(params ?? {})) {
    if (v !== undefined && v !== null && v !== '') u.set(k, v);
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

export const api = {
  // 通用
  serverInfo: () => request('GET', '/api/server-info'),
  state: () => request('GET', '/api/state'),
  capabilities: () => request('GET', '/api/capabilities'),
  detectHosts: () => request('POST', '/api/hosts/detect', {}),
  shutdown: () => request('POST', '/api/shutdown', {}),
  // 文件系统（授权根内）
  approveDir: (path) => request('POST', '/api/fs/approve', { path }),
  fsList: (path) => request('GET', '/api/fs/list' + qs({ path })),
  fsRead: (path) => request('GET', '/api/fs/read' + qs({ path })),
  // 仓库/作用域
  repoDiscover: (path) => request('POST', '/api/repo/discover', { path }),
  relink: (repoId, wtId, newPath) => request('POST', '/api/repo/relink', { repo_id: repoId, wt_id: wtId, new_path: newPath }),
  select: (body) => request('POST', '/api/profile/select', body),
  effective: (root) => request('GET', '/api/effective' + qs({ root })),
  previewRepoDefault: () => request('POST', '/api/preview/repo-default', {}),
  instructions: (body) => request('POST', '/api/profile/instructions', body),
  // 草稿
  getDraft: () => request('GET', '/api/draft'),
  putDraft: (baseRevision, draft) => request('PUT', '/api/draft', { base_revision: baseRevision, draft }),
  // 个人库
  libraryList: () => request('GET', '/api/library/list'),
  libraryImport: (dir, name, execute) => request('POST', '/api/library/import', { dir, name, execute }),
  libraryImportGit: (url, repoPath, ref, name, execute) => request('POST', '/api/library/import-git', { url, path: repoPath, ref, name, execute }),
  libraryImportEntry: (entry, name, execute) => request('POST', '/api/library/import-entry', { entry, name, execute }),
  librarySources: () => request('GET', '/api/library/sources'),
  libraryDelete: (id, execute) => request('POST', '/api/library/delete', { id, execute }),
  libraryResource: (id) => request('GET', '/api/library/resource' + qs({ id })),
  librarySave: (id, content, baseFingerprint) => request('PUT', '/api/library/resource', { id, content, base_fingerprint: baseFingerprint }),
  checkUpdate: (skill) => request('POST', '/api/library/check-update', { skill }),
  updateSkill: (skill, execute) => request('POST', '/api/library/update', { skill, execute }),
  // 任务
  plan: (root, scope, idempotencyKey) => request('POST', '/api/jobs/plan', { root, scope, idempotency_key: idempotencyKey }),
  apply: (planJobId, idempotencyKey) => request('POST', '/api/jobs/apply', { plan_job_id: planJobId, idempotency_key: idempotencyKey }),
  job: (id) => request('GET', '/api/jobs/' + encodeURIComponent(id)),
  jobs: () => request('GET', '/api/jobs'),
  cancel: (id) => request('POST', `/api/jobs/${encodeURIComponent(id)}/cancel`, {}),
  undo: (id) => request('POST', '/api/jobs/undo', { id }),
  // 流程
  workflows: () => request('GET', '/api/workflows'),
  workflowShow: (id) => request('GET', '/api/workflows/show' + qs({ id })),
  workflowNew: (name) => request('POST', '/api/workflows/new', { name }),
  workflowBind: (id, bindings) => request('POST', '/api/workflows/bind', { id, bindings }),
  workflowArtifact: (id, stage, title, content, baseVersion) => request('POST', '/api/workflows/artifact', { id, stage, title, content, base_version: baseVersion }),
  workflowArtifactRead: (id, artifactId, version) => request('GET', '/api/workflows/artifact' + qs({ id, artifact_id: artifactId, version })),
  workflowRename: (id, artifactId, title) => request('POST', '/api/workflows/rename', { id, artifact_id: artifactId, title }),
  workflowReviewed: (id) => request('POST', '/api/workflows/reviewed', { id }),
  workflowRecordInput: (id, resourceId) => request('POST', '/api/workflows/record-input', { id, resource_id: resourceId }),
  exportPreview: (id, artifactId, target) => request('POST', '/api/workflows/export', { id, artifact_id: artifactId, target }),
  exportExecute: (id, artifactId, target, fingerprint) => request('POST', '/api/workflows/export', { id, artifact_id: artifactId, target, execute: true, target_fingerprint: fingerprint }),
};

export function esc(s) {
  const d = document.createElement('div');
  d.textContent = String(s ?? '');
  return d.innerHTML;
}
