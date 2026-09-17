// HTML combobox/listbox. The hidden select is only the existing form/value adapter;
// neither the trigger nor the option menu uses the operating system's select UI.
let sequence = 0;
let opened = null;

export function Select(source) {
  const wasFocused = document.activeElement === source;
  const id = `select-${++sequence}`;
  const wrapper = document.createElement('span');
  wrapper.className = 'select-control';
  wrapper.style.width = source.style.width;
  const trigger = document.createElement('button');
  trigger.type = 'button';
  trigger.className = 'select-trigger';
  trigger.id = `${id}-trigger`;
  trigger.setAttribute('role', 'combobox');
  trigger.setAttribute('aria-haspopup', 'listbox');
  trigger.setAttribute('aria-controls', `${id}-list`);
  const text = document.createElement('span');
  trigger.append(text);
  const list = document.createElement('div');
  list.id = `${id}-list`;
  list.className = 'select-menu';
  list.setAttribute('role', 'listbox');
  list.setAttribute('popover', 'manual');
  source.before(wrapper);
  wrapper.append(source, trigger, list);
  source.classList.add('select-source');
  const previousTabIndex = source.getAttribute('tabindex');
  const previousAriaHidden = source.getAttribute('aria-hidden');
  source.tabIndex = -1;
  source.setAttribute('aria-hidden', 'true');
  const events = new AbortController();
  const listen = (el, type, fn, capture = false) => el.addEventListener(type, fn, {signal:events.signal,capture});
  let active = -1, isOpen = false, disposed = false, prefix = '', lastTyped = 0;
  const options = () => [...source.options];
  const enabled = option => !option.disabled && !option.hidden && !option.parentElement?.disabled;
  const indices = () => options().flatMap((o,i) => enabled(o) ? [i] : []);
  function label() {
    return source.getAttribute('aria-label') || [...source.labels].map(l => [...l.childNodes].filter(n => n !== wrapper).map(n => n.textContent).join('')).join(' ').trim() || '选择选项';
  }
  function sync() {
    if (disposed) return;
    const selected = source.options[source.selectedIndex];
    text.textContent = selected?.label || '请选择';
    trigger.disabled = source.matches(':disabled') || !!source.closest('[aria-busy="true"]');
    trigger.setAttribute('aria-label', label());
    list.setAttribute('aria-label', label());
    for (const attr of ['aria-describedby','aria-invalid','aria-required','aria-labelledby']) {
      const value = source.getAttribute(attr);
      if (value === null) trigger.removeAttribute(attr); else trigger.setAttribute(attr,value);
    }
    if (source.required) trigger.setAttribute('aria-required','true');
    trigger.setAttribute('aria-expanded', String(isOpen));
    if (trigger.disabled && isOpen) close();
    if (isOpen) renderOptions();
  }
  function renderOptions() {
    list.replaceChildren();
    let lastGroup = null;
    options().forEach((option,index) => {
      if (option.hidden) return;
      const group = option.parentElement.tagName === 'OPTGROUP' ? option.parentElement : null;
      if (group && group !== lastGroup) {
        const heading = document.createElement('div'); heading.className = 'select-group'; heading.textContent = group.label;
        list.append(heading);
      }
      lastGroup = group;
      const row = document.createElement('div');
      row.id = `${id}-option-${index}`; row.className = 'select-option'; row.dataset.index = index;
      row.setAttribute('role','option');
      row.setAttribute('aria-selected',String(index === source.selectedIndex));
      row.setAttribute('aria-disabled',String(!enabled(option)));
      row.textContent = option.label;
      list.append(row);
    });
    if (!indices().includes(active)) active = indices()[0] ?? -1;
    highlight(false);
    position();
  }
  function highlight(scroll = true) {
    list.querySelectorAll('[role=option]').forEach(row => row.classList.toggle('active',Number(row.dataset.index) === active));
    if (active >= 0) {
      trigger.setAttribute('aria-activedescendant',`${id}-option-${active}`);
      if (scroll) document.getElementById(`${id}-option-${active}`)?.scrollIntoView({block:'nearest'});
    } else trigger.removeAttribute('aria-activedescendant');
  }
  function position() {
    if (!isOpen) return;
    const rect = trigger.getBoundingClientRect();
    const width = Math.min(Math.max(rect.width,180),innerWidth - 16);
    const below = innerHeight - rect.bottom - 12, above = rect.top - 12;
    const bottom = below >= Math.min(280,list.scrollHeight) || below >= above;
    list.style.width = width + 'px';
    list.style.maxHeight = Math.max(44, Math.min(320,bottom ? below : above)) + 'px';
    list.style.left = Math.max(8,Math.min(rect.left,innerWidth - width - 8)) + 'px';
    list.style.top = (bottom ? rect.bottom + 4 : Math.max(8,rect.top - list.getBoundingClientRect().height - 4)) + 'px';
  }
  function close() {
    if (!isOpen) return;
    isOpen = false; if(list.matches(':popover-open')) list.hidePopover();
    trigger.setAttribute('aria-expanded','false'); trigger.removeAttribute('aria-activedescendant');
    if (opened === controller) opened = null;
  }
  function open() {
    sync();
    if (trigger.disabled || isOpen) return;
    opened?.close(); opened = controller;
    active = indices().includes(source.selectedIndex) ? source.selectedIndex : (indices()[0] ?? -1);
    isOpen = true; trigger.setAttribute('aria-expanded','true');
    renderOptions(); list.showPopover(); position(); highlight(); trigger.focus();
  }
  function choose(index) {
    if (!enabled(source.options[index] || {disabled:true})) return;
    const changed = source.selectedIndex !== index;
    source.selectedIndex = index;
    close(); trigger.focus();
    if (changed) { source.dispatchEvent(new Event('input',{bubbles:true})); source.dispatchEvent(new Event('change',{bubbles:true})); }
  }
  listen(trigger,'click',() => isOpen ? close() : open());
  listen(trigger,'keydown',event => {
    const key = event.key;
    if (key === 'Escape' && isOpen) { event.preventDefault(); event.stopPropagation(); close(); return; }
    if (key === 'Tab') { if (isOpen) choose(active); return; }
    if (['ArrowDown','ArrowUp','Home','End','PageDown','PageUp','Enter',' '].includes(key)) {
      event.preventDefault();
      if (!isOpen) { open(); if (key !== 'Home' && key !== 'End') return; }
      else if (key === 'Enter' || key === ' ') { choose(active); return; }
      const values = indices(), at = values.indexOf(active);
      const delta = key === 'PageDown' ? 10 : key === 'PageUp' ? -10 : key === 'ArrowDown' ? 1 : -1;
      active = key === 'Home' ? values[0] : key === 'End' ? values.at(-1) : values[Math.max(0,Math.min(values.length-1,at+delta))];
      highlight();
    } else if (key.length === 1 && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault(); open();
      const now = Date.now(); prefix = now-lastTyped > 700 ? key : prefix+key; lastTyped = now;
      const index = options().findIndex(o => enabled(o) && o.label.toLocaleLowerCase().startsWith(prefix.toLocaleLowerCase()));
      if (index >= 0) { active = index; highlight(); }
    }
  });
  listen(list,'pointerdown',event => {if(event.target.closest('[role=option]')) event.preventDefault();});
  listen(list,'click',event => { const row=event.target.closest('[role=option]'); if(row) choose(Number(row.dataset.index)); });
  listen(list,'pointermove',event => { const row=event.target.closest('[role=option]'); if(row && row.getAttribute('aria-disabled') !== 'true') {active=Number(row.dataset.index);highlight(false);} });
  listen(document,'pointerdown',event => { if(isOpen && !wrapper.contains(event.target)) close(); },true);
  listen(trigger,'blur',close);
  listen(window,'resize',position);
  listen(document,'scroll',event => {if(isOpen && !list.contains(event.target)) position();},true);
  listen(source,'change',sync);
  for(const label of source.labels) listen(label,'click',event => {if(!wrapper.contains(event.target)) {event.preventDefault();trigger.focus();}});
  listen(source,'invalid',event => {event.preventDefault(); trigger.focus(); trigger.setAttribute('aria-invalid','true');});
  if(source.form) listen(source.form,'reset',() => queueMicrotask(sync));
  for (const property of ['value','selectedIndex']) {
    const descriptor = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,property);
    Object.defineProperty(source,property,{configurable:true,get(){return descriptor.get.call(this);},set(value){descriptor.set.call(this,value);sync();}});
  }
  source.focus = options => trigger.focus(options);
  const controller = {sync,close,destroy() {
    close(); disposed=true; events.abort();
    delete source.value; delete source.selectedIndex; delete source.focus;
    source.classList.remove('select-source');
    if(previousTabIndex === null) source.removeAttribute('tabindex'); else source.setAttribute('tabindex',previousTabIndex);
    if(previousAriaHidden === null) source.removeAttribute('aria-hidden'); else source.setAttribute('aria-hidden',previousAriaHidden);
    if(wrapper.isConnected) wrapper.replaceWith(source);
  }};
  sync();
  if(wasFocused) trigger.focus();
  return controller;
}

// Adapt dynamic legacy forms in one place without changing their value/change contracts.
export function installSelects(root) {
  const controls = new Map();
  const scan = () => {
    for(const [source,control] of controls) if(!root.contains(source)) {control.destroy();controls.delete(source);}
    root.querySelectorAll('select').forEach(source => {if(!controls.has(source)) controls.set(source,Select(source));});
  };
  const observer = new MutationObserver(records => {
    if(records.some(r => r.type === 'childList')) scan();
    const changed = new Set();
    for(const record of records) {
      const node = record.target.nodeType === Node.ELEMENT_NODE ? record.target : record.target.parentElement;
      const source = node?.closest('select');
      if(source) changed.add(source);
      if(record.type === 'attributes' && ['disabled','aria-busy','open'].includes(record.attributeName)) node?.querySelectorAll('select').forEach(s => changed.add(s));
    }
    changed.forEach(source => controls.get(source)?.sync());
  });
  observer.observe(root,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:['disabled','selected','label','hidden','aria-label','aria-labelledby','aria-describedby','aria-invalid','aria-busy','open']});
  scan();
  return () => {observer.disconnect();controls.forEach(c=>c.destroy());};
}
