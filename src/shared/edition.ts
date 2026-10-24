/**
 * Редакции Rhizomind.
 *
 * - secure    — закрытая редакция для компаний с жёсткой ИБ: без сети, без плагинов,
 *               данные только на локальном диске. Единственная реализованная сейчас.
 * - community — Open Core Community: плагины сообщества и контролируемый доступ к сети.
 *               Строится поверх того же ядра и того же API (src/shared/api.ts).
 * - server    — серверное развёртывание внутри контура компании с доступом участников.
 *
 * Редакция фиксируется при сборке. Политика редакции — единственное место,
 * где решается, что приложению разрешено.
 */
export type Edition = 'secure' | 'community' | 'server';

export interface EditionPolicy {
  /** Разрешены ли любые сетевые запросы из приложения. */
  readonly network: boolean;
  /** Разрешена ли загрузка плагинов. */
  readonly plugins: boolean;
}

export const EDITION_POLICIES: Readonly<Record<Edition, EditionPolicy>> = {
  secure: { network: false, plugins: false },
  community: { network: false, plugins: false }, // будет определено при разработке редакции
  server: { network: false, plugins: false }, // будет определено при разработке редакции
};

export const EDITION: Edition = 'secure';

export const POLICY: EditionPolicy = EDITION_POLICIES[EDITION];
