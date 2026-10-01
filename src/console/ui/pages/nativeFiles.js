import {NativeFiles} from '../features/nativeFiles.js';
export function mount(container){
  const root=document.createElement('div');root.className='native-files-page';container.append(root);
  root.innerHTML='<header class="page-head"><div><h1>全局 Rules 与 Agent</h1></div></header>';
  const panel=NativeFiles(root);
  return {isDirty:()=>panel.isDirty(),destroy(){panel.destroy();root.remove();}};
}
