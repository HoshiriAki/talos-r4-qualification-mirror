<script setup lang="ts">
import { ref, computed } from 'vue'

const props = defineProps<{
  onLogin: (u: string, p: string, totpCode?: string) => Promise<void>
  initialUsername?: string
}>()

const username = ref(props.initialUsername || '')
const password = ref('')
const totpCode = ref('')
const mfaRequired = ref(false)
const showPwd = ref(false)
const loading = ref(false)
const error = ref('')

const canSubmit = computed(() => username.value.trim() && password.value && !loading.value)

async function submit() {
  if (!canSubmit.value) return
  loading.value = true; error.value = ''
  try {
    await props.onLogin(username.value.trim(), password.value, totpCode.value.trim() || undefined)
    // Redirect handled by caller
  } catch (e: any) {
    if (e?.code === 'AUTH_MFA_REQUIRED' || e?.code === 'AUTH_INVALID_MFA') {
      mfaRequired.value = true
    }
    error.value = e?.status === 429
      ? '登录尝试过于频繁，请 30 秒后重试'
      : (e?.message || '登录失败')
  } finally { loading.value = false }
}
</script>

<template>
  <form class="login-form" @submit.prevent="submit" aria-label="登录表单">
    <div class="form-field">
      <label class="form-label" for="login-user">用户名</label>
      <input
        id="login-user" v-model="username" type="text"
        class="input" placeholder="请输入用户名"
        :disabled="loading" autocomplete="username"
      />
    </div>
    <div v-if="mfaRequired" class="form-field">
      <label class="form-label" for="login-totp">TOTP 验证码</label>
      <input
        id="login-totp" v-model="totpCode" type="text"
        class="input" inputmode="numeric" pattern="[0-9]{6}" maxlength="6"
        placeholder="000000" :disabled="loading" autocomplete="one-time-code"
      />
    </div>
    <div class="form-field">
      <label class="form-label" for="login-pass">密码</label>
      <div class="pass-row">
        <input
          id="login-pass" v-model="password"
          :type="showPwd ? 'text' : 'password'"
          class="input" placeholder="请输入密码"
          :disabled="loading" autocomplete="current-password"
        />
        <button type="button" class="pass-toggle" @click="showPwd = !showPwd">{{ showPwd ? '隐藏' : '显示' }}</button>
      </div>
    </div>
    <div v-if="error" class="form-error" role="alert">{{ error }}</div>
    <button type="submit" class="btn btn-primary" :disabled="!canSubmit" style="width:100%">
      {{ loading ? '登录中...' : '登录' }}
    </button>
    <RouterLink class="recovery-link" to="/forgot-password">忘记密码？</RouterLink>
    <RouterLink class="register-link" to="/register">创建账号</RouterLink>
  </form>
</template>

<style scoped>
.login-form { display: flex; flex-direction: column; gap: 16px; }
.form-field { display: flex; flex-direction: column; gap: 6px; }
.form-label { font-family: 'Inter', sans-serif; font-size: 13px; color: var(--text-secondary); }
.input,
.btn,
.pass-toggle { min-height: 44px; }
.pass-row { display: flex; gap: 8px; }
.pass-toggle { min-width: 44px; background: none; border: none; font-family: 'Space Mono', monospace; font-size: 11px; color: var(--text-tertiary); cursor: pointer; white-space: nowrap; }
.form-error { font-family: 'Inter', sans-serif; font-size: 12px; color: var(--accent); padding: 8px 12px; border: 1px solid var(--accent); border-radius: 8px; background: var(--accent-muted); }
.recovery-link,
.register-link { align-self: center; font-family: var(--font-sans); font-size: var(--font-size-sm); color: var(--text-secondary); }
.recovery-link,
.register-link { display: inline-flex; min-height: 44px; align-items: center; }
.recovery-link:hover,
.register-link:hover { color: var(--accent); }
</style>
