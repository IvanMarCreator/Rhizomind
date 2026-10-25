// Подменное хранилище для компонентных тестов дерева: команды файлов ядра
// (tree_list, note_create, folder_create, entry_rename, entry_move,
// entry_trash, name_check) над списком в памяти. Только для тестов: в
// приложение не импортируется. Правила имён здесь упрощены — настоящие в ядре.
import type { CoreError, EntryKind, NameCheck, NameProblem, TreeEntry, TrashOutcome, VaultPath } from '../api/index.ts';
import type { CoreMocks } from '../api/testing.ts';

const key = (path: VaultPath) => path.join('/');
const lower = (name: string) => name.normalize('NFC').toLocaleLowerCase('ru');
const inside = (path: VaultPath, prefix: VaultPath) =>
  prefix.length <= path.length && prefix.every((part, i) => part === path[i]);

function nameError(problem: NameProblem): CoreError {
  return { code: 'invalid_name', message_key: 'errors.invalid_name', name_problem: problem };
}

export class FakeVault {
  entries = new Map<string, TreeEntry>();
  /** Ответ `entry_trash` (по умолчанию — в корзину). */
  trashOutcome: TrashOutcome = { outcome: 'trashed' };

  constructor(entries: TreeEntry[]) {
    for (const entry of entries) this.entries.set(key(entry.path), entry);
  }

  has(path: VaultPath): boolean {
    return this.entries.has(key(path));
  }

  #siblingTaken(dir: VaultPath, name: string, except?: VaultPath | null): boolean {
    return [...this.entries.values()].some(
      (entry) =>
        entry.path.length === dir.length + 1 &&
        inside(entry.path, dir) &&
        lower(entry.path.at(-1) ?? '') === lower(name) &&
        !(except && key(except) === key(entry.path)),
    );
  }

  check(dir: VaultPath, name: string, renaming?: VaultPath | null): NameCheck {
    const reject = (problem: NameProblem): NameCheck => ({ status: 'rejected', problem, message_key: `names.${problem}` });
    if (name.trim() === '' || /^\.md$/i.test(name)) return reject('empty');
    if (/[\\/:*?"<>|]/.test(name)) return reject('forbidden_chars');
    if (/^(con|prn|aux|nul|com\d|lpt\d)(\.|$)/i.test(name)) return reject('reserved_name');
    if (/[. ]$/.test(name)) return reject('trailing_dot_or_space');
    if (this.#siblingTaken(dir, name, renaming)) return reject('duplicate');
    if (name.startsWith('.')) return { status: 'ok', warning: 'hidden' };
    if (/^[#[\] ]|[#[\]]/.test(name)) return { status: 'ok', warning: 'unlinkable' };
    return { status: 'ok', warning: null };
  }

  #create(dir: VaultPath, name: string, kind: EntryKind): VaultPath {
    const result = this.check(dir, name);
    if (result.status === 'rejected') throw nameError(result.problem);
    const path = [...dir, name];
    this.entries.set(key(path), { kind, path });
    return path;
  }

  #relocate(from: VaultPath, to: VaultPath): void {
    for (const entry of [...this.entries.values()]) {
      if (!inside(entry.path, from)) continue;
      this.entries.delete(key(entry.path));
      const path = [...to, ...entry.path.slice(from.length)];
      this.entries.set(key(path), { ...entry, path });
    }
  }

  mocks(): CoreMocks {
    return {
      tree_list: () => ({ entries: [...this.entries.values()] }),
      name_check: ({ dir, name, renaming }) => this.check(dir, name, renaming),
      note_create: ({ dir, name }) => ({ path: this.#create(dir, name, 'note') }),
      folder_create: ({ dir, name }) => ({ path: this.#create(dir, name, 'folder') }),
      entry_rename: ({ path, new_name }) => {
        if (!this.has(path)) throw { code: 'not_found', message_key: 'errors.not_found' } satisfies CoreError;
        const dir = path.slice(0, -1);
        const result = this.check(dir, new_name, path);
        if (result.status === 'rejected') throw nameError(result.problem);
        const to = [...dir, new_name];
        this.#relocate(path, to);
        return { path: to };
      },
      entry_move: ({ path, new_dir }) => {
        const name = path.at(-1) ?? '';
        if (this.#siblingTaken(new_dir, name)) {
          throw { code: 'already_exists', message_key: 'errors.already_exists' } satisfies CoreError;
        }
        const to = [...new_dir, name];
        this.#relocate(path, to);
        return { path: to };
      },
      entry_trash: ({ path }) => {
        if (this.trashOutcome.outcome !== 'cancelled') {
          for (const entry of [...this.entries.values()]) {
            if (inside(entry.path, path)) this.entries.delete(key(entry.path));
          }
        }
        return this.trashOutcome;
      },
    };
  }
}
