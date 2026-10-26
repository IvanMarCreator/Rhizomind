// Настройки хранилища по умолчанию — те же, что `Settings::default()` в ядре
// (src-tauri/src/commands/settings.rs): ими окно работает, пока настройки не
// прочитаны или если их не удалось прочитать.
import type { Settings } from '../api/index.ts';

export const DEFAULT_SETTINGS: Settings = {
  theme: 'system',
  editor_mode: 'live_preview',
  text_width: 'column',
  font_size: 16,
  new_note_location: { kind: 'vault_root' },
  confirm_delete: true,
  max_edit_size_bytes: 10 * 1024 * 1024,
  hotkeys: {},
};
