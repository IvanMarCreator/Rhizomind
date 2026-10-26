// Состояние окна «Управление хранилищами» (задача 2.8а, С3.1): список
// хранилищ и действия с ним. Компоненты только рисуют состояние.
//
// Хранилище открывается в своём окне, а окно управления после этого
// закрывает ядро (src-tauri/src/windows/mod.rs, open_vault). Родительскую
// папку нового хранилища и папку для «Открыть папку как хранилище»
// пользователь выбирает в системном окне ядра: интерфейс путей не задаёт.
import { CoreCallError, core } from '../api/index.ts';
import type { VaultListItem } from '../api/index.ts';
import { userMessage } from '../i18n/errors.ts';
import { t } from '../i18n/index.ts';

/** Правая часть окна: действия (Э11а, Э11б) или форма создания (Э11в). */
export type ManagerView = 'actions' | 'create';

/** Ошибки создания, которые относятся к введённому имени (пояснение под полем). */
function isNameError(error: unknown): boolean {
  if (!(error instanceof CoreCallError)) return false;
  const { code } = error.error;
  return code === 'invalid_name' || code === 'already_exists' || code === 'invalid_argument';
}

export class VaultManagerModel {
  vaults = $state<VaultListItem[]>([]);
  view = $state<ManagerView>('actions');
  /** Идёт вызов ядра (системное окно выбора папки, открытие): кнопки не жмутся дважды. */
  busy = $state(false);
  /** Пояснение под полем имени нового хранилища. */
  nameError = $state<string | null>(null);

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
    this.view = 'create';
  }

  showActions(): void {
    this.nameError = null;
    this.view = 'actions';
  }

  /** Имя изменилось: прежнее пояснение к нему больше не относится. */
  nameChanged(): void {
    this.nameError = null;
  }

  /** Создать хранилище `name`: родительскую папку спрашивает ядро. */
  async create(name: string): Promise<void> {
    await this.#run(async () => {
      try {
        const outcome = await core.vaultCreate({ name });
        if (outcome.outcome === 'opened') this.view = 'actions';
      } catch (error) {
        if (isNameError(error)) this.nameError = userMessage(error);
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
