import { describe, expect, it, vi } from 'vitest';
import { finishAttachment, pasteContainsAttachment, pastedExternalPath } from './attachment.ts';

describe('attachment — границы ввода и исходного документа', () => {
  it('оставляет простой текст штатной вставке и распознаёт список файлов без доступа к файлам', () => {
    expect(pasteContainsAttachment(['text/plain', 'text/html'])).toBe(false);
    expect(pasteContainsAttachment(['Files', 'text/plain'])).toBe(true);
    expect(pasteContainsAttachment(['text/uri-list'])).toBe(true);
    expect(pasteContainsAttachment(['image/png'])).toBe(true);
  });

  it('выделяет целый абсолютный путь из текста и оставляет обычную фразу как есть', () => {
    for (const path of ['"C:\\work\\photo.png"', '\\\\server\\share\\photo.png', 'file:///tmp/file', '/tmp/file']) {
      expect(pastedExternalPath(path)).toBe(true);
    }
    expect(pastedExternalPath('Текст /tmp/пример в заметке')).toBe(false);
    expect(pastedExternalPath('/строка\nвторая строка')).toBe(false);
  });

  it('после закрытия исходного документа показывает путь уже записанной копии', () => {
    const insert = vi.fn(() => false);
    const notify = vi.fn();
    finishAttachment({ outcome: 'created', path: ['Работа', 'Фото.png'], target: 'Фото.png' }, insert, notify);
    expect(insert).toHaveBeenCalledWith('![[Фото.png]]');
    expect(notify.mock.calls[0]?.[0]).toContain('Работа/Фото.png');
  });
});
