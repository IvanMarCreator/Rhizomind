<!--
  Поле нового имени (Э4 — в строке дерева, Э4б — в заголовке заметки):
  проверка при вводе, пояснение под полем, Enter сохраняет только допустимое
  имя, Esc отменяет, уход фокуса сохраняет допустимое имя или отменяет.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import type { VaultPath } from '../api/index.ts';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import { NameInput } from './nameInput.svelte.ts';

  let {
    dir,
    stem,
    ext,
    renaming,
    submit,
    onDone,
    variant = 'row',
  }: {
    dir: VaultPath;
    stem: string;
    ext: string;
    renaming: VaultPath;
    submit: (fullName: string) => Promise<string | null>;
    /** Поле закрыто: имя сохранено или правка отменена. */
    onDone: () => void;
    variant?: 'row' | 'title';
  } = $props();

  const field = createField();
  const messageId = `name-field-${Math.random().toString(36).slice(2)}`;
  let input: HTMLInputElement | undefined = $state();
  let closed = false;

  function createField(): NameInput {
    return new NameInput({ dir, stem, ext, renaming, submit });
  }

  onMount(() => {
    input?.focus();
    input?.select();
  });

  function close() {
    if (closed) return;
    closed = true;
    onDone();
  }

  async function commit(fromBlur: boolean) {
    if (closed || field.busy) return;
    const result = await field.commit();
    if (result === 'rejected' && !fromBlur) return;
    close();
  }

  function onkeydown(event: KeyboardEvent) {
    event.stopPropagation();
    if (event.key === 'Enter') {
      event.preventDefault();
      void commit(false);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      close();
    }
  }
</script>

<div class="field {variant}" class:invalid={field.error !== null}>
  <div class="box">
    <input
      bind:this={input}
      value={field.value}
      aria-label={t('tree.rename.label')}
      aria-invalid={field.error ? 'true' : 'false'}
      aria-describedby={messageId}
      spellcheck="false"
      autocomplete="off"
      oninput={(event) => void field.input(event.currentTarget.value)}
      {onkeydown}
      onblur={() => void commit(true)}
      onclick={(event) => event.stopPropagation()}
      ondblclick={(event) => event.stopPropagation()}
    />
    {#if variant === 'title'}
      <span class="hint">{t('note.title.editHint')}</span>
    {/if}
  </div>
  <div id={messageId}>
    {#if field.error}
      <p class="message error" role="alert">
        <Icon name="triangle-alert" size={13} />
        <span>{field.error}</span>
      </p>
    {:else if field.warning}
      <p class="message warning" role="status">
        <Icon name="triangle-alert" size={13} />
        <span>{field.warning}</span>
      </p>
    {/if}
  </div>
</div>

<style>
  .field {
    flex: 1;
    min-width: 0;
  }

  .box {
    display: flex;
    align-items: center;
    gap: 12px;
    border-radius: 4px;
    background: var(--bg-app);
    box-shadow: 0 0 0 1.5px var(--accent);
  }

  .invalid .box {
    box-shadow: 0 0 0 1.5px var(--status-error);
  }

  input {
    flex: 1;
    min-width: 0;
    margin: 0;
    padding: 0 4px;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--text-primary);
    font: inherit;
    caret-color: var(--accent);
    user-select: text;
  }

  .row input {
    height: 20px;
  }

  .title .box {
    padding: 0 10px 0 0;
  }

  .title input {
    height: 36px;
    font-size: 28px;
    font-weight: 700;
    line-height: 36px;
  }

  .hint {
    flex: none;
    color: var(--text-faint);
    font-size: var(--font-size-caption);
    white-space: nowrap;
  }

  .message {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    margin: 6px 0 2px;
    font-size: var(--font-size-caption);
    line-height: 1.35;
    white-space: normal;
    user-select: text;
  }

  .row .message {
    padding: 6px 8px;
    border-radius: 4px;
    background: var(--bg-popover);
  }

  .row .message.error {
    box-shadow: 0 0 0 1px var(--status-error);
  }

  .title .message {
    font-size: 13px;
  }

  .message :global(svg) {
    margin-top: 1px;
  }

  .error {
    color: var(--status-error);
  }

  .warning {
    color: var(--status-warning);
  }
</style>
