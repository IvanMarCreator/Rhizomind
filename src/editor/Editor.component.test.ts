// Редактор в окне (С3.3, С3.6) на подмене ядра. Подменное ядро ведёт
// «диск» байтами и делит работу с редактором так же, как настоящее
// (src-tauri/src/commands/note.rs): чтение убирает BOM и приводит `\r\n` к
// `\n`, запись возвращает BOM и преобладающие переносы файла и не пишет
// файл, если текст не изменился.
import { EditorView } from '@codemirror/view';
import { clearMocks } from '@tauri-apps/api/mocks';
import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { tick } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import type { CoreError, FileVersion, NoteReadResponse, NoteWriteResponse, Settings, VaultPath } from '../api/index.ts';
import { emitCoreEvent, mockCore, type CoreMocks, type RecordedCall } from '../api/testing.ts';
import Shell from '../shell/Shell.svelte';
import Editor from './Editor.svelte';
import { WindowModel } from '../shell/window.svelte.ts';

// ---- Подменный диск ---------------------------------------------------------

const BOM = [0xef, 0xbb, 0xbf];
const utf8 = (text: string) => [...new TextEncoder().encode(text)];

/** Набор образцов: те же виды файлов, что в tools/gen-vault (папка «Кодировки»). */
const SAMPLES: Record<string, number[]> = {
  'LF без BOM.md': utf8('# Заметка\nПервая строка\n\n- пункт\n'),
  'CRLF без BOM.md': utf8('# Заметка\r\nПервая строка\r\n\r\n- пункт\r\n'),
  'LF с BOM.md': [...BOM, ...utf8('# Заметка\nтекст\n')],
  'CRLF с BOM.md': [...BOM, ...utf8('# Заметка\r\nтекст\r\n')],
  'Смешанные переносы.md': utf8('a\r\nb\nc\r\nd\n'),
  'Без перевода строки в конце.md': utf8('# Заметка\r\nпоследняя строка'),
  'Одиночный CR.md': utf8('строка\rещё\r\n'),
  'Пустая.md': [],
  'Только BOM.md': [...BOM],
  'Невалидный UTF-8.md': [...utf8('начало '), 0xff, 0xfe, ...utf8(' конец\r\n')],
};

const LIMIT = 10 * 1024 * 1024;

class Disk {
  readonly files = new Map<string, Uint8Array>();
  writes = 0;
  #modified = 1;

  constructor(files: Record<string, number[]>) {
    for (const [name, bytes] of Object.entries(files)) this.files.set(name, Uint8Array.from(bytes));
  }

  version(name: string): FileVersion {
    const bytes = this.files.get(name) ?? new Uint8Array();
    return { hash: [...bytes].join('.').padEnd(64, '#').slice(0, 64) + bytes.length, size: bytes.length, modified_ms: this.#modified };
  }

  decode(name: string): { text: string; invalid: boolean } {
    let bytes = this.files.get(name) ?? new Uint8Array();
    if (bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) bytes = bytes.slice(3);
    let invalid = false;
    let text: string;
    try {
      text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
    } catch {
      invalid = true;
      text = new TextDecoder('utf-8').decode(bytes);
    }
    return { text: text.replaceAll('\r\n', '\n'), invalid };
  }

  read(path: VaultPath): NoteReadResponse {
    const name = path.join('/');
    if (!this.files.has(name)) throw { code: 'not_found', message_key: 'errors.not_found' } satisfies CoreError;
    const { text, invalid } = this.decode(name);
    const size = this.files.get(name)?.length ?? 0;
    const read_only = invalid
      ? ({ reason: 'invalid_utf8' } as const)
      : size > LIMIT
        ? ({ reason: 'too_large', limit_bytes: LIMIT } as const)
        : null;
    return { text, version: this.version(name), read_only };
  }

  write(path: VaultPath, text: string): NoteWriteResponse {
    const name = path.join('/');
    const disk = this.files.get(name);
    if (!disk) throw { code: 'not_found', message_key: 'errors.not_found' } satisfies CoreError;
    if (this.decode(name).text === text.replaceAll('\r\n', '\n')) {
      return { outcome: 'saved', version: this.version(name) };
    }
    const raw = [...disk];
    const crlf = raw.filter((b, i) => b === 0x0a && raw[i - 1] === 0x0d).length;
    const lf = raw.filter((b) => b === 0x0a).length - crlf;
    const bom = raw[0] === 0xef && raw[1] === 0xbb && raw[2] === 0xbf;
    let body = text.replaceAll('\r\n', '\n');
    if (crlf > lf) body = body.replaceAll('\n', '\r\n');
    this.files.set(name, Uint8Array.from([...(bom ? BOM : []), ...utf8(body)]));
    this.writes++;
    this.#modified++;
    return { outcome: 'saved', version: this.version(name) };
  }
}

// ---- Окно ---------------------------------------------------------------------

const SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: LIMIT,
  hotkeys: {},
};

