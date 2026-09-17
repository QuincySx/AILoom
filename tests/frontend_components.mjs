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
