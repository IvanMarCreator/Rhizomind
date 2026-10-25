import { describe, expect, it } from 'vitest';
import { NavigationHistory, historyKeyAction } from './history.ts';

const a = ['Входящие', 'А.md'];
const b = ['Проекты', 'Б.md'];
const c = ['В.md'];

describe('история переходов', () => {
  it('пустая история: назад и вперёд недоступны', () => {
    const history = new NavigationHistory();

    expect(history.current).toBeNull();
    expect(history.canGoBack).toBe(false);
    expect(history.canGoForward).toBe(false);
    expect(history.back()).toBeNull();
    expect(history.forward()).toBeNull();
  });

  it('назад и вперёд проходят по открытым заметкам', () => {
    const history = new NavigationHistory();
    history.visit(a);
    history.visit(b);
    history.visit(c);

    expect(history.back()).toEqual(b);
    expect(history.back()).toEqual(a);
    expect(history.canGoBack).toBe(false);
    expect(history.back()).toBeNull();
    expect(history.current).toEqual(a);

    expect(history.forward()).toEqual(b);
    expect(history.forward()).toEqual(c);
    expect(history.canGoForward).toBe(false);
  });

  it('новый переход после «назад» отбрасывает путь вперёд', () => {
    const history = new NavigationHistory();
    history.visit(a);
    history.visit(b);
    history.back();

    history.visit(c);

    expect(history.canGoForward).toBe(false);
    expect(history.back()).toEqual(a);
  });

  it('повторное открытие той же заметки не добавляет шаг', () => {
    const history = new NavigationHistory();
    history.visit(a);
    history.visit(['Входящие', 'А.md']);

    expect(history.canGoBack).toBe(false);
  });

  it('хранит не больше заданного числа шагов, отбрасывая самые старые', () => {
    const history = new NavigationHistory(3);
    for (const name of ['1.md', '2.md', '3.md', '4.md']) history.visit([name]);

    expect(history.back()).toEqual(['3.md']);
    expect(history.back()).toEqual(['2.md']);
    expect(history.back()).toBeNull();
  });

  it('переименование заметки или папки меняет пути в истории', () => {
    const history = new NavigationHistory();
    history.visit(a);
    history.visit(b);

    history.rename(['Входящие'], ['Разобрать']);

    expect(history.back()).toEqual(['Разобрать', 'А.md']);
    expect(history.forward()).toEqual(b);
  });

  it('удалённая заметка пропадает из истории, соседние одинаковые шаги склеиваются', () => {
    const history = new NavigationHistory();
    history.visit(a);
    history.visit(b);
    history.visit(a);
    history.visit(c);

    history.remove(b);

    expect(history.current).toEqual(c);
    expect(history.back()).toEqual(a);
    expect(history.back()).toBeNull();
  });

  it('удаление текущей заметки делает текущей предыдущую', () => {
    const history = new NavigationHistory();
    history.visit(a);
    history.visit(b);

    history.remove(b);

    expect(history.current).toEqual(a);
    expect(history.canGoForward).toBe(false);
  });

  it('удаление папки убирает все заметки внутри', () => {
    const history = new NavigationHistory();
    history.visit(a);
    history.visit(c);

    history.remove(['Входящие']);

    expect(history.current).toEqual(c);
    expect(history.canGoBack).toBe(false);
  });
});

describe('горячие клавиши истории', () => {
  const key = (init: Partial<KeyboardEventInit> & { key: string }) => ({
    key: init.key,
    altKey: init.altKey ?? false,
    ctrlKey: init.ctrlKey ?? false,
    metaKey: init.metaKey ?? false,
    shiftKey: init.shiftKey ?? false,
  });

  it('Alt+← — назад, Alt+→ — вперёд', () => {
    expect(historyKeyAction(key({ key: 'ArrowLeft', altKey: true }))).toBe('back');
    expect(historyKeyAction(key({ key: 'ArrowRight', altKey: true }))).toBe('forward');
  });

  it('стрелки без Alt и с другими модификаторами не трогает', () => {
    expect(historyKeyAction(key({ key: 'ArrowLeft' }))).toBeNull();
    expect(historyKeyAction(key({ key: 'ArrowLeft', altKey: true, shiftKey: true }))).toBeNull();
    expect(historyKeyAction(key({ key: 'ArrowRight', altKey: true, ctrlKey: true }))).toBeNull();
    expect(historyKeyAction(key({ key: 'a', altKey: true }))).toBeNull();
  });
});