function mocksFor(disk: Disk, extra: CoreMocks = {}): CoreMocks {
  return {
    vault_current: () => ({ api_version: 1, vault: { id: 'v1', name: 'Рабочая база' } }),
    settings_read: () => ({ settings: SETTINGS }),
    workspace_read: () => ({ workspace: null }),
    workspace_write: () => null,
    note_read: ({ path }) => disk.read(path),
    note_write: ({ path, text }) => disk.write(path, text),
    ...extra,
  };
}

async function openWindow(disk: Disk, first: VaultPath, extra: CoreMocks = {}) {
  const calls = mockCore(mocksFor(disk, extra));
  const model = new WindowModel();
  render(Shell, { model });
  await waitFor(() => expect(model.status.kind).toBe('ready'));
  model.open(first);
  const view = await editorView();
  return { calls, model, view };
}

/** Редактор CodeMirror открытой заметки. */
async function editorView(): Promise<EditorView> {
  const content = await screen.findByRole('textbox', { name: 'Текст заметки' });
  const view = EditorView.findFromDOM(content);
  if (!view) throw new Error('нет редактора');
  return view;
}

/** Правка пользователя (как ввод с клавиатуры). */
function type(view: EditorView, insert: string, at = view.state.doc.length) {
  view.dispatch({ changes: { from: at, insert }, userEvent: 'input.type' });
}

const writes = (calls: RecordedCall[]) => calls.filter(([command]) => command === 'note_write');

afterEach(() => {
  // Сначала размонтировать (дерево снимает подписки), потом убрать подмену.
  cleanup();
  clearMocks();
  delete document.documentElement.dataset.theme;
});

// ---- Тесты ----------------------------------------------------------------------

describe('байт в байт: открыть и сохранить без правок — файл не меняется', () => {
  it.each(Object.keys(SAMPLES))('%s', async (name) => {
    const disk = new Disk(SAMPLES);
    const before = disk.files.get(name);
    const { model, view, calls } = await openWindow(disk, [name]);
    expect(view.state.doc.toString()).toBe(disk.decode(name).text);

    // Переход к другой заметке и закрытие окна — оба повода сохранить.
    model.open(['LF без BOM.md' === name ? 'Пустая.md' : 'LF без BOM.md']);
    window.dispatchEvent(new Event('pagehide'));
    await new Promise((resolve) => setTimeout(resolve, 1200));

    expect(disk.files.get(name)).toEqual(before);
    expect(disk.writes).toBe(0);
    expect(writes(calls)).toEqual([]);
  });

  it.each(Object.keys(SAMPLES).filter((name) => !name.startsWith('Невалидный')))(
    '%s: правка и её отмена — файл не меняется',
    async (name) => {
      const disk = new Disk(SAMPLES);
      const before = disk.files.get(name);
      const { view } = await openWindow(disk, [name]);

      type(view, 'лишнее', 0);
      view.dispatch({ changes: { from: 0, to: 'лишнее'.length }, userEvent: 'delete' });
      window.dispatchEvent(new Event('pagehide'));
      // Текст снова равен файлу: таймер автосохранения снят, ждать его не нужно.
      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(disk.files.get(name)).toEqual(before);
    },
  );

  it('правка в CRLF-файле с BOM: BOM и CRLF на месте, изменилась только правка', async () => {
    const disk = new Disk(SAMPLES);
    const { view } = await openWindow(disk, ['CRLF с BOM.md']);

    type(view, 'ещё\n');
    window.dispatchEvent(new Event('pagehide'));

    await waitFor(() =>
      expect(disk.files.get('CRLF с BOM.md')).toEqual(Uint8Array.from([...BOM, ...utf8('# Заметка\r\nтекст\r\nещё\r\n')])),
    );
  });
});

