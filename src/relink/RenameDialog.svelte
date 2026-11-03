<!--
  Окно вопроса при переименовании и перемещении в приложении (0.2-5.2;
  С2-3.5 «в приложении», С2-3.7; решения 109, 114, 116, 120; макеты Д-А и Д-1):
  «Отменить переименование» слева, «Переименовать без обновления» и «Обновить
  ссылки» справа; галочка «Больше не спрашивать». Пока индекс строится —
  «Подождите, индекс строится…»: в режиме «Всегда спрашивать» кнопки
  недоступны, в режиме «Всегда обновлять» — только отмена. Enter — «Обновить
  ссылки», Esc — отмена. Состояние и ответ — у дерева (FileTreeState.question):
  окно показывается оболочкой окна, переименование и из дерева, и из
  заголовка заметки идёт одним путём.
-->
<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { t } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import type { FileTreeState, RenameQuestion } from '../tree/tree.svelte.ts';

  let { tree }: { tree: FileTreeState } = $props();

  const uid = Math.random().toString(36).slice(2);
  const titleId = `rename-dialog-${uid}`;
  const hintId = `rename-dialog-hint-${uid}`;

  let dialog: HTMLDivElement | undefined = $state();
  let updateButton: HTMLButtonElement | undefined = $state();
  let cancelButton: HTMLButtonElement | undefined = $state();
  let dontAsk = $state(false);
  /** Куда вернуть фокус, когда окно закроется (поле имени, строка дерева). */
  let returnFocus: Element | null = null;
  /** Состояние окна, для которого уже поставлен фокус. */
  let focusedFor: string | null = null;

  const question = $derived(tree.question);
  const waiting = $derived(question?.state === 'waiting');
  /** В окне ожидания режима «Всегда обновлять» — только отмена (Д-1). */
  const cancelOnly = $derived(waiting && question?.mode === 'update');

  function plural(count: number): 'one' | 'few' | 'many' {
    const ten = count % 10;
    const hundred = count % 100;
    if (ten === 1 && hundred !== 11) return 'one';
    if (ten >= 2 && ten <= 4 && (hundred < 12 || hundred > 14)) return 'few';
    return 'many';
  }

  function title(q: RenameQuestion): string {
    if (q.state === 'waiting') return t('relink.wait.title');
    const form = plural(q.notes);
    const params = { name: q.name, count: q.notes };
    return q.kind === 'folder' ? t(`relink.ask.folder.${form}`, params) : t(`relink.ask.note.${form}`, params);
  }

  function body(q: RenameQuestion): string {
    if (q.state === 'waiting') {
      if (q.mode === 'update') return t('relink.wait.bodyUpdate');
      return q.kind === 'folder' ? t('relink.wait.bodyFolder') : t('relink.wait.body');
    }
    return q.kind === 'folder' ? t('relink.ask.bodyFolder') : t('relink.ask.body');
  }

  const percent = $derived(
    question?.progress && question.progress.total > 0
      ? Math.min(100, Math.floor((question.progress.done / question.progress.total) * 100))
      : null,
  );

  // Фокус: в вопросе — «Обновить ссылки» (Enter), пока индекс строится —
  // «Отменить переименование» (макет Д-1). Окно открылось — запомнить, куда вернуть.
  $effect(() => {
    const key = question ? question.state : null;
    if (key === focusedFor) return;
    if (focusedFor === null && key !== null) {
      returnFocus = document.activeElement;
      dontAsk = false;
    }
    focusedFor = key;
    if (key === null) {
      restoreFocus();
      return;
    }
    void tick().then(() => (key === 'ask' ? updateButton : cancelButton)?.focus());
  });

  function restoreFocus() {
    const target = returnFocus;
    returnFocus = null;
    if (target instanceof HTMLElement && target.isConnected) target.focus();
  }

  onDestroy(() => {
    if (tree.question) tree.answerQuestion('cancel');
  });

  function answer(value: 'update' | 'keep' | 'cancel') {
    if (value === 'keep' && dontAsk) return;
    tree.answerQuestion(value, value === 'update' && dontAsk);
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      answer('cancel');
    } else if (event.key === 'Tab') {
      // Окно модальное: фокус ходит по его доступным элементам по кругу.
      event.preventDefault();
      const items = dialog
        ? [...dialog.querySelectorAll<HTMLElement>('button, input')].filter(
            (item) => !(item as HTMLButtonElement | HTMLInputElement).disabled,
          )
        : [];
      const index = items.indexOf(document.activeElement as HTMLElement);
      const step = event.shiftKey ? -1 : 1;
      items[(index + step + items.length) % items.length]?.focus();
    }
  }
</script>

