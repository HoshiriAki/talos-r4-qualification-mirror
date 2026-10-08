<script lang="ts">
export interface ResetPasswordInput {
  username: string
  testChallenge: string
  newPassword: string
}
</script>

<script setup lang="ts">
import { ref, watch } from 'vue'

const props = withDefaults(defineProps<{
  initialUsername: string
  loading?: boolean
  submissionError?: string
}>(), {
  loading: false,
  submissionError: '',
})

const emit = defineEmits<{
  submit: [input: ResetPasswordInput]
}>()

const testChallenge = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const showNewPassword = ref(false)
const showConfirmation = ref(false)
const error = ref('')
const fieldErrors = ref<Record<string, string>>({})

watch([testChallenge, newPassword, confirmPassword], () => {
  fieldErrors.value = {}
  error.value = ''
})

function submit() {
  const username = props.initialUsername.trim()
  const normalizedChallenge = testChallenge.value.trim()

  const nextErrors: Record<string, string> = {}
  if (!username) nextErrors.username = '缺少用户名，请返回找回密码页面重新开始。'
  if (!normalizedChallenge) nextErrors.challenge = '请输入测试挑战字段。'
  if (newPassword.value.length < 8) {
    nextErrors.password = '新密码至少需要 8 个字符。'
  }
  if (!confirmPassword.value || newPassword.value !== confirmPassword.value) {
    nextErrors.confirmation = confirmPassword.value ? '两次输入的密码不一致。' : '请再次输入新密码。'
  }
  if (Object.keys(nextErrors).length) {
    fieldErrors.value = nextErrors
    error.value = Object.values(nextErrors)[0]
    return
  }

  error.value = ''
  emit('submit', {
    username,
    testChallenge: normalizedChallenge,
    newPassword: newPassword.value,
  })
}
</script>

<template>
  <form class="reset-form" aria-label="密码重置表单" @submit.prevent="submit">
    <div class="form-field">
      <label class="form-label" for="reset-username">用户名</label>
      <input
        id="reset-username"
        class="input"
        type="text"
        autocomplete="username"
        :value="initialUsername"
        readonly
        :disabled="loading"
        :aria-invalid="fieldErrors.username ? 'true' : undefined"
        :aria-describedby="fieldErrors.username ? 'reset-username-error' : undefined"
      />
      <p v-if="fieldErrors.username" id="reset-username-error" class="field-error">{{ fieldErrors.username }}</p>
    </div>

    <div class="form-field">
      <label class="form-label" for="reset-test-challenge">测试挑战字段</label>
      <input
        id="reset-test-challenge"
        v-model="testChallenge"
        class="input"
        type="text"
        autocomplete="off"
        :aria-invalid="fieldErrors.challenge ? 'true' : undefined"
        :aria-describedby="fieldErrors.challenge ? 'reset-test-challenge-help reset-test-challenge-error' : 'reset-test-challenge-help'"
        placeholder="仅用于测试发布"
        :disabled="loading"
      />
      <p id="reset-test-challenge-help" class="field-help">
        测试发布占位字段：不会进行任何真实身份验证。
      </p>
      <p v-if="fieldErrors.challenge" id="reset-test-challenge-error" class="field-error">{{ fieldErrors.challenge }}</p>
    </div>

    <div class="form-field">
      <label class="form-label" for="reset-new-password">新密码</label>
      <div class="password-row">
        <input
          id="reset-new-password"
          v-model="newPassword"
          class="input"
          :type="showNewPassword ? 'text' : 'password'"
          autocomplete="new-password"
          placeholder="至少 8 个字符"
          :disabled="loading"
          :aria-invalid="fieldErrors.password ? 'true' : undefined"
          :aria-describedby="fieldErrors.password ? 'reset-new-password-error' : undefined"
        />
        <button
          class="password-toggle"
          type="button"
          :disabled="loading"
          :aria-pressed="showNewPassword"
          :aria-label="showNewPassword ? '隐藏新密码' : '显示新密码'"
          @click="showNewPassword = !showNewPassword"
        >
          {{ showNewPassword ? '隐藏' : '显示' }}
        </button>
      </div>
      <p v-if="fieldErrors.password" id="reset-new-password-error" class="field-error">{{ fieldErrors.password }}</p>
    </div>

    <div class="form-field">
      <label class="form-label" for="reset-confirm-password">确认新密码</label>
      <div class="password-row">
        <input
          id="reset-confirm-password"
          v-model="confirmPassword"
          class="input"
          :type="showConfirmation ? 'text' : 'password'"
          autocomplete="new-password"
          placeholder="再次输入新密码"
          :disabled="loading"
          :aria-invalid="fieldErrors.confirmation ? 'true' : undefined"
          :aria-describedby="fieldErrors.confirmation ? 'reset-confirm-password-error' : undefined"
        />
        <button
          class="password-toggle"
          type="button"
          :disabled="loading"
          :aria-pressed="showConfirmation"
          :aria-label="showConfirmation ? '隐藏确认密码' : '显示确认密码'"
          @click="showConfirmation = !showConfirmation"
        >
          {{ showConfirmation ? '隐藏' : '显示' }}
        </button>
      </div>
      <p v-if="fieldErrors.confirmation" id="reset-confirm-password-error" class="field-error">{{ fieldErrors.confirmation }}</p>
    </div>

    <div v-if="error || submissionError" class="form-error" role="alert">{{ error || submissionError }}</div>

    <button class="btn btn-primary" type="submit" :disabled="loading">
      {{ loading ? '正在完成...' : '完成密码重置' }}
    </button>

    <RouterLink class="recovery-link" to="/forgot-password">返回找回密码</RouterLink>
  </form>
</template>

<style scoped>
.reset-form { display: flex; flex-direction: column; gap: var(--space-m); }
.form-field { display: flex; flex-direction: column; gap: var(--space-2); }
.form-label,
.field-help,
.recovery-link { font-family: var(--font-sans); font-size: var(--font-size-sm); }
.form-label { color: var(--text-secondary); }
.field-error { margin: 0; color: var(--accent); font-family: var(--font-sans); font-size: var(--font-size-sm); }
.input,
.btn,
.password-toggle { min-height: 44px; }
.field-help { margin: 0; color: var(--text-tertiary); line-height: 1.5; }
.password-row { display: flex; gap: var(--space-2); }
.password-row .input { flex: 1; min-width: 0; }
.password-toggle {
  min-width: 44px;
  border: 0;
  background: transparent;
  color: var(--text-tertiary);
  font-family: var(--font-mono);
  font-size: var(--font-size-xs);
  cursor: pointer;
}
.password-toggle:hover:not(:disabled) { color: var(--accent); }
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
.recovery-link { display: inline-flex; min-height: 44px; align-self: center; align-items: center; color: var(--text-secondary); }
.recovery-link:hover { color: var(--accent); }
</style>
