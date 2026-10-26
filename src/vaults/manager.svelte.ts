// Состояние окна «Управление хранилищами» (задача 2.8а, С3.1): список
// хранилищ и действия с ним. Компоненты только рисуют состояние.
//
// Хранилище открывается в своём окне, а окно управления после этого
// закрывает ядро (src-tauri/src/windows/mod.rs, open_vault). Родительскую
// папку нового хранилища («Обзор…», `folder_pick`) и папку для «Открыть папку
// как хранилище» пользователь выбирает в системном окне ядра; интерфейс
// только показывает выбранный путь и возвращает его ядру (`vault_create`
// примет лишь папку, выбранную в окне ядра).
import { CoreCallError, core } from '../api/index.ts';
import type { VaultListItem } from '../api/index.ts';
import { userMessage } from '../i18n/errors.ts';
import { t } from '../i18n/index.ts';

/** Правая часть окна: действия (Э11а, Э11б) или форма создания (Э11в). */
export type ManagerView = 'actions' | 'create';

/** К какому полю формы создания относится ошибка ядра; `null` — ни к какому (уведомление). */
function fieldOf(error: unknown): 'name' | 'parent' | null {
  if (!(error instanceof CoreCallError) || error.error.code === 'ipc_failed') return null;
  const failure = error.error;
  switch (failure.code) {
    case 'invalid_name':
    case 'already_exists':
      return 'name';
    case 'nested_vault':
    case 'vault_unavailable':
    case 'invalid_path':
      return 'parent';
    case 'invalid_argument':
      return failure.field === 'parent' ? 'parent' : 'name';
    default:
      return null;
  }
}

export class VaultManagerModel {
  vaults = $state<VaultListItem[]>([]);
  view = $state<ManagerView>('actions');
  /** Идёт вызов ядра (системное окно выбора папки, открытие): кнопки не жмутся дважды. */
  busy = $state(false);
  /** Пояснение под полем имени нового хранилища. */
  nameError = $state<string | null>(null);
  /** Родительская папка нового хранилища, выбранная в окне ядра («Обзор…»). */
  parent = $state<string | null>(null);
  /** Пояснение под полем «Где создать». */
  parentError = $state<string | null>(null);

  readonly #notify: (text: string) => void;

  constructor(notify: (text: string) => void) {
    this.#notify = notify;
  }

  async load(): Promise<void> {
    try {
      this.vaults = (await core.vaultList()).vaults;
    } catch (error) {
      this.#notify(userMessage(error));
    }
  }

  /** Открыть хранилище из списка: новое окно или фокус на открытое. */
  async open(id: string): Promise<void> {
    await this.#run(async () => {
      try {
        await core.vaultOpen({ id });
      } catch (error) {
        this.#notify(userMessage(error));
        // Хранилище могли убрать из списка в другом окне — список свежий.
        await this.load();
      }
    });
  }

  /** «Убрать из списка»: файлы на диске остаются. */
  async remove(vault: VaultListItem): Promise<void> {
    try {
      await core.vaultRemoveFromList({ id: vault.id });
      this.#notify(t('vaults.manager.removed', { name: vault.name }));
    } catch (error) {
      this.#notify(userMessage(error));
    }
    await this.load();
  }

  /** «Открыть папку как хранилище»: папку выбирают в системном окне ядра. */
  async openFolder(): Promise<void> {
    await this.#run(async () => {
      try {
        await core.vaultOpenFolder();
      } catch (error) {
        this.#notify(userMessage(error));
      }
      await this.load();
    });
  }

  showCreate(): void {
    this.nameError = null;
    this.parentError = null;
    this.parent = null;
    this.view = 'create';
  }

  showActions(): void {
    this.nameError = null;
    this.parentError = null;
    this.view = 'actions';
  }

  /** «Обзор…»: родительская папка в системном окне ядра. */
  async pickParent(): Promise<void> {
    await this.#run(async () => {
      try {
        const outcome = await core.folderPick();
        if (outcome.outcome === 'picked') {
          this.parent = outcome.path;
          this.parentError = null;
        }
      } catch (error) {
        if (fieldOf(error) === 'parent') {
          this.parent = null;
          this.parentError = userMessage(error);
        } else this.#notify(userMessage(error));
      }
    });
  }

  /** Имя изменилось: прежнее пояснение к нему больше не относится. */
  nameChanged(): void {
    this.nameError = null;
  }

  /** Создать хранилище `name` в выбранной родительской папке. */
  async create(name: string): Promise<void> {
    const folder = this.parent;
    if (folder === null) return;
    await this.#run(async () => {
      try {
        const outcome = await core.vaultCreate({ name, parent: folder });
        if (outcome.outcome === 'opened') this.view = 'actions';
      } catch (error) {
        const field = fieldOf(error);
        if (field === 'name') this.nameError = userMessage(error);
        else if (field === 'parent') this.parentError = userMessage(error);
        else this.#notify(userMessage(error));
      }
      await this.load();
    });
  }

  async #run(work: () => Promise<void>): Promise<void> {
    if (this.busy) return;
    this.busy = true;
    try {
      await work();
    } finally {
      this.busy = false;
    }
  }
}
