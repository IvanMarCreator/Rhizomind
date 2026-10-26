<!-- Переключатель «вкл/выкл» (Э12): кнопка с ролью switch. -->
<script lang="ts">
  let {
    checked,
    label,
    disabled = false,
    onChange,
  }: { checked: boolean; label: string; disabled?: boolean; onChange?: (value: boolean) => void } = $props();
</script>

<button
  type="button"
  class="toggle"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onChange?.(!checked)}
>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    position: relative;
    width: 34px;
    height: 20px;
    border-radius: 10px;
    background: var(--border-strong);
    transition: background-color 0.15s;
  }

  .toggle[aria-checked='true'] {
    background: var(--accent);
  }

  .toggle:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .toggle:focus-visible {
    outline: none;
    box-shadow: var(--focus-ring);
  }

  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--control-knob);
    box-shadow: 0 1px 2px rgb(0 0 0 / 25%);
    transition: transform 0.15s;
  }

  .toggle[aria-checked='true'] .knob {
    transform: translateX(14px);
  }
</style>
