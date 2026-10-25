import { describe, expect, it } from 'vitest';
import type { TreeEntry } from '../api/index.ts';
import {
  applyChanges,
  buildTree,
  canMove,
  displayName,
  entryMap,
  flattenVisible,
  isInside,
  joinName,
  splitName,
  typeLabel,
  untitledName,
} from './model.ts';

const folder = (...path: string[]): TreeEntry => ({ kind: 'folder', path });
const note = (...path: string[]): TreeEntry => ({ kind: 'note', path });
const file = (...path: string[]): TreeEntry => ({ kind: 'attachment', path });

const names = (entries: TreeEntry[]) => buildTree(entryMap(entries)).map((node) => node.name);

describe('сортировка дерева (С3.2, С5.3)', () => {
  it('сначала папки, затем файлы; естественный порядок чисел', () => {
    expect(
      names([
        note('Заметка 10.md'),
        file('Заметка 3.png'),
        folder('Проекты'),
        note('Заметка 2.md'),
        folder('Архив 10'),
        folder('Архив 9'),
      ]),
    ).toEqual(['Архив 9', 'Архив 10', 'Проекты', 'Заметка 2.md', 'Заметка 3.png', 'Заметка 10.md']);
  });

  it('по-русски: «ё» рядом с «е», регистр не разводит имена', () => {
    // «ё» сравнивается как «е»: «ёлка» < «ель» (к < ь).
    expect(names([note('ель.md'), note('Яблоко.md'), note('ёлка.md'), note('Жук.md')])).toEqual([
      'ёлка.md',
      'ель.md',
      'Жук.md',
      'Яблоко.md',
    ]);
  });

  it('вложенные папки сортируются так же', () => {
    const [root] = buildTree(entryMap([folder('П'), note('П', 'б 10.md'), folder('П', 'я'), note('П', 'б 9.md')]));
    expect(root?.children.map((node) => node.name)).toEqual(['я', 'б 9.md', 'б 10.md']);
  });
});

describe('состав дерева', () => {
  it('все файлы видны: заметки, картинки, PDF', () => {
    expect(names([note('а.md'), file('б.png'), file('в.pdf'), file('г')])).toEqual(['а.md', 'б.png', 'в.pdf', 'г']);
  });

  it('скрыты .rhizomind и всё, что начинается с точки (и всё внутри скрытых папок)', () => {
    expect(
      names([folder('.rhizomind'), note('.rhizomind', 'x.md'), folder('.git'), note('.скрытая.md'), note('а.md')]),
    ).toEqual(['а.md']);
    const [root] = buildTree(entryMap([folder('П'), note('П', '.tmp.md'), note('П', 'б.md')]));
    expect(root?.children.map((node) => node.name)).toEqual(['б.md']);
  });

  it('файл без записи о папке: папка достраивается', () => {
    const [root] = buildTree(entryMap([note('П', 'В', 'з.md')]));
    expect(root).toMatchObject({ name: 'П', kind: 'folder' });
    expect(root?.children[0]?.children[0]?.name).toBe('з.md');
  });

  it('видимые строки: только содержимое раскрытых папок, с уровнем', () => {
    const roots = buildTree(entryMap([folder('А'), note('А', '1.md'), folder('А', 'Б'), note('А', 'Б', '2.md')]));
    expect(flattenVisible(roots, new Set()).map((row) => row.node.name)).toEqual(['А']);
    const rows = flattenVisible(roots, new Set(['А']));
    expect(rows.map((row) => [row.node.name, row.level])).toEqual([
      ['А', 1],
      ['Б', 2],
      ['1.md', 2],
    ]);
  });
});

