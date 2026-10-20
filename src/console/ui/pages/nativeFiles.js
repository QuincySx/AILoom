import {NativeFiles} from '../features/nativeFiles.js';
import {GlobalSkills} from '../features/globalSkills.js';
import {libraryNavigation} from '../components/sectionNav.js';

export function mount(container){
  const files=location.hash==='#/library/rules';
  const root=document.createElement('div');root.className='native-files-page';container.append(root);
  root.innerHTML=libraryNavigation(files?'files':'global')+'<div data-content></div>';
  const panel=files?NativeFiles(root.querySelector('[data-content]')):GlobalSkills(root.querySelector('[data-content]'));
  return {isDirty:()=>Boolean(panel?.isDirty?.()),destroy(){panel?.destroy();root.remove();}};
}
