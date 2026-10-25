<!--
  Оболочка окна по концепту К2 (docs/design/README.md, Э1): лента иконок,
  боковая панель, область заметки со строкой над ней, уведомления внизу.
  Заголовок окна — системный (свой заголовок требует настройки окна в ядре).
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { historyKeyAction } from '../core/history/history.ts';
  import { t } from '../i18n/index.ts';
  import { isQuickSwitcherKey } from '../quick-switcher/keys.ts';
  import QuickSwitcher from '../quick-switcher/QuickSwitcher.svelte';
  import { applyTheme } from '../theme/theme.ts';
  import EmptyState from './EmptyState.svelte';
  import NoteHeader from './NoteHeader.svelte';
  import Notifications from './Notifications.svelte';
  import Ribbon from './Ribbon.svelte';
  import Sidebar from './Sidebar.svelte';
  import StartupError from './StartupError.svelte';
  import { WindowModel } from './window.svelte.ts';

  let { model = new WindowModel() }: { model?: WindowModel } = $props();

  /** Окно быстрого перехода (Ctrl+O, задача 2.7) открыто. */
  let switcherOpen = $state(false);

  function openSwitcher() {
    if (model.status.kind === 'ready' && model.vault) switcherOpen = true;
  }

  onMount(() => {
    void model.start();
    // Не <svelte:window>: Svelte передаёт тогда объект window значением, и
    // проверка сети (tools/check-no-network) не может доказать безопасность.
    window.addEventListener('keydown', onKeydown);
    return () => window.removeEventListener('keydown', onKeydown);
  });

  $effect(() => applyTheme(model.theme));

  function onKeydown(event: KeyboardEvent) {
    if (model.status.kind !== 'ready') return;
    if (isQuickSwitcherKey(event)) {
      event.preventDefault();
      openSwitcher();
      return;
    }
    // Под модальным окном быстрого перехода история не листается.
    if (switcherOpen) return;
    const action = historyKeyAction(event);
    if (!action) return;
    event.preventDefault();
    model.navigate(action);
  }
</script>

{#if model.status.kind === 'ready'}
  <div class="shell" data-testid="shell">
    <Ribbon filesOpen={model.sidebarOpen} onToggleFiles={() => model.toggleSidebar()} onQuickSwitcher={openSwitcher} />
    {#if model.sidebarOpen}
      <Sidebar vault={model.vault} />
    {/if}
    <main class="note">
      <NoteHeader
        path={model.currentPath}
        canGoBack={model.canGoBack}
        canGoForward={model.canGoForward}
        bind:mode={model.editorMode}
        onBack={() => model.back()}
        onForward={() => model.forward()}
      />
      <div class="body">
        {#if !model.currentPath}
          <EmptyState />
        {/if}
      </div>
    </main>
  </div>
  {#if switcherOpen}
    <QuickSwitcher
      currentPath={model.currentPath}
      onOpen={(path) => model.open(path)}
      onClose={() => (switcherOpen = false)}
      onNotice={(text) => model.notify(text)}
    />
  {/if}
{:else if model.status.kind === 'failed'}
  <StartupError title={model.status.title} message={model.status.message} />
{:else}
  <div class="loading" aria-busy="true" aria-label={t('startup.loading')}></div>
{/if}

<Notifications notices={model.notices} onDismiss={(id) => model.dismiss(id)} />

<style>
  .shell {
    display: flex;
    height: 100%;
  }

  .note {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    background: var(--bg-app);
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .loading {
    height: 100%;
  }
</style>
