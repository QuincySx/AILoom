// AIL-121~128 验收公共驱动：连接本机 Chrome CDP :9231，提供导航/求值/等待/截图。
// 用法：import { withTab } from './cdp.mjs'
export async function connect() {
  const tab = await (await fetch('http://127.0.0.1:9231/json/new?about:blank', { method: 'PUT' })).json();
  const socket = new WebSocket(tab.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject); });
  let seq = 0;
  const pending = new Map();
  const consoleErrors = [];
  socket.addEventListener('message', event => {
    const m = JSON.parse(event.data);
    if (m.id) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.reject(new Error(JSON.stringify(m.error))) : p.resolve(m.result); }
    if (m.method === 'Runtime.exceptionThrown') consoleErrors.push(m.params.exceptionDetails.text + (m.params.exceptionDetails.exception?.description ? ': ' + m.params.exceptionDetails.exception.description : ''));
    if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') consoleErrors.push(m.params.args.map(a => a.value ?? a.description ?? '').join(' '));
  });
  const send = (method, params = {}) => new Promise((resolve, reject) => { const id = ++seq; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })); });
  const evaluate = async expression => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error('页面异常: ' + JSON.stringify(r.exceptionDetails.exception?.description || r.exceptionDetails.text));
    return r.result.value;
  };
  const waitFor = async (expression, timeout = 10000) => {
    for (let n = 0; n * 100 < timeout; n++) {
      if (await evaluate(expression)) return true;
      await new Promise(r => setTimeout(r, 100));
    }
    throw new Error('等待超时: ' + expression + '\n页面文本: ' + (await evaluate('document.querySelector("#app")?.innerText')).slice(0, 800));
  };
  const screenshot = async path => {
    const shot = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    const { writeFile } = await import('node:fs/promises');
    await writeFile(path, Buffer.from(shot.data, 'base64'));
  };
  const goto = async hash => {
    await send('Page.navigate', { url: `http://127.0.0.1:8648/?token=${sandboxToken()}${hash}` });
    await new Promise(r => setTimeout(r, 600));
  };
  const viewport = (width, height) => send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: width < 700 });
  await send('Runtime.enable');
  await send('Page.enable');
  return { socket, send, evaluate, waitFor, screenshot, goto, viewport, consoleErrors,
    async close() { try { await fetch('http://127.0.0.1:9231/json/close/' + tab.id); } catch {} socket.close(); } };
}

// 简单断言器：全绿才 PASS；失败打印上下文并抛出。
export class Checks {
  constructor(name) { this.name = name; this.n = 0; this.fails = []; }
  async check(label, cond) {
    this.n++;
    const ok = typeof cond === 'function' ? await cond() : cond;
    console.log(`${ok ? 'PASS' : 'FAIL'} ${this.n}. ${label}`);
    if (!ok) this.fails.push(label);
    return ok;
  }
  finish() {
    console.log(`\n== ${this.name}: ${this.n - this.fails.length}/${this.n} PASS ==`);
    if (this.fails.length) { console.log('失败项: ' + this.fails.join(' | ')); process.exit(1); }
  }
}

// 读取沙盒 console 的当前 token。
import { readFileSync } from 'node:fs';
export const sandboxToken = () => {
  const line = readFileSync('/tmp/ailoom-dirux/console.log', 'utf8').match(/token=([a-f0-9-]+)/g)?.pop();
  return line.slice(6);
};
export const API = 'http://127.0.0.1:8648/api';
export async function apiCall(path, body, method) {
  const r = await fetch(API + path, {
    method: method || (body === undefined ? 'GET' : 'POST'),
    headers: { 'X-AILoom-Session': sandboxToken(), 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return r.json();
}
