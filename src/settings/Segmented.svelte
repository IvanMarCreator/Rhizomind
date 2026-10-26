<!-- Выбор одного из вариантов кнопками в общей рамке (Э12.2б: ширина текста). -->
<script lang="ts" generics="T extends string">
  let {
    value,
    options,
    labelledby,
    disabled = false,
    onChange,
  }: {
    value: T | null;
    options: readonly { value: T; label: string }[];
    /** id названия строки (aria-labelledby). */
    labelledby: string;
    disabled?: boolean;
    onChange?: (value: T) => void;
  } = $props();
</script>

<div class="segmented" role="radiogroup" aria-labelledby={labelledby} aria-disabled={disabled ? 'true' : undefined}>
  {#each options as option (option.value)}
    <button
      type="button"
      role="radio"
      class="option"
      aria-checked={option.value === value}
      {disabled}
      onclick={() => onChange?.(option.value)}
    >
      {option.label}
    </button>
  {/each}
</div>

<style>
  .segmented {
    display: flex;
    padding: 2px;
    border-radius: 6px;
    background: var(--bg-active);
  }

  .option {
    padding: 4px 10px;
    border-radius: 4px;
    color: var(--text-secondary);
    font-size: var(--font-size-caption);
    white-space: nowrap;
  }

  .option[aria-checked='true'] {
    background: var(--bg-popover);
    color: var(--text-primary);
    font-weight: 500;
    box-shadow: 0 1px 2px rgb(0 0 0 / 12%);
  }

  .option:disabled {
    color: var(--text-faint);
    cursor: default;
  }

  .option:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }
</style>
