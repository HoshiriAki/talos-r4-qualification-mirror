<script setup lang="ts">
import AuthVisualField from './AuthVisualField.vue'

defineProps<{
  module: string
  title: string
  subtitle: string
}>()
</script>

<template>
  <main class="auth-shell">
    <section class="auth-visual-field" aria-hidden="true">
      <AuthVisualField />
    </section>
    <section class="auth-form-field">
      <div class="auth-form-frame">
        <div class="auth-form-heading">
          <span class="module-number">{{ module }}</span>
          <h1>{{ title }}</h1>
          <p>{{ subtitle }}</p>
        </div>
        <div class="auth-notice-slot">
          <slot name="notice" />
        </div>
        <slot />
      </div>
    </section>
  </main>
</template>

<style scoped>
.auth-shell {
  min-height: 100vh;
  display: grid;
  grid-template-columns: minmax(0, 7fr) minmax(360px, 5fr);
  background: var(--bg-base);
}
.auth-visual-field {
  min-width: 0;
  border-right: 1px solid var(--border-base);
  background: var(--bg-surface);
}
.auth-form-field {
  display: grid;
  place-items: center;
  padding: var(--space-8);
}
.auth-form-frame { width: min(100%, 480px); }
.auth-form-heading { margin-bottom: var(--space-8); }
.auth-form-heading h1 {
  margin: var(--space-s) 0 var(--space-2);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: clamp(var(--font-size-3xl), 4vw, var(--font-size-hero));
}
.auth-form-heading p {
  margin: 0;
  color: var(--text-secondary);
  font-family: var(--font-sans);
}
.auth-notice-slot:empty { display: none; }
.auth-notice-slot:not(:empty) { margin-bottom: var(--space-l); }
@media (max-width: 760px) {
  .auth-shell { display: block; }
  .auth-visual-field { display: none; }
  .auth-form-field { min-height: 100vh; padding: var(--space-l); }
}
</style>
