<!--
  Раздел «Редактор» (Э12.2б): ширина текста и размер шрифта. Режима заметки
  по умолчанию нет (решение 72): он переключается кнопками над заметкой.
-->
<script lang="ts">
  import type { Settings, TextWidth } from '../../api/index.ts';
  import { LIMITS } from '../../api/limits.ts';
  import { t } from '../../i18n/index.ts';
  import Icon from '../../shell/Icon.svelte';
  import { bindingsOf, chordText } from '../hotkeys.ts';
  import NumberField from '../NumberField.svelte';
  import Row from '../Row.svelte';
  import Segmented from '../Segmented.svelte';
  import Toggle from '../Toggle.svelte';

  let { settings, onChange }: { settings: Settings; onChange: (patch: Partial<Settings>) => void } = $props();

  const [minFont, maxFont] = LIMITS.font_size_range;
  const modeKey = $derived(bindingsOf(settings.hotkeys, 'editor.toggle-mode')[0]);
  const widths: readonly { value: TextWidth; label: string }[] = [
    { value: 'column', label: t('settings.textWidth.column') },
    { value: 'full', label: t('settings.textWidth.full') },
  ];
</script>

<p class="info">
  <Icon name="info" size={14} />
  <span>
    {modeKey
      ? t('settings.editor.modeInfo', { shortcut: chordText(modeKey) })
      : t('settings.editor.modeInfoNoKey')}
  </span>
</p>
<Row id="settings-text-width" title={t('settings.textWidth.title')} hint={t('settings.textWidth.hint')}>
  <Segmented
    labelledby="settings-text-width"
    value={settings.text_width}
    options={widths}
    onChange={(text_width) => onChange({ text_width })}
  />
</Row>
<Row
  title={t('settings.fontSize.title')}
  hint={t('settings.fontSize.hint', { min: minFont, max: maxFont })}
>
  <NumberField
    value={settings.font_size}
    min={minFont}
    max={maxFont}
    label={t('settings.fontSize.title')}
    unit={t('settings.fontSize.unit')}
    onChange={(font_size) => onChange({ font_size })}
  />
</Row>
<Row title={t('settings.mono.title')} hint={t('settings.mono.hint')} badge={t('settings.badge.nextRelease')} disabled>
  <Toggle checked={false} label={t('settings.mono.title')} disabled />
</Row>

<style>
  .info {
    display: flex;
    gap: 8px;
    margin: 4px 0 6px;
    padding: 10px 12px;
    border-radius: 6px;
    background: var(--bg-active);
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    line-height: 17px;
  }

  .info :global(.icon) {
    margin-top: 1px;
    color: var(--icon-default);
  }
</style>
