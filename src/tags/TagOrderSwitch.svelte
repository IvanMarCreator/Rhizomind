<!--
  Переключатель порядка тегов «по имени / по числу заметок» (0.2-2.3, макет
  П-2): стоит в заголовке правой панели справа от «ТЕГИ N» (сниппет
  `headerActions` панели, подключение — Shell.svelte). Порядок хранится в
  состоянии панели и пишется с раскладкой (TagsModel.setOrder).
-->
<script lang="ts">
  import type { TagOrder } from '../core/layout/layout.ts';
  import { t, type MessageKey } from '../i18n/index.ts';
  import Icon from '../shell/Icon.svelte';
  import type { TagsModel } from './model.svelte.ts';

  let { model }: { model: Pick<TagsModel, 'order' | 'setOrder'> } = $props();

  const ORDERS: { order: TagOrder; label: MessageKey; icon: 'arrow-down-a-z' | 'arrow-down-wide-narrow' }[] = [
    { order: 'name', label: 'tags.order.name', icon: 'arrow-down-a-z' },
    { order: 'count', label: 'tags.order.count', icon: 'arrow-down-wide-narrow' },
  ];
</script>

<div class="order" role="group" aria-label={t('tags.order')}>
  {#each ORDERS as item (item.order)}
    <button
      type="button"
      class="order-button"
      class:active={model.order === item.order}
      aria-pressed={model.order === item.order}
      aria-label={t(item.label)}
      title={t(item.label)}
      onclick={() => model.setOrder(item.order)}
    >
      <Icon name={item.icon} size={14} />
    </button>
  {/each}
</div>

<style>
  /* tokens.md «0.2»: сегментный, сегменты 24 × 22, иконка 14. */
  .order {
    display: flex;
    flex: none;
    gap: 2px;
    padding: 2px;
    border-radius: 7px;
    background: var(--bg-hover);
  }

  .order-button {
    display: grid;
    place-items: center;
    width: 24px;
    height: 22px;
    border: 1px solid transparent;
    border-radius: 5px;
    color: var(--icon-muted);
    box-sizing: border-box;
  }

  .order-button:hover:not(.active) {
    color: var(--icon-default);
    cursor: pointer;
  }

  .order-button.active {
    border-color: var(--border-default);
    background: var(--bg-app);
    color: var(--text-primary);
  }

  .order-button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
</style>
