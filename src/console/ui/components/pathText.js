// Keep the useful filename visible; reveal the full path on hover or keyboard focus.
class PathText extends HTMLElement {
  constructor() {
    super();
    const shadow = this.attachShadow({mode:'open'});
    shadow.innerHTML = `<style>
      :host { display:block; min-width:0; max-width:100%; }
      .viewport { display:block; overflow:hidden; white-space:nowrap; text-overflow:ellipsis; direction:rtl; text-align:left; }
      .text { direction:ltr; unicode-bidi:isolate; }
      :host([revealing]) .viewport { direction:ltr; text-overflow:clip; }
      :host([revealing]) .text { display:inline-block; }
      :host(:focus-visible) { outline:2px solid currentColor; outline-offset:2px; }
    </style><span class="viewport"><span class="text"><slot></slot></span></span>`;
    this.onmouseenter = () => this.reveal();
    this.onmouseleave = () => { if(!this.matches(':focus')) this.reset(); };
    this.onfocus = () => this.reveal();
    this.onblur = () => { if(!this.matches(':hover')) this.reset(); };
  }
  connectedCallback() { if(!this.hasAttribute('tabindex')) this.tabIndex=0; }
  disconnectedCallback() { this.reset(); }
  reveal() {
    this.reset();
    this.setAttribute('revealing','');
    const text=this.shadowRoot.querySelector('.text');
    const distance=Math.max(0,text.getBoundingClientRect().width-this.clientWidth);
    if(!distance || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    this.animation=text.animate([
      {transform:'translateX(0)'},
      {transform:`translateX(-${distance}px)`},
    ],{duration:Math.max(1000,distance/45*1000),delay:800,fill:'both',easing:'linear'});
  }
  reset() { this.animation?.cancel(); this.animation=null; this.removeAttribute('revealing'); }
}
customElements.define('ailoom-path',PathText);
