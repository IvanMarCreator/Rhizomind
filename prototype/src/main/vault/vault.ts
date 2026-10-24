import { promises as fs } from 'node:fs';
import path from 'node:path';
import type { NoteRef, VaultInfo } from '../../shared/api';
import { isInside, resolveNotePath, toNotePath } from './safe-path';

const MAX_NOTE_BYTES = 10 * 1024 * 1024;

/**
 * Хранилище — обычная папка с .md-файлами.
 * Все операции ограничены этой папкой: символические ссылки не открываются,
 * служебные папки (".git", ".rhizomind" и любые с точкой) не сканируются.
 */
export class Vault {
  private constructor(readonly root: string) {}

  static async open(dir: string): Promise<Vault> {
    const real = await fs.realpath(dir);
    const stat = await fs.stat(real);
    if (!stat.isDirectory()) throw new Error('Хранилище должно быть папкой');
    return new Vault(real);
  }

  get name(): string {
    return path.basename(this.root);
  }

  async info(): Promise<VaultInfo> {
    return { name: this.name, noteCount: (await this.listNotes()).length };
  }

  async listNotes(): Promise<NoteRef[]> {
    const notes: NoteRef[] = [];
    await this.walk(this.root, notes);
    return notes.sort((a, b) => a.path.localeCompare(b.path));
  }

  async readNote(notePath: string): Promise<string> {
    const abs = await this.resolveExisting(notePath);
    const stat = await fs.stat(abs);
    if (stat.size > MAX_NOTE_BYTES) throw new Error('Заметка слишком большая');
    return fs.readFile(abs, 'utf8');
  }

  async writeNote(notePath: string, content: string): Promise<void> {
    if (typeof content !== 'string') throw new Error('Некорректное содержимое');
    if (Buffer.byteLength(content, 'utf8') > MAX_NOTE_BYTES) throw new Error('Заметка слишком большая');
    const abs = await this.resolveForWrite(notePath);
    await fs.writeFile(abs, content, 'utf8');
  }

  async createNote(title: string): Promise<NoteRef> {
    const base = sanitizeTitle(title);
    for (let i = 0; i < 1000; i++) {
      const name = i === 0 ? `${base}.md` : `${base} ${i}.md`;
      const abs = path.join(this.root, name);
      try {
        // флаг "wx" не перезапишет существующий файл
        await fs.writeFile(abs, `# ${base}\n`, { encoding: 'utf8', flag: 'wx' });
        return toRef(this.root, abs);
      } catch (e) {
        if ((e as NodeJS.ErrnoException).code !== 'EEXIST') throw e;
      }
    }
    throw new Error('Не удалось подобрать имя заметки');
  }

  private async walk(dir: string, out: NoteRef[]): Promise<void> {
    const entries = await fs.readdir(dir, { withFileTypes: true });
    for (const entry of entries) {
      if (entry.name.startsWith('.')) continue;
      if (entry.isSymbolicLink()) continue;
      const abs = path.join(dir, entry.name);
      if (entry.isDirectory()) await this.walk(abs, out);
      else if (entry.isFile() && entry.name.toLowerCase().endsWith('.md')) out.push(toRef(this.root, abs));
    }
  }

  /** Путь существующей заметки; после разрешения симлинков он всё ещё внутри хранилища. */
  private async resolveExisting(notePath: string): Promise<string> {
    const abs = resolveNotePath(this.root, notePath);
    const real = await fs.realpath(abs);
    if (!isInside(this.root, real)) throw new Error('Путь вне хранилища');
    return real;
  }

  /** Путь для записи: родительская папка должна существовать и лежать внутри хранилища. */
  private async resolveForWrite(notePath: string): Promise<string> {
    const abs = resolveNotePath(this.root, notePath);
    const parent = await fs.realpath(path.dirname(abs));
    if (!isInside(this.root, parent)) throw new Error('Путь вне хранилища');
    const target = path.join(parent, path.basename(abs));
    try {
      const stat = await fs.lstat(target);
      if (stat.isSymbolicLink()) throw new Error('Запись в символические ссылки запрещена');
    } catch (e) {
      if ((e as NodeJS.ErrnoException).code !== 'ENOENT') throw e;
    }
    return target;
  }
}

function toRef(root: string, abs: string): NoteRef {
  return { path: toNotePath(root, abs), title: path.basename(abs).replace(/\.md$/i, '') };
}

/** Убирает символы, запрещённые в именах файлов на Windows, macOS и Linux. */
export function sanitizeTitle(title: string): string {
  const clean = String(title)
    .replace(/[<>:"/\\|?*#^[\]\u0000-\u001f]/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .replace(/[. ]+$/, '')
    .slice(0, 120);
  if (!clean || /^(con|prn|aux|nul|com\d|lpt\d)$/i.test(clean)) return 'Без названия';
  return clean;
}