describe('автосохранение и индикатор', () => {
  it('«Сохранено» → ввод → «Сохраняется» → через 1 с записано и «Сохранено»', async () => {
    const disk = new Disk(SAMPLES);
    const { view, calls } = await openWindow(disk, ['LF без BOM.md']);
    const status = screen.getByTestId('save-status');
    expect(status.textContent?.trim()).toBe('Сохранено');

    type(view, 'новое\n');
    await tick();
    expect(status.textContent?.trim()).toBe('Сохраняется');
    await new Promise((resolve) => setTimeout(resolve, 500));
    expect(writes(calls)).toEqual([]);

    await waitFor(() => expect(status.textContent?.trim()).toBe('Сохранено'), { timeout: 2000 });
    expect(writes(calls)).toHaveLength(1);
    expect(new TextDecoder().decode(disk.files.get('LF без BOM.md'))).toBe('# Заметка\nПервая строка\n\n- пункт\nновое\n');
  });

  it('переход к другой заметке записывает ввод сразу, не дожидаясь 1 с', async () => {
    const disk = new Disk(SAMPLES);
    const { view, model, calls } = await openWindow(disk, ['LF без BOM.md']);

    type(view, 'x');
    model.open(['CRLF без BOM.md']);
    // Переход применяется при ближайшем обновлении Svelte, без таймера.
    await tick();

    expect(writes(calls)).toHaveLength(1);
    await screen.findByRole('heading', { level: 1, name: 'CRLF без BOM' });
    await waitFor(() => expect(new TextDecoder().decode(disk.files.get('LF без BOM.md'))).toMatch(/x$/));
  });

  it('закрытие окна (pagehide) отправляет запись сразу', async () => {
    const disk = new Disk(SAMPLES);
    const { view, calls } = await openWindow(disk, ['LF без BOM.md']);

    type(view, 'перед закрытием');
    window.dispatchEvent(new Event('pagehide'));

    expect(writes(calls)).toHaveLength(1);
  });

  it('уход со страницы (beforeunload) и скрытие окна тоже сохраняют', async () => {
    const disk = new Disk(SAMPLES);
    const { view, calls } = await openWindow(disk, ['LF без BOM.md']);

    type(view, '1');
    window.dispatchEvent(new Event('beforeunload'));
    expect(writes(calls)).toHaveLength(1);
  });
});

describe('закрытие окна (задача 0.7): сигнал ядра window:close-requested', () => {
  const answers = (calls: RecordedCall[]) =>
    calls.map(([command]) => command).filter((command) => command.startsWith('window_close_'));

  it('ввод записывается, и только потом ядру уходит «готово»', async () => {
    const disk = new Disk(SAMPLES);
    let release: () => void = () => undefined;
    const held = new Promise<void>((resolve) => (release = resolve));
    const { view, calls } = await openWindow(disk, ['LF без BOM.md'], {
      note_write: async ({ path, text }) => {
        await held;
        return disk.write(path, text);
      },
      window_close_ready: () => null,
      window_close_cancel: () => null,
    });
    type(view, 'ещё');

    await emitCoreEvent('window:close-requested', { sessionEnd: false });
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(writes(calls)).toHaveLength(1);
    expect(answers(calls)).toEqual([]);

    release();
    await waitFor(() => expect(answers(calls)).toEqual(['window_close_ready']));
    expect(disk.decode('LF без BOM.md').text).toBe('# Заметка\nПервая строка\n\n- пункт\nещё');
  });

  it('без заметки — сразу «готово»', async () => {
    const calls = mockCore(mocksFor(new Disk({}), { tree_list: () => ({ entries: [] }), window_close_ready: () => null }));
    const model = new WindowModel();
    render(Shell, { model });
    await waitFor(() => expect(model.status.kind).toBe('ready'));

    await emitCoreEvent('window:close-requested', { sessionEnd: false });
    await waitFor(() => expect(answers(calls)).toEqual(['window_close_ready']));
  });

  it('запись не удалась — окно не закрывается: «отмена» ядру и уведомление; текст в редакторе', async () => {
    const disk = new Disk(SAMPLES);
    const failure: CoreError = { code: 'io_failed', message_key: 'errors.io_failed' };
    const { view, calls } = await openWindow(disk, ['LF без BOM.md'], {
      note_write: () => {
        throw failure;
      },
      window_close_ready: () => null,
      window_close_cancel: () => null,
    });
    type(view, 'ещё');

    await emitCoreEvent('window:close-requested', { sessionEnd: false });

    await waitFor(() => expect(answers(calls)).toEqual(['window_close_cancel']));
    expect(await screen.findByText(/Окно не закрыто: не удалось сохранить заметку «LF без BOM»/)).toBeTruthy();
    expect(view.state.doc.toString()).toContain('ещё');
  });
});