{#if question}
  <div class="scrim">
    <div
      bind:this={dialog}
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-busy={waiting ? 'true' : undefined}
      tabindex="-1"
      data-testid="rename-dialog"
      {onkeydown}
    >
      <div class="content">
        <div class="head">
          <span class="badge" class:spin={waiting} aria-hidden="true">
            <Icon name={waiting ? 'loader-circle' : 'link'} size={16} />
          </span>
          <h2 class="title" id={titleId}>{title(question)}</h2>
          <button type="button" class="close" aria-label={t('relink.ask.close')} title={t('relink.ask.close')} onclick={() => answer('cancel')}>
            <Icon name="x" size={15} />
          </button>
        </div>

        <div class="change" aria-label={t('relink.ask.change', { from: question.from, to: question.to })}>
          <span class="from">{question.from}</span>
          <span class="arrow" aria-hidden="true"><Icon name="arrow-right" size={13} /></span>
          <span class="to">{question.to}</span>
        </div>

        <p class="body">{body(question)}</p>

        {#if waiting}
          <div class="progress">
            <span class="progress-label">
              <span class="spin small" aria-hidden="true"><Icon name="loader-circle" size={13} /></span>
              {percent === null ? t('relink.wait.progressUnknown') : t('relink.wait.progress', { percent })}
            </span>
            <div
              class="bar"
              role="progressbar"
              aria-label={t('relink.wait.progressUnknown')}
              aria-valuemin="0"
              aria-valuemax="100"
              aria-valuenow={percent ?? undefined}
            >
              <span class="fill" class:unknown={percent === null} style:width={percent === null ? undefined : `${percent}%`}></span>
            </div>
          </div>
        {/if}

        {#if !cancelOnly}
          <label class="check" class:disabled={waiting}>
            <input type="checkbox" spellcheck="false" bind:checked={dontAsk} disabled={waiting} />
            <span>{t('relink.ask.dontAsk')}</span>
          </label>
          <p class="caption">{t('relink.ask.dontAskHint')}</p>
        {/if}
      </div>

      <div class="actions">
        <button type="button" class="button" bind:this={cancelButton} onclick={() => answer('cancel')}>
          {t('relink.ask.cancel')}
        </button>
        {#if !cancelOnly}
          <span class="spacer"></span>
          <span class="keep">
            <button
              type="button"
              class="button"
              disabled={waiting || dontAsk}
              aria-describedby={dontAsk && !waiting ? hintId : undefined}
              onclick={() => answer('keep')}
            >
              {t('relink.ask.keep')}
            </button>
            {#if dontAsk && !waiting}
              <span class="tooltip" id={hintId} role="tooltip">{t('relink.ask.keepDisabled')}</span>
            {/if}
          </span>
          <button
            type="button"
            class="button primary"
            bind:this={updateButton}
            disabled={waiting}
            onclick={() => answer('update')}
          >
            {t('relink.ask.update')}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: grid;
    place-items: center;
    background: var(--bg-scrim);
  }

  .dialog {
    width: 700px;
    max-width: calc(100vw - 32px);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg-popover);
    box-shadow: 0 16px 48px rgb(0 0 0 / 22%);
  }

  .dialog:focus {
    outline: none;
  }

  .content {
    padding: 20px 20px 16px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .badge {
    display: grid;
    place-items: center;
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--accent-subtle);
    color: var(--accent-text);
  }

  .title {
    flex: 1;
    min-width: 0;
    margin: 0;
    color: var(--text-primary);
    font-size: 15px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .close {
    display: grid;
    place-items: center;
    flex: none;
    width: 26px;
    height: 26px;
    border-radius: 5px;
    color: var(--icon-default);
  }

  .close:hover {
    background: var(--bg-hover);
  }

  .close:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .change {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 100%;
    margin: 14px 0 0;
    padding: 5px 10px;
    border-radius: 6px;
    background: var(--bg-code);
    box-sizing: border-box;
  }

  .from {
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }

  .arrow {
    display: grid;
    flex: none;
    color: var(--icon-muted);
  }

  .to {
    color: var(--text-primary);
    font-weight: 500;
    overflow-wrap: anywhere;
  }

  .body {
    margin: 14px 0 0;
    color: var(--text-secondary);
  }

  .progress {
    margin: 12px 0 0;
  }

  .progress-label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }

  .bar {
    position: relative;
    height: 4px;
    margin-top: 6px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--bg-active);
  }

  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 2px;
    background: var(--accent);
  }

  .fill.unknown {
    width: 30%;
    animation: slide 1.4s ease-in-out infinite;
  }

  .check {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin: 14px 0 0;
    color: var(--text-primary);
    cursor: pointer;
  }

  .check.disabled {
    color: var(--text-faint);
    cursor: default;
  }

  .check input {
    width: 15px;
    height: 15px;
    margin: 0;
    accent-color: var(--accent);
  }

  .check input:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .caption {
    margin: 6px 0 0;
    color: var(--text-faint);
    font-size: var(--font-size-caption);
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--border-default);
    border-radius: 0 0 10px 10px;
    background: var(--bg-sidebar);
  }

  .spacer {
    flex: 1;
  }

  .keep {
    position: relative;
    display: inline-flex;
  }

  .tooltip {
    position: absolute;
    top: calc(100% + 6px);
    left: 50%;
    z-index: 1;
    padding: 5px 9px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--bg-popover);
    box-shadow: 0 4px 12px rgb(0 0 0 / 14%);
    color: var(--text-primary);
    font-size: var(--font-size-caption);
    white-space: nowrap;
    transform: translateX(-50%);
  }

  .button {
    height: 32px;
    padding: 0 14px;
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

  .button:disabled {
    border-color: var(--border-default);
    color: var(--text-faint);
    cursor: default;
  }

  .button.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--text-on-accent);
  }

  .button.primary:hover:not(:disabled) {
    filter: brightness(1.08);
  }

  .button.primary:disabled {
    opacity: 0.5;
  }

  /* Кнопка по умолчанию видна с кольцом фокуса (макет Д-А): Enter нажмёт её. */
  .button:focus {
    outline: none;
    box-shadow:
      0 0 0 2px var(--bg-sidebar),
      0 0 0 4px var(--accent);
  }

  .spin :global(.icon) {
    animation: turn 1s linear infinite;
  }

  .small {
    display: inline-grid;
  }

  @keyframes turn {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes slide {
    from {
      left: -30%;
    }

    to {
      left: 100%;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spin :global(.icon),
    .fill.unknown {
      animation: none;
    }
  }
</style>
