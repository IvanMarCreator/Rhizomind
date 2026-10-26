<!--
  Форма «Создать хранилище» (Э11в, решение владельца 62): имя → «Обзор…»
  (родительская папка в системном окне ядра, команда folder_pick; путь
  показывается в поле только для чтения) → «Создать» (vault_create с именем и
  этой папкой). Под полями — итоговый путь. Имя проверяет ядро теми же
  правилами, что имена файлов; папку — что она есть и не внутри хранилища.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import type { VaultManagerModel } from './manager.svelte.ts';

  let { model }: { model: VaultManagerModel } = $props();

  let name = $state('');
  let field: HTMLInputElement | undefined = $state();
  const id = Math.random().toString(36).slice(2);
  const nameErrorId = `vault-name-error-${id}`;
  const parentErrorId = `vault-parent-error-${id}`;
  const hintId = `vault-create-hint-${id}`;
  const canCreate = $derived(name.trim() !== '' && model.parent !== null && !model.busy);
  /** Итоговый путь нового хранилища — для подсказки; разделитель — как в пути папки. */
  const target = $derived.by(() => {
    if (model.parent === null || name.trim() === '') return null;
    const separator = model.parent.includes('\\') ? '\\' : '/';
    return model.parent.endsWith(separator) ? `${model.parent}${name}` : `${model.parent}${separator}${name}`;
  });

  onMount(() => field?.focus());

  function submit() {
    if (canCreate) void model.create(name);
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      model.showActions();
    } else if (event.key === 'Enter') {
      event.preventDefault();
      submit();
    }
  }

  function describedBy(...ids: (string | false)[]): string {
    return ids.filter(Boolean).join(' ');
  }
</script>

<div class="create">
  <button type="button" class="back" onclick={() => model.showActions()}>
    <Icon name="arrow-left" size={14} />
    {t('vaults.manager.back')}
  </button>
  <h2 class="title">{t('vaults.manager.create.title')}</h2>
  <!-- Не <form>: проверка сети запрещает формы (tools/check-no-network); Enter — в onkeydown. -->
  <div class="form">
    <label class="field">
      <span class="label">{t('vaults.manager.name')}</span>
      <input
        bind:this={field}
        bind:value={name}
        class="input"
        class:invalid={model.nameError !== null}
        type="text"
        spellcheck="false"
        autocomplete="off"
        aria-invalid={model.nameError !== null}
        aria-describedby={describedBy(hintId, model.nameError !== null && nameErrorId)}
        oninput={() => model.nameChanged()}
        {onkeydown}
      />
    </label>
    {#if model.nameError}
      <p class="error" id={nameErrorId} role="alert">{model.nameError}</p>
    {/if}
    <div class="field">
      <label class="label" for={`vault-parent-${id}`}>{t('vaults.manager.where')}</label>
      <div class="row">
        <input
          id={`vault-parent-${id}`}
          class="input path"
          class:invalid={model.parentError !== null}
          type="text"
          readonly
          value={model.parent ?? ''}
          title={model.parent ?? undefined}
          aria-invalid={model.parentError !== null}
          aria-describedby={describedBy(hintId, model.parentError !== null && parentErrorId)}
        />
        <button type="button" class="secondary" disabled={model.busy} onclick={() => void model.pickParent()}>
          {t('vaults.manager.browse')}
        </button>
      </div>
    </div>
    {#if model.parentError}
      <p class="error" id={parentErrorId} role="alert">{model.parentError}</p>
    {/if}
    <p class="hint" id={hintId}>
      {#if target}{t('vaults.manager.create.resultBefore')}<strong class="target">{target}</strong>{t(
          'vaults.manager.create.resultAfter',
        )}{:else}{t('vaults.manager.create.where')}{/if}
    </p>
    <div class="buttons">
      <button type="button" class="primary" disabled={!canCreate} onclick={submit}>
        {t('vaults.manager.create.button')}
      </button>
      <button type="button" class="cancel" onclick={() => model.showActions()}>{t('vaults.manager.cancel')}</button>
    </div>
  </div>
</div>

<style>
  .create {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
  }

  .back {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 14px;
    border-radius: 4px;
    color: var(--icon-default);
    font-size: 12px;
    font-weight: 500;
  }

  .back:hover {
    color: var(--text-primary);
  }

  .title {
    margin: 0;
    color: var(--text-primary);
    font-size: 16px;
    font-weight: 600;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: 100%;
    padding-top: 16px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .label {
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
  }

  .input {
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-primary);
    font: inherit;
    font-size: 13px;
    box-sizing: border-box;
    caret-color: var(--accent);
    user-select: text;
  }

  .input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 0.5px var(--accent);
  }

  .input.invalid,
  .input.invalid:focus {
    border-color: var(--status-error);
    box-shadow: 0 0 0 0.5px var(--status-error);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .path {
    flex: 1;
    min-width: 0;
    text-overflow: ellipsis;
    cursor: default;
  }

  .secondary {
    flex: none;
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 500;
  }

  .secondary:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .secondary:disabled {
    opacity: 0.45;
  }

  .target {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .error {
    margin: -8px 0 0;
    color: var(--status-error);
    font-size: 13px;
  }

  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: 12px;
    line-height: 17px;
  }

  .buttons {
    display: flex;
    gap: 8px;
    padding-top: 4px;
  }

  .primary,
  .cancel {
    height: 32px;
    padding: 0 14px;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 500;
  }

  .primary {
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .primary:disabled {
    opacity: 0.45;
  }

  .cancel {
    color: var(--text-secondary);
  }

  .cancel:hover {
    background: var(--bg-hover);
  }
</style>
