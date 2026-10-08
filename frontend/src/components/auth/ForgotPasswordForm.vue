<script setup lang="ts">
import { ref, watch } from 'vue'

withDefaults(defineProps<{
  loading?: boolean
  submissionError?: string
}>(), {
  loading: false,
  submissionError: '',
})

const emit = defineEmits<{
  submit: [username: string]
}>()

const username = ref('')
const error = ref('')
const usernameError = ref('')

watch(username, () => {
  usernameError.value = ''
  error.value = ''
})

function submit() {
  const normalizedUsername = username.value.trim()

  if (!normalizedUsername) {
    usernameError.value = '请输入用户名。'
    error.value = usernameError.value
    return
  }

  error.value = ''
  emit('submit', normalizedUsername)
}
</script>

<template>
  <form class="recovery-form" aria-label="密码找回表单" @submit.prevent="submit">
    <div class="form-field">
      <label class="form-label" for="recovery-username">用户名</label>
      <input
        id="recovery-username"
        v-model="username"
        class="input"
        type="text"
        autocomplete="username"
        placeholder="请输入用户名"
        :disabled="loading"
        :aria-invalid="usernameError ? 'true' : undefined"
        :aria-describedby="usernameError ? 'recovery-username-error' : undefined"
      />
      <p v-if="usernameError" id="recovery-username-error" class="field-error">{{ usernameError }}</p>
    </div>

    <div v-if="error || submissionError" class="form-error" role="alert">{{ error || submissionError }}</div>

    <button class="btn btn-primary" type="submit" :disabled="loading">
      {{ loading ? '正在处理...' : '继续' }}
    </button>

    <RouterLink class="login-link" to="/login">返回登录</RouterLink>
  </form>
</template>

<style scoped>
.recovery-form { display: flex; flex-direction: column; gap: var(--space-m); }
.form-field { display: flex; flex-direction: column; gap: var(--space-2); }
.form-label,
.login-link { font-family: var(--font-sans); font-size: var(--font-size-sm); }
.form-label { color: var(--text-secondary); }
.field-error { margin: 0; color: var(--accent); font-family: var(--font-sans); font-size: var(--font-size-sm); }
.input,
.btn { min-height: 44px; }
.form-error {
  padding: var(--space-2) var(--space-s);
  border: 1px solid var(--accent);
  border-radius: var(--radius-sm);
  background: var(--accent-muted);
  color: var(--accent);
  font-family: var(--font-sans);
  font-size: var(--font-size-sm);
}
.btn { width: 100%; }
.login-link { display: inline-flex; min-height: 44px; align-self: center; align-items: center; color: var(--text-secondary); }
.login-link:hover { color: var(--accent); }
</style>
