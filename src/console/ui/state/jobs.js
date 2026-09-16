// AIL-086：任务轮询状态 —— 轮询可取消（页面销毁/切页 destroy），超时显式报错；
// 服务端执行中的任务不因页面销毁而假称取消（cancel 仅请求安全边界停止）。

import { api, ApiError } from '../services/api.js';

export function waitJob(id, { onProgress, timeoutMs = 20000, signal } = {}) {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;
    let stopped = false;
    const stop = () => { stopped = true; };
    if (signal) signal.addEventListener('abort', stop, { once: true });
    const tick = async () => {
      if (stopped) return; // 页面销毁：停止读取，服务端任务继续
      try {
        const j = await api.job(id);
        if (j.status !== 'queued' && j.status !== 'running') return resolve(j);
        if (onProgress) onProgress((j.progress ?? []).slice(-1)[0]);
        if (Date.now() > deadline) {
          return reject(new ApiError('server', undefined, { error: '任务超时未完成：' + id }));
        }
        setTimeout(tick, 250);
      } catch (e) {
        if (e.kind === 'offline' && !stopped) {
          // 断线重连查询（不自动 POST）
          setTimeout(tick, 1000);
          return;
        }
        reject(e);
      }
    };
    tick();
  });
}