describe('конфликт изменений и удаление снаружи', () => {
  it('ядро сохранило обе версии — уведомление внизу окна', async () => {
    const disk = new Disk(SAMPLES);
    const { view } = await openWindow(disk, ['LF без BOM.md'], {
      note_write: () => ({
        outcome: 'conflict_saved',
        version: disk.version('LF без BOM.md'),
        copy_path: ['LF без BOM (внешняя версия 2026-10-25 12-00).md'],
      }),
    });

    type(view, 'моё');
    window.dispatchEvent(new Event('pagehide'));

    expect(
      await screen.findByText(
        'Заметка «LF без BOM» изменилась на диске одновременно с вашими правками. Сохранены обе версии: ваша — в заметке, версия с диска — рядом, в «LF без BOM (внешняя версия 2026-10-25 12-00)».',
      ),
    ).toBeTruthy();
    expect(screen.getByTestId('save-status').textContent?.trim()).toBe('Сохранено');
  });

  it('сбой записи — уведомление, «Не сохранено», текст остаётся в редакторе', async () => {
    const disk = new Disk(SAMPLES);
    const { view } = await openWindow(disk, ['LF без BOM.md'], {
      note_write: () => {
        throw { code: 'io_failed', message_key: 'errors.io_failed' } satisfies CoreError;
      },
    });

    type(view, 'важное');
    window.dispatchEvent(new Event('pagehide'));

    expect(
      await screen.findByText(
        'Не удалось сохранить заметку «LF без BOM». Не удалось прочитать или записать файл. Текст остаётся в редакторе, сохранение повторится.',
      ),
    ).toBeTruthy();
    expect(screen.getByTestId('save-status').textContent?.trim()).toBe('Не сохранено');
    expect(view.state.doc.toString()).toMatch(/важное$/);
  });

  it('файла нет на диске (not_found): текст остаётся, «Не сохранено: файла нет на диске», ошибка объявляется', async () => {
    const disk = new Disk(SAMPLES);
    const { view, calls } = await openWindow(disk, ['LF без BOM.md']);
    disk.files.delete('LF без BOM.md');

    type(view, 'x');
    window.dispatchEvent(new Event('pagehide'));

    await waitFor(() =>
      expect(screen.getByTestId('save-status').textContent?.trim()).toBe('Не сохранено: файла нет на диске'),
    );
    expect(view.state.doc.toString()).toMatch(/x$/);
    expect(view.state.readOnly).toBe(false);
    expect(writes(calls)).toHaveLength(1);
    expect(screen.getByRole('status', { name: 'Состояние сохранения' }).textContent).toBe(
      'Не сохранено: файла нет на диске',
    );
  });

  it('пауза ввода не объявляется живой областью; подробность ошибки доступна с клавиатуры', async () => {
    const disk = new Disk(SAMPLES);
    const { view } = await openWindow(disk, ['LF без BOM.md'], {
      note_write: () => {
        throw { code: 'io_failed', message_key: 'errors.io_failed' } satisfies CoreError;
      },
    });
    const live = screen.getByRole('status', { name: 'Состояние сохранения' });

    type(view, 'a');
    await tick();
    expect(live.textContent).toBe('');

    window.dispatchEvent(new Event('pagehide'));
    await waitFor(() => expect(live.textContent).toBe('Не сохранено'));
    const detail = screen.getByRole('button', {
      name: 'Не сохранено',
      description: 'Не удалось прочитать или записать файл.',
    });
    detail.focus();
    expect(document.activeElement).toBe(detail);
  });
});

describe('только чтение', () => {
  it('невалидный UTF-8: подсказка, текст не редактируется, индикатор «Только чтение»', async () => {
    const disk = new Disk(SAMPLES);
    const { view, calls } = await openWindow(disk, ['Невалидный UTF-8.md']);

    expect(screen.getByText('Только чтение: файл не в кодировке UTF-8')).toBeTruthy();
    expect(screen.getByTestId('save-status').textContent?.trim()).toBe('Только чтение');
    expect(view.state.readOnly).toBe(true);
    expect(view.contentDOM.getAttribute('contenteditable')).toBe('false');

    type(view, 'x');
    window.dispatchEvent(new Event('pagehide'));
    expect(writes(calls)).toEqual([]);
  });

  it('файл больше лимита: размер и лимит в подсказке, где поднять лимит', async () => {
    const size = 14.2 * 1024 * 1024;
    const big: NoteReadResponse = {
      text: '# Выгрузка\nстрока\n',
      version: { hash: 'e'.repeat(64), size, modified_ms: 1 },
      read_only: { reason: 'too_large', limit_bytes: LIMIT },
    };
    const { view } = await openWindow(new Disk({}), ['Выгрузка.md'], { note_read: () => big });

    const mb = new Intl.NumberFormat('ru', { maximumFractionDigits: 1 }).format(size / 1024 / 1024);
    expect(screen.getByText(`Только чтение: файл ${mb} МБ, лимит для редактирования — 10 МБ`)).toBeTruthy();
    expect(
      screen.getByText(
        'Чтобы редактировать, увеличьте «Максимальный размер файла для редактирования» в настройках хранилища и откройте файл заново.',
      ),
    ).toBeTruthy();
    expect(view.state.readOnly).toBe(true);
  });

  it('файл больше 100 МБ: ядро не отдаёт текст — так и сказано', async () => {
    const disk = new Disk({});
    const huge: NoteReadResponse = {
      text: '',
      version: { hash: 'f'.repeat(64), size: 150 * 1024 * 1024, modified_ms: 1 },
      read_only: { reason: 'too_large', limit_bytes: LIMIT },
    };
    await openWindow(disk, ['Огромный.md'], { note_read: () => huge });

    expect(screen.getByText('Файл больше 100 МБ, поэтому его текст не показывается.')).toBeTruthy();
  });
});

