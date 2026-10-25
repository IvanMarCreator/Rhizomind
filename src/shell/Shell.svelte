<!--
  Оболочка окна по концепту К2 (docs/design/README.md, Э1): лента иконок,
  боковая панель, область заметки со строкой над ней, уведомления внизу.
  Заголовок окна — системный (свой заголовок требует настройки окна в ядре).
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { historyKeyAction } from '../core/history/history.ts';
  import { t } from '../i18n/index.ts';
  import { applyTheme } from '../theme/theme.ts';
  import Editor from '../editor/Editor.svelte';
  import SaveIndicator from '../editor/SaveIndicator.svelte';
  import type { SaveStatus } from '../editor/session.ts';
  import NoteTitle from '../tree/NoteTitle.svelte';
  import { FileTreeState, type TreeHost } from '../tree/tree.svelte.ts';
  import EmptyState from './EmptyState.svelte';
  import NoteHeader from './NoteHeader.svelte';
  import Notifications from './Notifications.svelte';
  import Ribbon from './Ribbon.svelte';
  import Sidebar from './Sidebar.svelte';
  import StartupError from './StartupError.svelte';
  import { WindowModel } from './window.svelte.ts';

  let { model = new WindowModel() }: { model?: WindowModel } = $props();
  let saveStatus = $state.raw<SaveStatus | null>(null);

  /** Редактор: сигналы «переименовано» и «удалено» доходят до него раньше, чем окно сменит путь. */
  let editor: Editor | undefined = $state();

  /** Дерево файлов окна: им же переименовывается заметка из заголовка. */
  const files = createFiles();

  function createFiles(): FileTreeState {
    const owner = model;
    const host: TreeHost = {
      get currentPath() {
        return owner.currentPath;
      },
      get confirmDelete() {
        return owner.confirmDelete;
      },
      open: (path) => owner.open(path),
      entryRenamed: (from, to) => {
        // Сначала редактор: смена пути после этого — не уход с заметки.
        editor?.renamed(from, to);
        owner.entryRenamed(from, to);
      },
      entryRemoved: (path) => {
        // Ввод удалённой заметки не пишется (решение владельца).
        editor?.removed(path);
        owner.entryRemoved(path);
      },
      notify: (text) => owner.notify(text),
    };
    return new FileTreeState(host);
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
    const action = historyKeyAction(event);
    if (!action) return;
    event.preventDefault();
    model.navigate(action);
  }
</script>

{#if model.status.kind === 'ready'}
  <div class="shell" data-testid="shell">
    <Ribbon filesOpen={model.sidebarOpen} onToggleFiles={() => model.toggleSidebar()} />
    {#if model.sidebarOpen}
      <Sidebar vault={model.vault} {files} />
    {/if}
    <main class="note">
      <NoteHeader
        path={model.currentPath}
        canGoBack={model.canGoBack}
        canGoForward={model.canGoForward}
        bind:mode={model.editorMode}
        onBack={() => model.back()}
        onForward={() => model.forward()}
      >
        {#snippet status()}
          <SaveIndicator status={saveStatus} />
        {/snippet}
      </NoteHeader>
      <div class="body">
        {#if !model.currentPath}
          <EmptyState />
        {:else}
          <!-- Заголовок над текстом; редактор (2.4) встаёт под ним в ту же колонку. -->
          <div class="column">
            <NoteTitle tree={files} path={model.currentPath} />
          </div>
        {/if}
        <Editor
          bind:this={editor}
          path={model.currentPath}
          showTitle={false}
          notify={(text) => model.notify(text)}
          onStatus={(status) => (saveStatus = status)}
        />
      </div>
    </main>
  </div>
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

  .column {
    max-width: var(--note-column-width);
    margin: 0 auto;
    padding: 28px 32px 0;
  }

  .loading {
    height: 100%;
  }
</style>
