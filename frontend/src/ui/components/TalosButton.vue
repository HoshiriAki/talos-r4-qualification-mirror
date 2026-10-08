<script setup lang="ts">
import { computed } from 'vue'
import type { TalosButtonSize, TalosButtonState, TalosButtonVariant } from '../component-types'

const props = withDefaults(defineProps<{
  label?: string
  executingLabel?: string
  variant?: TalosButtonVariant
  size?: TalosButtonSize
  state?: TalosButtonState
  type?: 'button' | 'submit' | 'reset'
}>(), {
  label: '执行操作',
  executingLabel: '执行中',
  variant: 'execution-primary',
  size: 'default',
  state: 'default',
  type: 'button',
})

const emit = defineEmits<{
  click: [event: MouseEvent]
}>()

const isDisabled = computed(() => props.state === 'disabled' || props.state === 'executing')
const displayLabel = computed(() => props.state === 'executing' ? props.executingLabel : props.label)

function handleClick(event: MouseEvent) {
  if (isDisabled.value) return
  emit('click', event)
}
</script>

<template>
  <button
    :type="type"
    class="talos-button"
    :class="[
      `talos-button--${variant}`,
      `talos-button--${size}`,
      `talos-button--${state}`,
    ]"
    :disabled="isDisabled"
    :aria-busy="state === 'executing'"
    @click="handleClick"
  >
    <span class="talos-button__signal" aria-hidden="true" data-lab-slot="leading-signal" />
    <span class="talos-button__content" data-lab-slot="primary-content">
      <slot>{{ displayLabel }}</slot>
    </span>
  </button>
</template>

<style scoped>
.talos-button {
  --talos-button-border: var(--border-strong);
  --talos-button-bg: transparent;
  --talos-button-text: var(--text-primary);
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 0;
  border: 1px solid var(--talos-button-border);
  border-radius: var(--radius-lg);
  background: var(--talos-button-bg);
  color: var(--talos-button-text);
  font-family: var(--font-mono);
  font-weight: 700;
  letter-spacing: 0.06em;
  line-height: 1;
  overflow: hidden;
  cursor: pointer;
  transition:
    border-color var(--duration-micro) var(--ease-micro),
    background var(--duration-micro) var(--ease-micro),
    color var(--duration-micro) var(--ease-micro),
    transform var(--duration-press) var(--ease-press);
}

.talos-button:focus-visible {
  outline: 2px solid var(--border-accent);
  outline-offset: 3px;
}

.talos-button:active:not(:disabled) {
  transform: scale(0.98);
}

.talos-button:disabled {
  cursor: not-allowed;
  opacity: 0.35;
}

.talos-button--small {
  min-height: 32px;
  padding-inline: 12px;
  font-size: var(--font-size-caption);
}

.talos-button--default {
  min-height: 38px;
  padding-inline: 16px;
  font-size: var(--font-size-xs);
}

.talos-button--large {
  min-height: 44px;
  padding-inline: 24px;
  font-size: var(--font-size-base);
}

.talos-button--execution-primary {
  --talos-button-border: var(--action-primary-border, var(--text-primary));
  --talos-button-bg: var(--action-primary-bg, var(--text-primary));
  --talos-button-text: var(--action-primary-text, var(--text-inverse));
}

.talos-button--secondary {
  --talos-button-border: var(--border-strong);
  --talos-button-bg: var(--bg-field);
}

.talos-button--danger {
  --talos-button-border: var(--status-error);
  --talos-button-bg: var(--accent-muted);
  --talos-button-text: var(--text-primary);
  border-width: 2px;
}

.talos-button--ghost {
  --talos-button-border: transparent;
  --talos-button-bg: transparent;
  --talos-button-text: var(--text-secondary);
}

.talos-button--secondary:hover:not(:disabled),
.talos-button--ghost:hover:not(:disabled) {
  --talos-button-border: var(--border-accent);
  --talos-button-text: var(--text-primary);
}

.talos-button__content {
  position: relative;
  z-index: 1;
}

.talos-button__signal {
  position: absolute;
  inset-block: 0;
  left: 0;
  width: 36%;
  background: var(--accent);
  transform: translateX(-140%) skewX(-18deg);
  opacity: 0;
  pointer-events: none;
  will-change: transform, opacity;
}

@media (hover: hover) and (pointer: fine) {
  .talos-button--execution-primary:hover:not(:disabled) .talos-button__signal {
    animation: talos-button-sweep 200ms cubic-bezier(0.23, 1, 0.32, 1) 1;
  }
}

.talos-button--executing .talos-button__signal {
  left: auto;
  right: 0;
  width: 4px;
  opacity: 1;
  transform: none;
  animation: none;
  will-change: auto;
}

@keyframes talos-button-sweep {
  0% { transform: translateX(-140%) skewX(-18deg); opacity: 0; }
  15% { opacity: 1; }
  100% { transform: translateX(310%) skewX(-18deg); opacity: 0; }
}

@media (prefers-reduced-motion: reduce) {
  .talos-button__signal {
    animation: none !important;
  }

  .talos-button {
    transition:
      border-color 150ms ease,
      background-color 150ms ease,
      color 150ms ease,
      opacity 150ms ease;
  }

  .talos-button:active:not(:disabled) {
    transform: none;
  }

  .talos-button--execution-primary:hover:not(:disabled) {
    box-shadow: inset 4px 0 0 var(--accent);
  }
}
</style>
