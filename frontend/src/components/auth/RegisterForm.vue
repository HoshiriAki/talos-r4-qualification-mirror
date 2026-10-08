<script lang="ts">
export interface AccountRegistrationInput {
  tenantName: string
  username: string
  password: string
}
</script>

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
  submit: [input: AccountRegistrationInput]
}>()

const organizationName = ref('')
const username = ref('')
const password = ref('')
const confirmPassword = ref('')
const showPassword = ref(false)
const acceptedTerms = ref(false)
const error = ref('')
const fieldErrors = ref<Record<string, string>>({})

watch([organizationName, username, password, confirmPassword, acceptedTerms], () => {
  fieldErrors.value = {}
  error.value = ''
})

function submit() {
  const normalizedOrganizationName = organizationName.value.trim()
  const normalizedUsername = username.value.trim()

  const nextErrors: Record<string, string> = {}
  if (!normalizedOrganizationName) nextErrors.organization = '请输入组织名称。'
  if (!normalizedUsername) nextErrors.username = '请输入用户名。'
  if (!password.value) nextErrors.password = '请输入密码。'
  if (!confirmPassword.value) nextErrors.confirmation = '请再次输入密码。'
  if (password.value.length < 8) {
    nextErrors.password = '密码至少需要 8 个字符。'
  }
  if (confirmPassword.value && password.value !== confirmPassword.value) {
    nextErrors.confirmation = '两次输入的密码不一致。'
  }
  if (!acceptedTerms.value) {
    nextErrors.agreement = '请先阅读并同意服务条款。'
  }
  if (Object.keys(nextErrors).length) {
    fieldErrors.value = nextErrors
    error.value = Object.values(nextErrors)[0]
    return
  }

  error.value = ''
  emit('submit', {
    tenantName: normalizedOrganizationName,
    username: normalizedUsername,
    password: password.value,
  })
}
</script>

<template>
  <form class="register-form" aria-label="账号创建表单" @submit.prevent="submit">
    <div class="form-field">
      <label class="form-label" for="register-organization">组织名称</label>
      <input
        id="register-organization"
        v-model="organizationName"
        class="input"
        type="text"
        autocomplete="organization"
        placeholder="请输入企业或团队名称"
        :disabled="loading"
        :aria-invalid="fieldErrors.organization ? 'true' : undefined"
        :aria-describedby="fieldErrors.organization ? 'register-organization-error' : undefined"
      />
      <p v-if="fieldErrors.organization" id="register-organization-error" class="field-error">{{ fieldErrors.organization }}</p>
    </div>

    <div class="form-field">
      <label class="form-label" for="register-username">用户名</label>
      <input
        id="register-username"
        v-model="username"
        class="input"
        type="text"
        autocomplete="username"
        placeholder="请输入用户名"
        :disabled="loading"
        :aria-invalid="fieldErrors.username ? 'true' : undefined"
        :aria-describedby="fieldErrors.username ? 'register-username-error' : undefined"
      />
      <p v-if="fieldErrors.username" id="register-username-error" class="field-error">{{ fieldErrors.username }}</p>
    </div>

    <div class="form-field">
      <label class="form-label" for="register-password">密码</label>
      <div class="password-row">
        <input
          id="register-password"
          v-model="password"
          class="input"
          :type="showPassword ? 'text' : 'password'"
          autocomplete="new-password"
          placeholder="至少 8 个字符"
          :disabled="loading"
          :aria-invalid="fieldErrors.password ? 'true' : undefined"
          :aria-describedby="fieldErrors.password ? 'register-password-error' : undefined"
        />
        <button
          class="password-toggle"
          type="button"
          :disabled="loading"
          :aria-pressed="showPassword"
          @click="showPassword = !showPassword"
        >
          {{ showPassword ? '隐藏' : '显示' }}
        </button>
      </div>
      <p v-if="fieldErrors.password" id="register-password-error" class="field-error">{{ fieldErrors.password }}</p>
    </div>

    <div class="form-field">
      <label class="form-label" for="register-confirm-password">确认密码</label>
      <input
        id="register-confirm-password"
        v-model="confirmPassword"
        class="input"
        :type="showPassword ? 'text' : 'password'"
        autocomplete="new-password"
        placeholder="再次输入密码"
        :disabled="loading"
        :aria-invalid="fieldErrors.confirmation ? 'true' : undefined"
        :aria-describedby="fieldErrors.confirmation ? 'register-confirm-password-error' : undefined"
      />
      <p v-if="fieldErrors.confirmation" id="register-confirm-password-error" class="field-error">{{ fieldErrors.confirmation }}</p>
    </div>

    <label class="terms-field" for="register-terms">
      <input id="register-terms" v-model="acceptedTerms" type="checkbox" :disabled="loading" :aria-invalid="fieldErrors.agreement ? 'true' : undefined" :aria-describedby="fieldErrors.agreement ? 'register-terms-error' : undefined" />
      <span>我已阅读并同意服务条款</span>
    </label>
    <p v-if="fieldErrors.agreement" id="register-terms-error" class="field-error">{{ fieldErrors.agreement }}</p>

    <div v-if="error || submissionError" class="form-error" role="alert">{{ error || submissionError }}</div>

    <button class="btn btn-primary" type="submit" :disabled="loading">
      {{ loading ? '正在创建...' : '创建账号' }}
    </button>

    <RouterLink class="login-link" to="/login">返回登录</RouterLink>
  </form>
</template>

<style scoped>
.register-form { display: flex; flex-direction: column; gap: var(--space-m); }
.form-field { display: flex; flex-direction: column; gap: var(--space-2); }
.form-label,
.terms-field,
.login-link { font-family: var(--font-sans); font-size: var(--font-size-sm); }
.form-label { color: var(--text-secondary); }
.field-error { margin: 0; color: var(--accent); font-family: var(--font-sans); font-size: var(--font-size-sm); }
.input,
.btn,
.password-toggle { min-height: 44px; }
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
.terms-field { display: flex; min-height: 44px; align-items: center; gap: var(--space-2); color: var(--text-secondary); }
.terms-field input { accent-color: var(--accent); }
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
