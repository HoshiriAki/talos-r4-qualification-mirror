<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import AuthShell from '@/components/auth/AuthShell.vue'
import ForgotPasswordForm from '@/components/auth/ForgotPasswordForm.vue'
import { authPageAdapters } from './authPageAdapters'

const router = useRouter()
const loading = ref(false)
const submissionError = ref('')

async function recoverPassword(username: string) {
  if (loading.value) return

  loading.value = true
  submissionError.value = ''
  try {
    await authPageAdapters.beginPasswordRecovery(username)
    await router.push({ name: 'ResetPassword', query: { username } })
  } catch {
    submissionError.value = '暂时无法开始密码找回，请稍后重试。'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <AuthShell
    module="MODULE.AUTH / 03"
    title="找回密码"
    subtitle="输入用户名以继续重置流程"
  >
    <ForgotPasswordForm :loading="loading" :submission-error="submissionError" @submit="recoverPassword" />
  </AuthShell>
</template>
