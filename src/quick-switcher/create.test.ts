import { describe, expect, it } from 'vitest';
import { createOffer, hasFolderSeparator, newNoteDir, noteFileName } from './create.ts';

const items = [
  { name: 'Встреча', path: ['Проекты', 'Альфа', 'Встреча.md'] },
  { name: 'Встреча по запуску', path: ['Встреча по запуску.md'] },
];

describe('пункт «Создать заметку»', () => {
  it('пустой запрос — не предлагается', () => {
    expect(createOffer('', items)).toBeNull();
    expect(createOffer('   ', [])).toBeNull();
  });

  it('заметки с таким именем нет — предлагается имя без пробелов по краям', () => {
    expect(createOffer('  встр ', items)).toBe('встр');
    expect(createOffer('Новая', [])).toBe('Новая');
  });

  it('заметка с таким именем есть (без учёта регистра) — не предлагается', () => {
    expect(createOffer('встреча', items)).toBeNull();
    expect(createOffer('ВСТРЕЧА ПО ЗАПУСКУ', items)).toBeNull();
  });

  it('сравнение с нормализацией Unicode: составная и разложенная «й»', () => {
    expect(createOffer('Мой план', [{ name: 'Мой план', path: ['Мой план.md'] }])).toBeNull();
  });

  it('«.md» в конце запроса не удваивается', () => {
    expect(createOffer('Итоги.md', [])).toBe('Итоги');
    expect(createOffer('Встреча.MD', items)).toBeNull();
  });

  it('имя файла — с .md', () => {
    expect(noteFileName('встр')).toBe('встр.md');
  });
});

describe('косая черта в имени (решение 50)', () => {
  it('прямая и обратная косая черта — разделитель папок, имя с ней не создаётся', () => {
    expect(hasFolderSeparator('Проекты/План')).toBe(true);
    expect(hasFolderSeparator('А\\Б')).toBe(true);
    expect(hasFolderSeparator('План')).toBe(false);
  });
});

describe('папка новой заметки — по настройке «где создавать новые заметки»', () => {
  const current = ['Проекты', 'Альфа', 'Встреча.md'];

  it('корень хранилища', () => {
    expect(newNoteDir({ kind: 'vault_root' }, current)).toEqual([]);
  });

  it('папка открытой заметки; без открытой — корень', () => {
    expect(newNoteDir({ kind: 'current_folder' }, current)).toEqual(['Проекты', 'Альфа']);
    expect(newNoteDir({ kind: 'current_folder' }, null)).toEqual([]);
  });

  it('заданная папка', () => {
    expect(newNoteDir({ kind: 'folder', path: ['Входящие'] }, current)).toEqual(['Входящие']);
  });
});
