// Помощники выпуска (задача 3.4, .github/workflows/release.yml): номер версии,
// описание выпуска из CHANGELOG.md, контрольные суммы SHA-256. Без сети.
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import path from 'node:path';

/** Дата в заголовке раздела, пока выпуск не помечен тегом (заполняет оркестратор). */
export const DATE_PLACEHOLDER = 'ГГГГ-ММ-ДД';

const VERSION = /^\d+\.\d+\.\d+$/;

/** Строка, которой начинается описание кандидата (тег `v0.1.0-rc.N`). */
export const CANDIDATE_NOTE = 'Кандидат для ручного аудита, не для распространения.';

export interface ReleaseTag {
  /** Версия проекта: `0.1.0` и для `v0.1.0`, и для `v0.1.0-rc.2`. */
  readonly version: string;
  /** Кандидат `-rc.N`: черновик с пометкой «пререлиз». */
  readonly candidate: boolean;
}

/** `v0.1.0` → выпуск 0.1.0; `v0.1.0-rc.2` → кандидат 0.1.0. Другое — ошибка. */
export function parseTag(tag: string): ReleaseTag {
  const match = /^v(\d+\.\d+\.\d+)(-rc\.[1-9]\d*)?$/.exec(tag);
  if (!match?.[1]) throw new Error(`тег выпуска «${tag}» должен быть вида v0.1.0 или v0.1.0-rc.1`);
  return { version: match[1], candidate: match[2] !== undefined };
}

/** `version` из таблицы `[package]` (до следующей таблицы). */
function cargoPackageVersion(manifest: string): string {
  const text = manifest.replaceAll('\r\n', '\n');
  const start = text.indexOf('[package]\n');
  if (start === -1) return '(нет)';
  const next = text.indexOf('\n[', start + 1);
  const table = text.slice(start, next === -1 ? undefined : next);
  return /^version\s*=\s*"([^"]+)"/m.exec(table)?.[1] ?? '(нет)';
}

/** Версия из package.json, src-tauri/tauri.conf.json и src-tauri/Cargo.toml — одна на всех. */
export function projectVersions(root: string): { version: string } {
  const found: Record<string, string> = {
    'package.json': (JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')) as { version: string }).version,
    'src-tauri/tauri.conf.json': (
      JSON.parse(readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')) as { version: string }
    ).version,
    'src-tauri/Cargo.toml': cargoPackageVersion(readFileSync(path.join(root, 'src-tauri', 'Cargo.toml'), 'utf8')),
  };
  const values = new Set(Object.values(found));
  const [version] = values;
  if (values.size !== 1 || version === undefined || !VERSION.test(version)) {
    throw new Error(`версии в файлах проекта расходятся: ${Object.entries(found).map(([f, v]) => `${f}: ${v}`).join(', ')}`);
  }
  return { version };
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/**
 * Текст раздела `## [<версия>] — <дата>` без заголовка: до следующего раздела
 * `## ` или до ссылок `[x]: …` в конце файла. `requireDate` — для выпуска по
 * тегу: дата должна быть настоящей, не заглушкой.
 */
export function releaseNotes(changelog: string, version: string, { requireDate }: { requireDate: boolean }): string {
  const text = changelog.replaceAll('\r\n', '\n');
  const heading = new RegExp(`^## \\[${escapeRegExp(version)}\\] — (\\S+)[^\\n]*\\n`, 'm').exec(text);
  if (!heading) throw new Error(`в CHANGELOG.md нет раздела «## [${version}] — <дата>»`);
  const date = heading[1] ?? '';
  if (requireDate && !/^\d{4}-\d{2}-\d{2}$/.test(date)) {
    throw new Error(`у раздела ${version} в CHANGELOG.md дата «${date}» — поставьте дату выпуска перед тегом`);
  }
  const rest = text.slice(heading.index + heading[0].length);
  const end = /^(## |\[[^\]]+\]: )/m.exec(rest);
  const body = (end ? rest.slice(0, end.index) : rest).trim();
  if (body === '') throw new Error(`раздел ${version} в CHANGELOG.md пуст`);
  return `${body}\n`;
}

/** Содержимое SHA256SUMS.txt (формат `sha256sum`: «хэш, два пробела, имя»), по имени файла. */
export function checksums(dir: string, files: readonly string[]): string {
  return [...files]
    .sort()
    .map((name) => `${createHash('sha256').update(readFileSync(path.join(dir, name))).digest('hex')}  ${name}\n`)
    .join('');
}