describe('имена в дереве', () => {
  it('заметка — без .md; вложение — без расширения, с меткой типа; папка — как есть', () => {
    expect(displayName(note('П', 'Встреча.md'))).toBe('Встреча');
    expect(displayName(note('План.MD'))).toBe('План');
    expect(displayName(file('схема-процесса.png'))).toBe('схема-процесса');
    expect(typeLabel(file('схема-процесса.png'))).toBe('PNG');
    expect(typeLabel(file('ТЗ.v2.pdf'))).toBe('PDF');
    expect(displayName(file('Makefile'))).toBe('Makefile');
    expect(typeLabel(file('Makefile'))).toBeNull();
    expect(displayName(folder('Архив.2025'))).toBe('Архив.2025');
    expect(typeLabel(note('а.md'))).toBeNull();
  });

  it('при переименовании правится имя без расширения, расширение сохраняется', () => {
    expect(splitName(note('Встреча.md'))).toEqual({ stem: 'Встреча', ext: '.md' });
    expect(splitName(file('схема.png'))).toEqual({ stem: 'схема', ext: '.png' });
    expect(splitName(file('Makefile'))).toEqual({ stem: 'Makefile', ext: '' });
    expect(splitName(folder('Архив.2025'))).toEqual({ stem: 'Архив.2025', ext: '' });
    expect(joinName('Новое', '.md')).toBe('Новое.md');
  });

  it('«Без названия»: первое свободное имя без учёта регистра', () => {
    expect(untitledName('Без названия', '.md', [])).toBe('Без названия.md');
    expect(untitledName('Без названия', '.md', ['без названия.md', 'Без названия 1.md'])).toBe('Без названия 2.md');
    expect(untitledName('Без названия', '', ['Без названия'])).toBe('Без названия 1');
  });
});

describe('изменения из события vault:changed', () => {
  const base = entryMap([folder('П'), note('П', 'а.md'), note('б.md')]);

  it('создание, удаление (с содержимым папки), переименование (с содержимым)', () => {
    let result = applyChanges(base, [{ kind: 'created', path: ['П', 'в.png'], entry_kind: 'attachment' }]);
    expect(result.stale).toBe(false);
    expect(result.entries.get('П/в.png')).toEqual(file('П', 'в.png'));

    result = applyChanges(result.entries, [{ kind: 'renamed', from: ['П'], to: ['Архив', 'П'] }]);
    expect([...result.entries.keys()].sort()).toEqual(['Архив/П', 'Архив/П/а.md', 'Архив/П/в.png', 'б.md']);

    result = applyChanges(result.entries, [{ kind: 'removed', path: ['Архив', 'П'] }]);
    expect([...result.entries.keys()]).toEqual(['б.md']);
  });

  it('повтор уже применённого изменения ничего не меняет', () => {
    const once = applyChanges(base, [{ kind: 'renamed', from: ['б.md'], to: ['в.md'] }]);
    const twice = applyChanges(once.entries, [{ kind: 'renamed', from: ['б.md'], to: ['в.md'] }]);
    expect(twice.stale).toBe(false);
    expect([...twice.entries.keys()].sort()).toEqual(['П', 'П/а.md', 'в.md']);
    expect(applyChanges(base, [{ kind: 'removed', path: ['нет.md'] }]).stale).toBe(false);
    expect(applyChanges(base, [{ kind: 'modified', path: ['б.md'] }]).entries).toBe(base);
  });

  it('переименование неизвестного элемента — дерево надо перечитать', () => {
    expect(applyChanges(base, [{ kind: 'renamed', from: ['нет.md'], to: ['да.md'] }]).stale).toBe(true);
  });

  it('файл сменил расширение снаружи: заметка становится вложением и наоборот', () => {
    const result = applyChanges(base, [
      { kind: 'renamed', from: ['б.md'], to: ['б.txt'] },
      { kind: 'renamed', from: ['П', 'а.md'], to: ['П', 'а.MD'] },
    ]);
    expect(result.entries.get('б.txt')?.kind).toBe('attachment');
    expect(result.entries.get('П/а.MD')?.kind).toBe('note');
  });
});

describe('перемещение перетаскиванием', () => {
  it('нельзя в ту же папку, в себя и внутрь себя', () => {
    expect(canMove(['П', 'а.md'], ['П'])).toBe(false);
    expect(canMove(['а.md'], [])).toBe(false);
    expect(canMove(['П'], ['П'])).toBe(false);
    expect(canMove(['П'], ['П', 'В'])).toBe(false);
    expect(canMove(['П', 'а.md'], [])).toBe(true);
    expect(canMove(['П'], ['Пр'])).toBe(true);
  });

  it('isInside: сам путь и пути внутри, но не соседи с общим началом имени', () => {
    expect(isInside(['П', 'а.md'], ['П'])).toBe(true);
    expect(isInside(['П'], ['П'])).toBe(true);
    expect(isInside(['Пр', 'а.md'], ['П'])).toBe(false);
  });
});
