import { EditorView, WidgetType } from '@codemirror/view';
import type { VaultPath } from '../../api/index.ts';
import type { WikiHost } from '../wikilinks/host.ts';
import { previewInto } from './view.ts';

const cleanups = new WeakMap<HTMLElement, () => void>();
export class EmbedWidget extends WidgetType {
  readonly path: VaultPath;
  readonly target: string;
  readonly host: WikiHost;
  constructor(path: VaultPath, target: string, host: WikiHost) {
    super();
    this.path = path; this.target = target; this.host = host;
  }
  override eq(other: EmbedWidget): boolean {
    return this.host === other.host && this.target === other.target && this.path.length === other.path.length && this.path.every((p,i) => p === other.path[i]);
  }
  override toDOM(view: EditorView): HTMLElement {
    const node = document.createElement('span');
    node.dataset.rzEmbed = this.target;
    const load = () => previewInto(node,{ kind: 'target', from: this.path, target: this.target },
      (source,origin) => this.host.previewEmbed?.(source,origin), () => view.requestMeasure());
    let cancel: (() => void) | null = null;
    node.textContent = this.target || '[[]]';
    const observer = typeof IntersectionObserver === 'undefined' ? null : new IntersectionObserver(entries => {
      if (entries.some(e => e.isIntersecting)) { cancel ??= load(); }
      else if (cancel) {
        node.style.minHeight = node.offsetHeight + 'px';
        cancel(); cancel = null;
        node.textContent = this.target || '[[]]';
      }
    }, { root: view.scrollDOM, rootMargin: '64px' });
    if (observer) observer.observe(node);
    else cancel = load();
    const unlisten = this.host.subscribe(event => { if (event.kind === 'refresh' && cancel) { cancel(); cancel = load(); } });
    cleanups.set(node, () => { observer?.disconnect(); unlisten(); cancel?.(); });
    return node;
  }
  override destroy(node: HTMLElement) { cleanups.get(node)?.(); cleanups.delete(node); }
}
