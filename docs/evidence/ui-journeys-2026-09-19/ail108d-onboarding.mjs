// AIL-108 补齐：onboarding 六步向导状态机驱动（从任意步骤走完）
import { connect } from './cdp.mjs';
import { execSync } from 'node:child_process';
const PROJECT = '/tmp/ailoom-flow/projects/onboard';
const b = await connect('http://127.0.0.1:8642/');
await b.waitFor('!!document.querySelector("#nav")');
await b.goto('#/onboarding');
await b.waitFor(`!!document.querySelector('.wizard-steps')`);
const step = async () => {
  const on = await b.evaluate(`document.querySelector('.wizard-steps button.on')?.textContent || ''`);
  return on;
};
const clickBtn = async (text) => {
  await b.waitFor(`[...document.querySelectorAll('#app button')].some(x=>x.textContent===${JSON.stringify(text)} && !x.disabled)`);
  await b.evaluate(`[...document.querySelectorAll('#app button')].find(x=>x.textContent===${JSON.stringify(text)}).click()`);
};
const confirmIfOpen = async (label) => {
  if (await b.evaluate(`!!document.querySelector('dialog[open] .confirmation-message')`)) {
    await b.evaluate(`[...document.querySelectorAll('dialog[open] .dialog-actions button')].find(b=>b.textContent===${JSON.stringify(label)}).click()`);
    return true;
  }
  return false;
};

for (let guard = 0; guard < 30; guard++) {
  const s = await step();
  console.log('当前步骤:', s || '(未知)');
  if (await b.evaluate(`location.hash === '#/library'`)) { console.log('已进入资源库'); break; }
  if (s.includes('1 选择项目')) {
    await b.waitFor(`!!document.querySelector('#app input')`);
    await b.evaluate(`(()=>{ const i=document.querySelector('#app input'); i.value=${JSON.stringify(PROJECT)}; i.dispatchEvent(new Event('input')); })()`);
    await clickBtn('批准此目录');
    await new Promise(r=>setTimeout(r,800));
  } else if (s.includes('2 确认工作树')) {
    if (await b.evaluate(`!!document.querySelector('[data-pick]')`)) {
      await b.evaluate(`document.querySelector('[data-pick]').click()`);
      await new Promise(r=>setTimeout(r,300));
    }
    await clickBtn('确认项目，下一步');
    await new Promise(r=>setTimeout(r,500));
  } else if (s.includes('3 选择资源')) {
    await clickBtn('探测本机宿主（只读 --version）');
    await b.waitFor(`document.querySelector('#app').innerText.includes('Claude: 2.')`);
    await b.evaluate(`(()=>{ const h=document.querySelector('[data-h="claude"]'); if(!h.checked){h.checked=true;h.dispatchEvent(new Event('change'));} })()`);
    await b.waitFor(`!!document.querySelector('[data-resource-options] input')`);
    await b.evaluate(`(()=>{ const box=[...document.querySelectorAll('[data-resource-options] input')].find(x=>x.value.includes('/skill/common/code-review')); box.checked=true; box.dispatchEvent(new Event('change')); })()`);
    await b.screenshot('/tmp/ailoom-flow/evidence/ail108b-onboarding-step3.png');
    await clickBtn('保存选择，下一步预览');
    await new Promise(r=>setTimeout(r,900));
  } else if (s.includes('4 预览改动')) {
    await clickBtn('生成预览');
    await new Promise(r=>setTimeout(r,1500));
    await b.screenshot('/tmp/ailoom-flow/evidence/ail108b-onboarding-preview.png');
  } else if (s.includes('5 确认应用')) {
    await clickBtn('应用');
    await confirmIfOpen('确认应用');
    await new Promise(r=>setTimeout(r,1500));
  } else if (s.includes('6 完成设置')) {
    await b.screenshot('/tmp/ailoom-flow/evidence/ail108b-onboarding-applied.png');
    await clickBtn('完成设置，进入资源库');
    await new Promise(r=>setTimeout(r,600));
  } else {
    await new Promise(r=>setTimeout(r,500));
  }
}
if (!(await b.evaluate(`location.hash === '#/library'`))) throw new Error('向导未走完');
console.log('六步全部完成并进入资源库');
const has = execSync(`test -e ${JSON.stringify(PROJECT + '/.claude/skills/code-review')} && echo yes || echo no`).toString().trim();
console.log('onboard 目录产物:', has);
if (has !== 'yes') throw new Error('向导应用应写入 skill 产物');
console.log('ERRORS:', JSON.stringify(b.errors));
if (b.errors.length) throw new Error('页面存在未处理异常');
await b.close();
console.log('ONBOARDING WIZARD PASSED');
