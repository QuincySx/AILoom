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
  nativeFiles: body => request('POST', '/api/native-files', body),
  serviceStatus: () => request('POST', '/api/service/status', {}),
  serviceAutostart: enabled => request('POST', '/api/service/autostart', {enabled}),
  knowledge: body => request('POST', '/api/knowledge', body),
  // 通用
  serverInfo: () => request('GET', '/api/server-info'),
  state: () => request('GET', '/api/state'),
  capabilities: () => request('GET', '/api/capabilities'),
  shutdown: () => request('POST', '/api/shutdown', {}),
  // 文件系统（授权根内）
  approveDir: (path) => request('POST', '/api/fs/approve', { path }),
  pickDirectory: () => request('POST', '/api/fs/pick-directory', {}),
  projectMetadata: (body) => request('POST', '/api/projects/metadata', body),
  fsList: (path) => request('GET', '/api/fs/list' + qs({ path })),
  fsRead: (path) => request('GET', '/api/fs/read' + qs({ path })),
  // 仓库/作用域
  repoDiscover: (path) => request('POST', '/api/repo/discover', { path }),
  select: (body) => request('POST', '/api/profile/select', body),
  effective: (root, scope, view) => request('GET', '/api/effective' + qs({ root, scope, view })),
  configureScope: body => request('POST', '/api/profile/scope', body),
  discoverDirectories: (root, depth=3) => request('GET', '/api/project/discover-directories' + qs({ root, depth })),
  projectDirs: (root) => request('GET', '/api/project/dirs' + qs({ root })),
  instructions: (body) => request('POST', '/api/profile/instructions', body),
  deployStatus: (root, scope) => request('GET', '/api/deploy-status' + qs({ root, scope })),
  // 草稿
  getDraft: () => request('GET', '/api/draft'),
  putDraft: (baseRevision, draft) => request('PUT', '/api/draft', { base_revision: baseRevision, draft }),
  // 资源库
  libraryList: () => request('GET', '/api/library/list'),
  resources: () => request('GET', '/api/resources'),
  scanProjectSkills: (root, sub) => request('GET', '/api/project/scan-skills' + qs({ root, sub })),
  projectDeletePreview: (root, sub, path) => request('POST', '/api/project/delete-skill', { root, sub, path, execute: false }),
  projectDeleteExecute: (root, sub, token, name) => request('POST', '/api/project/delete-skill', { root, sub, token, name, execute: true }),
  mcpDetail: (id) => request('GET', '/api/resources/mcp-detail' + qs({ id })),
  collections: () => request('GET', '/api/collections'),
  ccSwitchScan: (manifest) => request('POST', '/api/migrations/cc-switch/scan', { manifest }),
  ccSwitchLocation: () => request('GET', '/api/migrations/cc-switch/location'),
  ccSwitchRead: (directory) => request('POST', '/api/migrations/cc-switch/read', { directory, confirm_source_read:true }),
  ccSwitchPreview: (scanId, selected, management = 'managed', skillsDirectory) => request('POST', '/api/migrations/cc-switch/preview', { scan_id:scanId, selected, management, skills_directory:skillsDirectory }),
  ccSwitchApply: (previewId) => request('POST', '/api/migrations/cc-switch/apply', { preview_id:previewId }),
  collectionCheck: (sourceId) => request('POST', '/api/collections/check', { source_id: sourceId }),
  collectionUpdate: (previewIds) => request('POST', '/api/collections/update', { preview_ids: previewIds }),
  collectionRemove: (sourceId, execute = false) => request('POST', '/api/collections/remove', { source_id: sourceId, execute }),
  collectionPreview: (body) => request('POST', '/api/collections/preview', body),
  collectionApply: (previewId) => request('POST', '/api/collections/apply', { preview_id: previewId }),
  libraryImport: (dir, name, execute) => request('POST', '/api/library/import', { dir, name, execute }),
  libraryImportEntry: (entry, name, execute) => request('POST', '/api/library/import-entry', { entry, name, execute }),
  libraryDelete: (id, execute) => request('POST', '/api/library/delete', { id, execute }),
  libraryResource: (id) => request('GET', '/api/library/resource' + qs({ id })),
  libraryDefinitionSave: (id, definition, baseFingerprint) => request('PUT', '/api/library/resource', {id,definition,base_fingerprint:baseFingerprint}),
  libraryCreate: (fields) => request('POST', '/api/library/resource', fields),
  librarySave: (id, content, baseFingerprint) => request('PUT', '/api/library/resource', { id, content, base_fingerprint: baseFingerprint }),
  checkUpdate: (skill) => request('POST', '/api/library/check-update', { skill }),
  updateSkill: (skill, execute, previewId) => request('POST', '/api/library/update', { skill, execute, preview_id:previewId }),
  // 任务
  plan: (root, scope, idempotencyKey) => request('POST', '/api/jobs/plan', { root, scope, idempotency_key: idempotencyKey }),
  // 撤回中途失败的同步（journal 恢复），与 `ailoom personal --action recover` 同一实现
  projectRecover: (root) => request('POST', '/api/project/recover', { root }),
  apply: (planJobId, idempotencyKey) => request('POST', '/api/jobs/apply', { plan_job_id: planJobId, idempotency_key: idempotencyKey }),
  job: (id) => request('GET', '/api/jobs/' + encodeURIComponent(id)),
  jobs: () => request('GET', '/api/jobs'),
  undo: (id) => request('POST', '/api/jobs/undo', { id }),
  // 流程
};

export function esc(s) {
  const d = document.createElement('div');
  d.textContent = String(s ?? '');
  return d.innerHTML.replaceAll('"', '&quot;').replaceAll("'", '&#39;');
}
