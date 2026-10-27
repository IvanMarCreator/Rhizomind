// Строки длиннее порога не переносятся посреди слова (решение 83, задача 2.10).
//
// Перенос длинных слов (`overflow-wrap: anywhere`, codemirror-base.css) на
// WebKitGTK раскладывает строку в мегабайты без пробелов дольше минуты — окно
// не отвечает (файл больше лимита). Таким строкам — класс `cm-rz-hugeLine`
// с обычным переносом. Смотрятся только видимые строки.
import { RangeSetBuilder } from '@codemirror/state';
import { Decoration, ViewPlugin, type DecorationSet, type EditorView, type ViewUpdate } from '@codemirror/view';

/** Порог длины строки в символах. */
export const HUGE_LINE = 10_000;

const hugeLine = Decoration.line({ class: 'cm-rz-hugeLine' });

function build(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  let last = -1;
  for (const { from, to } of view.visibleRanges) {
    for (let pos = from; pos <= to; ) {
      const line = view.state.doc.lineAt(pos);
      if (line.from > last && line.length > HUGE_LINE) builder.add(line.from, line.from, hugeLine);
      last = line.from;
      pos = line.to + 1;
    }
  }
  return builder.finish();
}

export const hugeLines = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = build(view);
    }

    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) this.decorations = build(update.view);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);