describe('заголовок и открытие', () => {
  it('заголовок над текстом — имя файла; проверка орфографии выключена', async () => {
    const disk = new Disk(SAMPLES);
    const { view } = await openWindow(disk, ['LF с BOM.md']);

    expect(screen.getByRole('heading', { level: 1, name: 'LF с BOM' })).toBeTruthy();
    expect(view.contentDOM.getAttribute('spellcheck')).toBe('false');
  });

  it('при открытии редактор не забирает фокус (поле имени новой заметки в дереве, 2.3)', async () => {
    const outside = document.createElement('input');
    document.body.append(outside);
    outside.focus();
    const disk = new Disk(SAMPLES);
    await openWindow(disk, ['LF без BOM.md']);

    expect(document.activeElement).toBe(outside);
    outside.remove();
  });

  it('заметку не удалось открыть — сообщение из словаря', async () => {
    mockCore(mocksFor(new Disk({})));
    const model = new WindowModel();
    render(Shell, { model });
    await waitFor(() => expect(model.status.kind).toBe('ready'));
    model.open(['Нет.md']);

    expect(await screen.findByText('Не удалось открыть заметку. Файл или папка не найдены.')).toBeTruthy();
  });
});

describe('сигналы «переименовано» и «удалено» (API компонента для дерева и событий ядра)', () => {
  async function renderEditor(disk: Disk, path: VaultPath) {
    const calls = mockCore(mocksFor(disk));
    const result = render(Editor, { path, notify: () => {}, onStatus: () => {} });
    const view = await editorView();
    return { calls, view, ...result };
  }

  it('renamed + переход на новый путь: запись не по старому пути, а по новому с прежней версией', async () => {
    const disk = new Disk(SAMPLES);
    const version = disk.version('LF без BOM.md');
    const { calls, view, component, rerender } = await renderEditor(disk, ['LF без BOM.md']);
    type(view, 'x');
    disk.files.set('Новое имя.md', disk.files.get('LF без BOM.md') ?? new Uint8Array());
    disk.files.delete('LF без BOM.md');

    component.renamed(['LF без BOM.md'], ['Новое имя.md']);
    await rerender({ path: ['Новое имя.md'] });
    expect(writes(calls)).toEqual([]);

    window.dispatchEvent(new Event('pagehide'));
    expect(writes(calls)).toEqual([
      ['note_write', { request: { path: ['Новое имя.md'], text: view.state.doc.toString(), expected_version: version } }],
    ]);
  });

  it('removed + закрытие: ввод удалённой заметки не пишется', async () => {
    const disk = new Disk(SAMPLES);
    const { calls, view, component, rerender } = await renderEditor(disk, ['LF без BOM.md']);
    type(view, 'x');

    component.removed(['LF без BOM.md']);
    await rerender({ path: null });
    window.dispatchEvent(new Event('pagehide'));
    await new Promise((resolve) => setTimeout(resolve, 1200));

    expect(JSON.stringify(writes(calls))).toBe('[]');
  });
});

describe('удалённая заметка', () => {
  it('после removed() редактор только для чтения, индикатор «Удалена на диске»', async () => {
    mockCore(mocksFor(new Disk(SAMPLES)));
    const result = render(Editor, {
      path: ['LF без BOM.md'],
      notify: () => {},
      onStatus: () => {},
    });
    const view = await editorView();
    type(view, 'x');

    result.component.removed(['LF без BOM.md']);

    await waitFor(() => expect(view.state.readOnly).toBe(true));
    expect(view.contentDOM.getAttribute('contenteditable')).toBe('false');
  });
});
