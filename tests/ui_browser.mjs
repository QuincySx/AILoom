// Local browser acceptance; needs an independently launched Chrome on :9231.
import { writeFile } from 'node:fs/promises';
const [url, output, mode = 'current'] = process.argv.slice(2);
const tab = await (await fetch('http://127.0.0.1:9231/json/new?' + encodeURIComponent(url), { method: 'PUT' })).json();
const socket = new WebSocket(tab.webSocketDebuggerUrl);
await new Promise(resolve => socket.addEventListener('open', resolve, { once: true }));
let seq = 0;
const pending = new Map(), errors = [];
socket.addEventListener('message', event => {
  const m = JSON.parse(event.data);
  if (m.id) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.reject(m.error) : p.resolve(m.result); }
  if (m.method === 'Runtime.exceptionThrown') errors.push(m.params.exceptionDetails.text + ': ' + JSON.stringify(m.params.exceptionDetails.exception));
});
function send(method, params = {}) {
  return new Promise((resolve, reject) => { const id = ++seq; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })); });
}
async function evaluate(expression) {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(JSON.stringify(r.exceptionDetails));
  return r.result.value;
}
async function waitFor(expression) {
  for (let n = 0; n < 100; n++) {
    if (await evaluate(expression)) return;
    await new Promise(r => setTimeout(r, 100));
  }
  await screenshot(output + '-failed.png');
  throw new Error('Timed out: ' + expression + '\n' + await evaluate('document.querySelector("#app").innerText'));
}
async function screenshot(path) {
  const shot = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
  await writeFile(path, Buffer.from(shot.data, 'base64'));
}
await send('Runtime.enable');
await send('Page.enable');
await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false });
await waitFor('!!document.querySelector("#app")?.textContent.trim()');
await screenshot(output + '-desktop.png');
if (mode === 'onboarding') {
  const [project, skill] = process.argv.slice(5);
  const click = async text => evaluate(`[...document.querySelectorAll('#app button')].find(b => b.textContent === ${JSON.stringify(text)}).click()`);
  await evaluate("location.hash='#/onboarding'");
  await waitFor("!!document.querySelector('.wizard-steps')");
  await click('1 选择项目');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='批准此目录')");
  await evaluate(`document.querySelector('#app input').value=${JSON.stringify(project)}`);
  await click('批准此目录');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent === '识别当前目录')");
  await click('识别当前目录');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent === '确认项目，下一步')");
  await click('确认项目，下一步');
  await waitFor('!!document.querySelector("[data-h=claude]")');
  await evaluate("document.querySelector('[data-h=claude]').checked=true; document.querySelector('[data-inline-import]').parentElement.open=true");
  await waitFor("!document.querySelector('[data-resource-options]').textContent.includes('正在读取')");
  if (!await evaluate("!!document.querySelector('[data-resource-options] input')")) {
  await evaluate("document.querySelector('[data-add]').click(); document.querySelector('[data-provider]').value='local'; document.querySelector('[data-provider]').dispatchEvent(new Event('change'))");
  await evaluate(`document.querySelector('[data-url]').value=${JSON.stringify(skill)}; document.querySelector('[data-form]').requestSubmit()`);
  await waitFor("!!document.querySelector('[data-confirm]') && !document.querySelector('[data-confirm]').disabled");
  await click('确认导入');
  await waitFor("!!document.querySelector('[data-resource-options] input')");
  }
  await evaluate("const selectedResource = document.querySelector('[data-resource-options] input'); selectedResource.checked=true; selectedResource.dispatchEvent(new Event('change'))");
  await click('保存选择，下一步预览');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='生成预览')");
  await click('生成预览');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='应用' && !b.disabled)");
  await click('应用');
  await waitFor("[...document.querySelectorAll('#app button')].some(b=>b.textContent==='完成设置，进入资源库')");
  await screenshot(output + '-applied.png');
  await click('完成设置，进入资源库');
  await waitFor("location.hash === '#/library'");
}
if (mode !== 'before') {
  await evaluate('location.hash = "#/library"');
  await waitFor('!!document.querySelector("[data-add]")');
  await evaluate('document.querySelector("[data-add]").click()');
  await waitFor('!document.querySelector("[data-import]").classList.contains("hidden")');
  const providers = await evaluate('[...document.querySelector("[data-provider]").options].map(o=>o.value)');
  if (providers.join(',') !== 'github,gitlab,git,local,entry') throw new Error('Missing source choices');
  for (const p of providers) {
    await evaluate(`document.querySelector('[data-provider]').value=${JSON.stringify(p)}; document.querySelector('[data-provider]').dispatchEvent(new Event('change'))`);
  }
  await evaluate("document.querySelector('[data-provider]').value='gitlab'; document.querySelector('[data-provider]').dispatchEvent(new Event('change'))");
  await screenshot(output + '-import.png');
  await send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: false });
  await screenshot(output + '-mobile.png');
  const overflow = await evaluate('({width:innerWidth, scroll:document.documentElement.scrollWidth})');
  if (overflow.scroll > overflow.width + 1) throw new Error('Horizontal overflow: ' + JSON.stringify(overflow));
}
console.log(JSON.stringify({ title: await evaluate('document.title'), errors, screenshots:output }));
socket.close();
await fetch('http://127.0.0.1:9231/json/close/' + tab.id);
if (errors.length) process.exitCode = 1;
