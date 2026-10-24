import type { NoteRef } from '../shared/api';
import { parseWikiLinks } from '../core/wikilinks';

/*
 * Интерфейс v0.1: список заметок, редактор, ссылки и обратные ссылки.
 * Вся работа с данными идёт через window.rhizomind (src/shared/api.ts).
 * Пользовательский текст вставляется только через textContent, без innerHTML.
 */
const api = window.rhizomind;

const el = {
  edition: byId('edition'),
  openVault: byId<HTMLButtonElement>('open-vault'),
  vaultName: byId('vault-name'),
  newNoteForm: byId<HTMLFormElement>('new-note-form'),
  newNoteTitle: byId<HTMLInputElement>('new-note-title'),
  noteList: byId<HTMLUListElement>('note-list'),
  noteTitle: byId('note-title'),
  saveStatus: byId('save-status'),
  editor: byId<HTMLTextAreaElement>('editor'),
  outgoing: byId<HTMLUListElement>('outgoing'),
  backlinks: byId<HTMLUListElement>('backlinks'),
};

let notes: NoteRef[] = [];
let current: NoteRef | null = null;
let saveTimer: number | undefined;

el.edition.textContent = api.edition === 'secure' ? 'Secure' : api.edition;

el.openVault.addEventListener('click', async () => {
  const info = await api.vault.open();
  if (!info) return;
  el.vaultName.textContent = `${info.name} · заметок: ${info.noteCount}`;
  el.newNoteForm.hidden = false;
  current = null;
  showNote(null, '');
  await refreshList();
});

el.newNoteForm.addEventListener('submit', async (event) => {
  event.preventDefault();
  const title = el.newNoteTitle.value.trim();
  if (!title) return;
  el.newNoteTitle.value = '';
  const ref = await api.vault.createNote(title);
  await refreshList();
  await openNote(ref);
});

el.editor.addEventListener('input', () => {
  if (!current) return;
  el.saveStatus.textContent = 'Изменено';
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => void save(), 500);
});

async function save(): Promise<void> {
  if (!current) return;
  try {
    await api.vault.writeNote(current.path, el.editor.value);
    el.saveStatus.textContent = 'Сохранено';
    await renderOutgoing(el.editor.value);
  } catch (e) {
    el.saveStatus.textContent = `Ошибка сохранения: ${(e as Error).message}`;
  }
}

async function refreshList(): Promise<void> {
  notes = await api.vault.listNotes();
  el.noteList.replaceChildren(
    ...notes.map((note) => {
      const li = document.createElement('li');
      li.textContent = note.path.replace(/\.md$/i, '');
      li.title = note.path;
      li.classList.toggle('active', note.path === current?.path);
      li.addEventListener('click', () => void openNote(note));
      return li;
    }),
  );
}

async function openNote(note: NoteRef): Promise<void> {
  if (current) {
    window.clearTimeout(saveTimer);
    await save();
  }
  const text = await api.vault.readNote(note.path);
  current = note;
  showNote(note, text);
  await refreshList();
  await Promise.all([renderOutgoing(text), renderBacklinks(note)]);
}

function showNote(note: NoteRef | null, text: string): void {
  el.noteTitle.textContent = note?.title ?? '';
  el.saveStatus.textContent = '';
  el.editor.value = text;
  el.editor.disabled = !note;
  if (!note) {
    el.outgoing.replaceChildren();
    el.backlinks.replaceChildren();
  }
}

async function renderOutgoing(text: string): Promise<void> {
  const seen = new Set<string>();
  const items: HTMLLIElement[] = [];
  for (const link of parseWikiLinks(text)) {
    const key = link.target.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    const target = await api.links.resolve(link.target);
    items.push(
      linkItem(link.alias ?? link.target, target ? () => void openNote(target) : () => void createFromLink(link.target), !target),
    );
  }
  el.outgoing.replaceChildren(...(items.length ? items : [emptyItem('Нет ссылок')]));
}

async function renderBacklinks(note: NoteRef): Promise<void> {
  const refs = await api.links.backlinks(note.path);
  el.backlinks.replaceChildren(
    ...(refs.length ? refs.map((r) => linkItem(r.title, () => void openNote(r), false)) : [emptyItem('Нет обратных ссылок')]),
  );
}

/** Клик по ссылке на несуществующую заметку создаёт её, как в Obsidian. */
async function createFromLink(target: string): Promise<void> {
  const ref = await api.vault.createNote(target.split('/').pop() ?? target);
  await refreshList();
  await openNote(ref);
}

function linkItem(label: string, onClick: () => void, unresolved: boolean): HTMLLIElement {
  const li = document.createElement('li');
  li.textContent = label;
  li.classList.toggle('unresolved', unresolved);
  if (unresolved) li.title = 'Заметки нет. Нажмите, чтобы создать';
  li.addEventListener('click', onClick);
  return li;
}

function emptyItem(text: string): HTMLLIElement {
  const li = document.createElement('li');
  li.textContent = text;
  li.className = 'empty';
  return li;
}

function byId<T extends HTMLElement = HTMLElement>(id: string): T {
  const node = document.getElementById(id);
  if (!node) throw new Error(`Нет элемента #${id}`);
  return node as T;
}
