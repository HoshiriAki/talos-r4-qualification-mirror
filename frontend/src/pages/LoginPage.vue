<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { fetchAndMergeRemoteSettings, resolveHomePage } from '@/utils/settings'
import AuthNotice from '@/components/auth/AuthNotice.vue'
import AuthShell from '@/components/auth/AuthShell.vue'
import LoginForm from '@/components/auth/LoginForm.vue'

const route = useRoute()
const router = useRouter()
const auth = useAuthStore()
const savedUsername = localStorage.getItem('talos-username') || ''

const safeRedirect = computed(() => {
  const raw = route.query.redirect
  return typeof raw === 'string' && raw.startsWith('/') && !raw.startsWith('//')
    ? raw
    : undefined
})

const notice = computed(() => {
  if (route.query.registered === '1') {
    return { tone: 'success' as const, message: '注册成功，请使用新账号登录。' }
  }
  if (route.query.reset === '1') {
    return { tone: 'success' as const, message: '密码已重置，请使用新密码登录。' }
  }
  return null
})

async function login(username: string, password: string, totpCode?: string) {
  const authority = route.path.startsWith('/control') ? 'platform' : 'tenant'
  await auth.login(username, password, authority, totpCode)
  localStorage.setItem('talos-username', username)

  // Authentication succeeded. Remote preferences are optional bootstrap data;
  // a temporary settings outage must not turn a valid login into an error.
  if (authority === 'tenant') {
    try {
      await fetchAndMergeRemoteSettings()
    } catch (error) {
      console.warn('[settings] remote settings unavailable after login', error)
    }
  }

  await router.replace(safeRedirect.value || (authority === 'platform' ? '/control/overview' : resolveHomePage()))
}
</script>

<template>
  <AuthShell
    module="MODULE.AUTH / 01"
    title="登录 TALOS"
    :subtitle="route.path.startsWith('/control') ? '使用平台成员身份进入控制平面' : '使用租户成员身份进入业务工作台'"
  >
    <template #notice>
      <AuthNotice v-if="notice" :tone="notice.tone" :message="notice.message" />
    </template>
    <LoginForm :on-login="login" :initial-username="savedUsername" />
  </AuthShell>
</template>
