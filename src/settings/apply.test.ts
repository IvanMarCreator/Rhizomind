import { describe, expect, it } from 'vitest';
import { applyTextSettings, type StyleRoot } from './apply.ts';

function fakeRoot(): StyleRoot & { values: Map<string, string> } {
  const values = new Map<string, string>();
  return {
    values,
    style: {
      setProperty: (name: string, value: string | null) => void values.set(name, value ?? ''),
      removeProperty: (name: string) => {
        values.delete(name);
        return '';
      },
    },
  };
}

describe('ширина текста и размер шрифта', () => {
  it('во всю ширину снимает ограничение колонки, колонка возвращает его', () => {
    const root = fakeRoot();
    applyTextSettings({ text_width: 'full', font_size: 20 }, root);
    expect(root.values.get('--note-column-width')).toBe('none');
    expect(root.values.get('--note-font-size')).toBe('20px');

    applyTextSettings({ text_width: 'column', font_size: 16 }, root);
    expect(root.values.has('--note-column-width')).toBe(false);
    expect(root.values.get('--note-font-size')).toBe('16px');
  });
});
