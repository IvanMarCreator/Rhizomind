// Командная строка помощников выпуска (задача 3.4, .github/workflows/release.yml):
//   node tools/release/cli.ts version [--tag v0.1.0 | v0.1.0-rc.N]
//       версия проекта (одна в package.json, tauri.conf.json, Cargo.toml);
//       с --tag — ещё и совпадение с тегом. Печатает «версия кандидат(true|false)».
//   node tools/release/cli.ts notes <версия> <файл> [--require-date] [--candidate]
//       описание выпуска из раздела CHANGELOG.md; --require-date — для выпуска
//       по тегу v0.1.0: дата не может быть заглушкой ГГГГ-ММ-ДД; --candidate —
//       первой строкой «Кандидат для ручного аудита, не для распространения».
//   node tools/release/cli.ts checksums <папка>
//       SHA256SUMS.txt по всем файлам папки (кроме самого SHA256SUMS.txt).
import { readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { CANDIDATE_NOTE, checksums, parseTag, projectVersions, releaseNotes } from './release.ts';

const root = path.join(import.meta.dirname, '..', '..');
const [command, ...args] = process.argv.slice(2);

function option(name: string): string | undefined {
  const index = args.indexOf(name);
  return index === -1 ? undefined : args[index + 1];
}

try {
  if (command === 'version') {
    const { version } = projectVersions(root);
    const tag = option('--tag');
    const candidate = tag === undefined ? false : parseTag(tag).candidate;
    if (tag !== undefined && parseTag(tag).version !== version) {
      throw new Error(`тег ${tag} не совпадает с версией проекта ${version}`);
    }
    console.log(`${version} ${candidate}`);
  } else if (command === 'notes') {
    const [version, out] = args;
    if (version === undefined || out === undefined) throw new Error('notes <версия> <файл> [--require-date] [--candidate]');
    const changelog = readFileSync(path.join(root, 'CHANGELOG.md'), 'utf8');
    const notes = releaseNotes(changelog, version, { requireDate: args.includes('--require-date') });
    writeFileSync(out, args.includes('--candidate') ? `**${CANDIDATE_NOTE}**\n\n${notes}` : notes);
    console.log(`описание выпуска ${version} → ${out}`);
  } else if (command === 'checksums') {
    const [dir] = args;
    if (dir === undefined) throw new Error('checksums <папка>');
    const files = readdirSync(dir).filter((name) => name !== 'SHA256SUMS.txt' && statSync(path.join(dir, name)).isFile());
    if (files.length === 0) throw new Error(`в ${dir} нет файлов`);
    const sums = checksums(dir, files);
    writeFileSync(path.join(dir, 'SHA256SUMS.txt'), sums);
    process.stdout.write(sums);
  } else {
    throw new Error('команда: version | notes | checksums');
  }
} catch (error) {
  console.error(`release: ${error instanceof Error ? error.message : String(error)}`);
  process.exit(1);
}
