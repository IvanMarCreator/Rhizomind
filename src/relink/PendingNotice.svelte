<!--
  Уведомление «нужен ответ» (0.2-6.3; С2-3.5 «в Проводнике», С2-3.6
  «Массовое копирование»; решения 110, 111; макеты У-1, У-2): значок, текст,
  приглушённая вторая строка, кнопки ответа — основная слева. Без крестика:
  висит до ответа. Ответ не принят — строка ошибки; если ответить уже нельзя,
  кнопок нет и появляется крестик. Окно поверх работы не открывается.
-->
<script lang="ts">
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import type { PendingQuestion } from './notices.ts';

  let {
    question,
    onAnswer,
    onDismiss,
  }: {
    question: PendingQuestion;
    /** `true` — «Обновить ссылки» / «Уточнить», `false` — «Не обновлять» / «Не трогать». */
    onAnswer: (apply: boolean) => void;
    onDismiss: () => void;
  } = $props();
</script>

<div class="notice" data-reason={question.reason} aria-busy={question.busy ? 'true' : undefined}>
  <span class="mark">
    <Icon name={question.reason === 'duplicate' ? 'copy' : 'pencil'} size={16} />
  </span>
  <div class="main">
    <p class="text">{question.text}</p>
    {#if question.detail && !question.stale}<p class="detail">{question.detail}</p>{/if}
    {#if question.error}<p class="error" role="alert">{question.error}</p>{/if}
    {#if !question.stale}
      <div class="buttons">
        <button type="button" class="button primary" disabled={question.busy} onclick={() => onAnswer(true)}>
          {question.apply}
        </button>
        <button type="button" class="button" disabled={question.busy} onclick={() => onAnswer(false)}>
          {question.skip}
        </button>
      </div>
    {/if}
  </div>
  {#if question.closable}
    <button
      type="button"
      class="close"
      aria-label={t('notifications.close')}
      title={t('notifications.close')}
      onclick={onDismiss}
    >
      <Icon name="x" size={14} />
    </button>
  {/if}
</div>

<style>
  .notice {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 14px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--bg-popover);
    box-shadow: 0 6px 20px rgb(0 0 0 / 12%);
    pointer-events: auto;
  }

  .mark {
    display: grid;
    flex: none;
    padding-top: 1px;
    color: var(--icon-default);
  }

  .main {
    flex: 1;
    min-width: 0;
  }

  .text {
    margin: 0;
    color: var(--text-primary);
    line-height: 18px;
  }

  .detail,
  .error {
    margin: 4px 0 0;
    font-size: var(--font-size-caption);
    line-height: 16px;
  }

  .detail {
    color: var(--text-muted);
  }

  .error {
    color: var(--status-error);
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 10px;
  }

  .button {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-app);
    color: var(--text-primary);
    font-weight: 500;
    white-space: nowrap;
  }

  .button:hover:not(:disabled) {
    background: var(--bg-hover);
  }

  .button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .button.primary:hover:not(:disabled) {
    background: var(--accent);
    filter: brightness(1.08);
  }

  .button:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .button:focus-visible,
  .close:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .close {
    display: grid;
    place-items: center;
    flex: none;
    width: 22px;
    height: 22px;
    margin: -2px -4px 0 0;
    border-radius: 5px;
    color: var(--icon-default);
  }

  .close:hover {
    background: var(--bg-hover);
  }
</style>
