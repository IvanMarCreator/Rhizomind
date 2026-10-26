<!--
  Целое число в пределах (размер шрифта, лимит в МБ). Значение применяется по
  Enter или при уходе из поля; вне пределов или не число — прежнее значение.
-->
<script lang="ts">
  let {
    value,
    min,
    max,
    label,
    unit,
    onChange,
  }: { value: number; min: number; max: number; label: string; unit: string; onChange: (value: number) => void } =
    $props();

  let input: HTMLInputElement | undefined = $state();

  function commit() {
    if (!input) return;
    const next = Number(input.value);
    if (Number.isInteger(next) && next >= min && next <= max) {
      if (next !== value) onChange(next);
    } else {
      input.value = String(value);
    }
  }
</script>

<input
  bind:this={input}
  class="number"
  type="number"
  inputmode="numeric"
  {min}
  {max}
  step="1"
  value={String(value)}
  aria-label={label}
  onchange={commit}
  onkeydown={(event) => {
    if (event.key === 'Enter') commit();
  }}
/>
<span class="unit">{unit}</span>

<style>
  .number {
    width: 64px;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--bg-app);
    color: var(--text-primary);
    font: inherit;
    text-align: right;
    box-sizing: border-box;
  }

  .number:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: var(--focus-ring);
  }

  .unit {
    min-width: 18px;
    color: var(--text-muted);
    font-size: var(--font-size-caption);
  }
</style>
