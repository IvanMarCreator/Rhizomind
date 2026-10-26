<!--
  Форма «Создать хранилище» (Э11в): имя и кнопка «Создать». Родительскую
  папку пользователь выбирает в системном окне ядра после нажатия «Создать»
  (команда vault_create): поля «Где создать» с кнопкой «Обзор…» из макета в
  0.1 нет — путь в интерфейс не передаётся (вопрос владельцу, обзор 2.8а).
  Имя проверяет ядро теми же правилами, что имена файлов.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import type { VaultManagerModel } from './manager.svelte.ts';

  let { model }: { model: VaultManagerModel } = $props();

  let name = $state('');
  let field: HTMLInputElement | undefined = $state();
  const errorId = `vault-name-error-${Math.random().toString(36).slice(2)}`;
  const canCreate = $derived(name.trim() !== '' && !model.busy);

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
        aria-describedby={model.nameError ? errorId : undefined}
        oninput={() => model.nameChanged()}
        {onkeydown}
      />
    </label>
    {#if model.nameError}
      <p class="error" id={errorId} role="alert">{model.nameError}</p>
    {/if}
    <p class="hint">{t('vaults.manager.create.where')}</p>
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
