// Node 内置测试；这里只验证组件状态，不冒充真实浏览器验收。
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';
import path from 'node:path';

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.value = ''; this.style = {}; this.classList = { add() {}, remove() {} }; }
  append(...nodes) { this.children.push(...nodes); }
  appendChild(node) { this.append(node); }
  setAttribute(name, value) { (this.attributes ??= {})[name] = value; }
  remove() { this.removed = true; }
}

async function component(file) {
  const document = {createElement: tag => new Element(tag), activeElement: null};
  const module = new vm.SourceTextModule(await readFile('src/console/ui/components/' + file, 'utf8'), {context:vm.createContext({document})});
  await module.link(() => { throw new Error('Unexpected dependency'); });
  await module.evaluate();
  return module.namespace;
}

test('按钮变体、重复点击保护，以及异步期间销毁', async () => {
  const {Button} = await component('button.js');
  const root = new Element('div');
  let release, calls = 0;
  const button = Button(root, {label:'保存', variant:'default', onPress:() => { calls++; return new Promise(resolve => {release=resolve;}); }});
  const el = root.children[0];
  assert.equal(el.attributes['data-variant'], 'default');
  const pending = el.onclick();
  assert.equal(el.attributes['aria-busy'], 'true');
  assert.equal(el.disabled, true);
  await el.onclick();
  assert.equal(calls, 1);
  button.destroy(); release(); await pending;
  assert.equal(el.removed, true);
});

test('Field 关联标签、错误语义和禁用状态', async () => {
  const {Field} = await component('field.js');
  const root = new Element('div');
  const field = Field(root, {label:'名称', error:'必填', disabled:true});
  const [label,input,error] = root.children[0].children;
  assert.equal(label.htmlFor, input.id);
  assert.equal(input.attributes['aria-invalid'], 'true');
  assert.equal(error.attributes.role, 'alert');
  assert.equal(input.disabled, true);
  field.update({error:null, disabled:false});
  assert.equal(input.attributes['aria-invalid'], 'false');
  assert.equal(input.disabled, false);
});

test('活动样式只有一个 Token 来源，组件与布局不写独立色值', async () => {
  const root = 'src/console/ui/';
  const tokens = await readFile(root + 'tokens.css', 'utf8');
  const base = await readFile(root + 'base.css', 'utf8');
  const controls = await readFile(root + 'components.css', 'utf8');
  const shell = await readFile(root + 'shell.html', 'utf8');
  assert.ok(!shell.includes('/ui/theme.css') && !shell.includes('/ui/theme.js'));
  assert.ok(shell.includes('/ui/components.css'));
  const defined = new Set([...tokens.matchAll(/(--[\w-]+)\s*:/g)].map(m => m[1]));
  for (const [,name] of (tokens + base + controls).matchAll(/var\((--[\w-]+)/g)) assert.ok(defined.has(name), name + ' must be defined');
  assert.ok(!/(?:#[\da-f]{3,8}\b|rgba?\(|hsla?\(|oklch\()/i.test(base + controls), 'color values belong in tokens.css');
  for (const name of ['--background','--foreground','--primary','--primary-foreground','--border','--input','--ring']) assert.ok(defined.has(name));
});

test('编辑器标记输入、保留脏正文、正确处理保存期间的新输入', async () => {
  const document = { createElement: tag => new Element(tag), activeElement: null };
  const context = vm.createContext({ document });
  const modules = new Map();
  async function load(file) {
    if (modules.has(file)) return modules.get(file);
    const module = new vm.SourceTextModule(await readFile(file, 'utf8'), { context, identifier: file });
    modules.set(file, module);
    await module.link((name, referencing) => load(path.resolve(path.dirname(referencing.identifier), name)));
    return module;
  }
  const mod = await load(path.resolve('src/console/ui/components/editor.js'));
  await mod.evaluate();
  const root = new Element('div');
  let changed;
  const editor = mod.namespace.Editor(root, { content: 'base', onChange: v => { changed = v; } });
  const input = root.children[0].children[0].children[1];
  assert.equal(editor.value(), 'base');
  input.value = 'my draft'; input.oninput();
  assert.equal(editor.isDirty(), true);
  assert.equal(changed, 'my draft');
  editor.update({ content: 'server response' });
  assert.equal(editor.value(), 'my draft', '过期响应不能覆盖用户输入');
  editor.markSaved('earlier submitted text');
  assert.equal(editor.isDirty(), true, '保存期间的新输入仍是未保存');
  editor.markSaved('my draft');
  assert.equal(editor.isDirty(), false);
  input.value = 'later'; input.oninput();
  editor.setValue('explicit reload');
  assert.equal(editor.value(), 'explicit reload');
  assert.equal(editor.isDirty(), false);
  editor.destroy(); assert.equal(root.children[0].removed, true);
});
