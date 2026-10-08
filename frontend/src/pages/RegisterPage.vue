<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import AuthShell from '@/components/auth/AuthShell.vue'
import RegisterForm from '@/components/auth/RegisterForm.vue'
import type { AccountRegistrationInput } from '@/components/auth/RegisterForm.vue'
import { authPageAdapters } from './authPageAdapters'

const router = useRouter()
const loading = ref(false)
const submissionError = ref('')

async function register(input: AccountRegistrationInput) {
  if (loading.value) return

  loading.value = true
  submissionError.value = ''
  try {
    await authPageAdapters.registerAccount(input)
    await router.replace({ name: 'Login', query: { registered: '1' } })
  } catch {
    submissionError.value = '暂时无法创建账号，请稍后重试。'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <AuthShell
    module="MODULE.AUTH / 02"
    title="创建账号"
    subtitle="创建 TALOS 工作台账号"
  >
    <RegisterForm :loading="loading" :submission-error="submissionError" @submit="register" />
  </AuthShell>
</template>
