import path from 'node:path';

/** true, если target лежит внутри root (или совпадает с ним). Сравнение лексическое. */
export function isInside(root: string, target: string): boolean {
  const rel = path.relative(root, target);
  if (rel === '') return true;
  if (path.isAbsolute(rel)) return false; // другой диск в Windows
  return rel !== '..' && !rel.startsWith('..' + path.sep);
}

/**
 * Превращает путь заметки из API ("Папка/Заметка.md") в абсолютный путь внутри хранилища.
 * Отклоняет абсолютные пути, выход наружу через "..", нулевые байты и не-.md файлы.
 */
export function resolveNotePath(root: string, notePath: string): string {
  if (typeof notePath !== 'string' || notePath.length === 0 || notePath.length > 1024) {
    throw new Error('Некорректный путь заметки');
  }
  if (notePath.includes('\0')) throw new Error('Некорректный путь заметки');
  if (!notePath.toLowerCase().endsWith('.md')) throw new Error('Разрешены только .md-файлы');

  const parts = notePath.replace(/\\/g, '/').split('/');
  if (parts.some((p) => p === '..' || p === '')) throw new Error('Некорректный путь заметки');
  if (path.isAbsolute(notePath) || /^[a-zA-Z]:/.test(notePath)) {
    throw new Error('Абсолютные пути запрещены');
  }

  const abs = path.resolve(root, ...parts);
  if (!isInside(root, abs)) throw new Error('Путь вне хранилища');
  return abs;
}

/** Абсолютный путь внутри хранилища → путь заметки для API ("Папка/Заметка.md"). */
export function toNotePath(root: string, abs: string): string {
  return path.relative(root, abs).split(path.sep).join('/');
}
