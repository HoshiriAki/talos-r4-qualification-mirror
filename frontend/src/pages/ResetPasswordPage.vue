<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AuthShell from '@/components/auth/AuthShell.vue'
import ResetPasswordForm from '@/components/auth/ResetPasswordForm.vue'
import type { ResetPasswordInput } from '@/components/auth/ResetPasswordForm.vue'
import { authPageAdapters } from './authPageAdapters'

const route = useRoute()
const router = useRouter()
const loading = ref(false)
const submissionError = ref('')

const initialUsername = computed(() => {
  const username = route.query.username
  return typeof username === 'string' ? username : ''
})

async function resetPassword(input: ResetPasswordInput) {
  if (loading.value) return

  loading.value = true
  submissionError.value = ''
  try {
    await authPageAdapters.completePasswordReset(input)
    await router.push({ name: 'Login', query: { reset: '1' } })
  } catch {
    submissionError.value = '暂时无法重置密码，请稍后重试。'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <AuthShell
    module="MODULE.AUTH / 04"
    title="重置密码"
    subtitle="设置用于后续登录的新密码"
  >
    <ResetPasswordForm
      :initial-username="initialUsername"
      :loading="loading"
      :submission-error="submissionError"
      @submit="resetPassword"
    />
  </AuthShell>
</template>
