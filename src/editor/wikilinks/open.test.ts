// Переход по вики-ссылке (С3.4): разрешённая — открыть заметку (и заголовок),
// неразрешённая — создать заметку в папке текущей или по пути из ссылки,
// создавая папки (решение 84). Недопустимая цель — ни одного вызова ядра:
// вики-ссылка не может выйти за хранилище и не уходит в open_external.
import { describe, expect, it } from 'vitest';
import type { TreeEntry, VaultPath } from '../../api/index.ts';
import { CoreCallError } from '../../api/index.ts';
import { followWikilink, type FollowDeps } from './open.ts';

interface Fake {
  deps: FollowDeps;
  calls: string[];
  entries: TreeEntry[];
}

function fake(resolved: Record<string, VaultPath | null> = {}, entries: TreeEntry[] = []): Fake {
  const calls: string[] = [];
  const state = { entries: [...entries] };
  const exists = (path: VaultPath) => state.entries.some((e) => e.path.join('/') === path.join('/'));
  const deps: FollowDeps = {
    resolve: async (from, target) => {
      calls.push(`resolve ${from.join('/')} → ${target}`);
      return resolved[target] ?? null;
    },
    treeList: async () => {
      calls.push('tree_list');
      return state.entries;
    },
    folderCreate: async (dir, name) => {
      calls.push(`folder_create ${[...dir, name].join('/')}`);
      const path = [...dir, name];
      if (exists(path)) throw new CoreCallError('folder_create', { code: 'already_exists', message_key: 'errors.already_exists' });
      state.entries.push({ path, kind: 'folder' });
      return path;
    },
    noteCreate: async (dir, name) => {
      calls.push(`note_create ${[...dir, name].join('/')}`);
      const path = [...dir, name];
      state.entries.push({ path, kind: 'note' });
      return path;
    },
  };
  return { deps, calls, entries: state.entries };
}

const FROM = ['Проекты', 'Дневник.md'];

describe('разрешённая ссылка', () => {
  it('открывается заметка, на которую указывает ядро', async () => {
    const { deps, calls } = fake({ 'Альфа/Встреча': ['Проекты', 'Альфа', 'Встреча.md'] });
    expect(await followWikilink('Альфа/Встреча|встречи', FROM, deps)).toEqual({
      path: ['Проекты', 'Альфа', 'Встреча.md'],
      heading: null,
      created: false,
    });
    expect(calls).toEqual(['resolve Проекты/Дневник.md → Альфа/Встреча']);
  });

  it('ссылка на заголовок — заголовок для перехода', async () => {
    const { deps } = fake({ Встреча: ['Встреча.md'] });
    expect(await followWikilink(' Встреча # Итоги ', FROM, deps)).toEqual({ path: ['Встреча.md'], heading: 'Итоги', created: false });
  });

  it('от корня — ключ с `/`', async () => {
    const { deps, calls } = fake({ '/Встреча': ['Встреча.md'] });
    expect((await followWikilink('/Встреча', FROM, deps))?.path).toEqual(['Встреча.md']);
    expect(calls).toEqual(['resolve Проекты/Дневник.md → /Встреча']);
  });
});

describe('неразрешённая ссылка создаёт заметку', () => {
  it('имя без пути — в папке заметки со ссылкой', async () => {
    const { deps, calls } = fake();
    expect(await followWikilink('Новая#Раздел', FROM, deps)).toEqual({ path: ['Проекты', 'Новая.md'], heading: null, created: true });
    expect(calls).toEqual(['resolve Проекты/Дневник.md → Новая', 'note_create Проекты/Новая.md']);
  });

  it('заметка в корне — в корне', async () => {
    const { deps } = fake();
    expect((await followWikilink('Новая', ['Дневник.md'], deps))?.path).toEqual(['Новая.md']);
  });

  it('с путём — путь от корня хранилища, недостающие папки создаются', async () => {
    const { deps, calls } = fake({}, [{ path: ['Работа'], kind: 'folder' }]);
    expect((await followWikilink('Работа/Встречи/Новая', FROM, deps))?.path).toEqual(['Работа', 'Встречи', 'Новая.md']);
    expect(calls).toEqual([
      'resolve Проекты/Дневник.md → Работа/Встречи/Новая',
      'tree_list',
      'folder_create Работа/Встречи',
      'note_create Работа/Встречи/Новая.md',
    ]);
  });

  it('папка уже есть в другом регистре — используется её имя на диске', async () => {
    const { deps, calls } = fake({}, [{ path: ['Работа'], kind: 'folder' }]);
    expect((await followWikilink('/работа/Новая', FROM, deps))?.path).toEqual(['Работа', 'Новая.md']);
    expect(calls).not.toContain('folder_create работа');
  });

  it('`.md` в конце не удваивается', async () => {
    const { deps } = fake();
    expect((await followWikilink('Новая.md', FROM, deps))?.path).toEqual(['Проекты', 'Новая.md']);
  });

  it('файл с именем папки из пути — ошибка ядра передаётся наверх, заметка не создаётся', async () => {
    const { deps, calls } = fake({}, [{ path: ['Работа'], kind: 'attachment' }]);
    await expect(followWikilink('Работа/Новая', FROM, deps)).rejects.toBeInstanceOf(CoreCallError);
    expect(calls.filter((c) => c.startsWith('note_create'))).toEqual([]);
  });
});

describe('недопустимая цель — ни одного вызова ядра', () => {
  it.each([
    ['якорь внутри заметки', '#Заголовок'],
    ['пустая', ''],
    ['выход из папки', '../Секрет'],
    ['выход из папки в середине', 'Альфа/../../Секрет'],
    ['текущая папка', './Встреча'],
    ['абсолютный путь Windows', 'C:\\Windows\\win.ini'],
    ['абсолютный путь с косой', 'C:/Windows/win'],
    ['длинный путь \\\\?\\', '\\\\?\\C:\\x'],
    ['UNC', '\\\\server\\share\\x'],
    ['UNC косыми', '//server/share/x'],
    ['адрес file:', 'file:///C:/x'],
    ['адрес https', 'https://example.org'],
    ['управляющий символ', 'a\u0000b'],
  ])('%s', async (_name, inner) => {
    const { deps, calls } = fake();
    expect(await followWikilink(inner, FROM, deps)).toBeNull();
    expect(calls).toEqual([]);
  });
});
