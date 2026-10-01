import { execFile } from 'node:child_process';
import { realpath, stat } from 'node:fs/promises';
import { isAbsolute } from 'node:path';

export async function projectPath(root) {
  if (typeof root !== 'string' || !isAbsolute(root)) throw new Error('请提供项目的绝对路径');
  const path = await realpath(root);
  if (!(await stat(path)).isDirectory()) throw new Error('项目路径不是文件夹');
  return path;
}

// No shell, terminal injection, HTTP server, or independently maintained business logic.
export async function runCli({ executable = 'ailoom', root, dataRoot, action, timeout = 120000 }) {
  if (!['version', 'status', 'plan', 'sync', 'recover'].includes(action)) throw new Error('不支持的操作');
  if (executable !== 'ailoom' && !isAbsolute(executable)) throw new Error('CLI 路径必须是绝对路径');
  const args = ['--json'];
  if (dataRoot) {
    if (!isAbsolute(dataRoot)) throw new Error('数据目录必须是绝对路径');
    args.push('--data-root', dataRoot);
  }
  args.push(action === 'recover' ? 'sync' : action);
  if (action !== 'version') args.push('--root', await projectPath(root));
  if (action === 'recover') args.push('--recover');
  return new Promise((resolve, reject) => {
    execFile(executable, args, { timeout, maxBuffer: 8 * 1024 * 1024, windowsHide: true }, (error, stdout, stderr) => {
      if (error) {
        const message = error.code === 'ENOENT' ? '找不到 AILoom CLI，请配置 executable 的绝对路径'
          : error.killed ? 'AILoom 执行超时；请检查状态，必要时恢复未完成的操作'
          : stderr.trim() || error.message;
        reject(new Error(message));
        return;
      }
      try {
        const envelope = JSON.parse(stdout);
        if (envelope.schema_version !== 1 || !Object.hasOwn(envelope, 'result')) throw new Error('不兼容的 CLI 输出版本');
        resolve(envelope.result);
      } catch (error) { reject(new Error(`无法读取 AILoom 结果：${error.message}`)); }
    });
  });
}
