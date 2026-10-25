// Поле нового имени без разметки: сохраняется ровно то имя, которое проверило ядро.
import { clearMocks } from '@tauri-apps/api/mocks';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { NameCheck } from '../api/index.ts';
import { mockCore } from '../api/testing.ts';
import { NameInput } from './nameInput.svelte.ts';

afterEach(() => clearMocks());

describe('NameInput', () => {
  it('сохраняет проверенное имя, даже если значение поменялось во время проверки', async () => {
    let answer: ((value: NameCheck) => void) | undefined;
    mockCore({ name_check: () => new Promise<NameCheck>((resolve) => (answer = resolve)) });
    const submit = vi.fn(async () => null);
    const field = new NameInput({ kind: 'note', dir: [], stem: 'А', ext: '.md', renaming: ['А.md'], submit });
    field.value = 'Б';

    const done = field.commit();
    await vi.waitFor(() => expect(answer).toBeDefined());
    field.value = 'В:';
    answer?.({ status: 'ok', warning: null });

    expect(await done).toBe('saved');
    expect(submit).toHaveBeenCalledWith('Б.md');
  });

  it.each([
    ['note', 'На эту заметку нельзя будет сослаться.'],
    ['folder', 'На заметки в этой папке нельзя будет сослаться с путём.'],
    ['attachment', 'На этот файл нельзя будет сослаться.'],
  ] as const)('предупреждение unlinkable для «%s» — свой текст', async (kind, text) => {
    mockCore({ name_check: () => ({ status: 'ok', warning: 'unlinkable' }) });
    const field = new NameInput({ kind, dir: [], stem: 'А', ext: '', renaming: ['А'], submit: async () => null });
    await field.input('#А');
    expect(field.warning).toBe(text);
  });
});
