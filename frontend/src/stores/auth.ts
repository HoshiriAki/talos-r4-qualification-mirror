import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import * as authApi from '@/api/auth'
import type { AuthUser, PlatformCapability, PlatformRole, TenantRole } from '@/api/auth'

export const useAuthStore = defineStore('auth', () => {
  const user = ref<AuthUser | null>(null)
  const loading = ref(false)
  const initialized = ref(false)
  const initializedAuthority = ref<'tenant' | 'platform' | null>(null)

  const isAuthenticated = computed(() => !!user.value)
  const isTenantAuthority = computed(() => user.value?.authority.kind === 'tenant')
  const isPlatformAuthority = computed(() => user.value?.authority.kind === 'platform')
  const tenantRole = computed<TenantRole | null>(() => user.value?.authority.kind === 'tenant' ? user.value.authority.role : null)
  const platformRoles = computed<PlatformRole[]>(() => user.value?.authority.kind === 'platform' ? user.value.authority.roles : [])
  const isTenantAdmin = computed(() => tenantRole.value === 'admin' || tenantRole.value === 'owner')
  const isTenantOwner = computed(() => tenantRole.value === 'owner')
  const currentUser = computed(() => user.value)

  function hasCapability(capability: PlatformCapability): boolean {
    return user.value?.capabilities.includes(capability) ?? false
  }

  const initError = ref<string | null>(null)
  let initPromise: Promise<void> | null = null

  async function init(authority: 'tenant' | 'platform' = 'tenant', force = false) {
    if (!force && initialized.value && initializedAuthority.value === authority) return
    if (initPromise) await initPromise
    if (!force && initialized.value && initializedAuthority.value === authority) return

    initPromise = (async () => {
      loading.value = true
      initError.value = null
      try {
        const res = await fetch('/auth/me', {
          credentials: 'same-origin',
          headers: authority === 'platform' ? { 'X-Talos-Authority': 'platform' } : undefined,
        })
        if (res.status === 200) {
          const data = await res.json()
          user.value = data.user
          initialized.value = true
          initializedAuthority.value = authority
        } else if (res.status === 401) {
          user.value = null
          initialized.value = true
          initializedAuthority.value = authority
        } else {
          initError.value = `登录状态检查失败 (${res.status})`
        }
      } catch (e: any) {
        initError.value = e.message || '网络连接失败'
      } finally {
        loading.value = false
        initPromise = null
      }
    })()

    return initPromise
  }

  async function login(username: string, password: string, authority: 'tenant' | 'platform' = 'tenant', totpCode?: string) {
    loading.value = true
    try {
      const data = await authApi.login(username, password, authority, totpCode)
      user.value = data.user
      initialized.value = true
      initializedAuthority.value = authority
    } finally {
      loading.value = false
    }
  }

  async function logout() {
    const wasPlatform = isPlatformAuthority.value
    try {
      await authApi.logout()
    } catch {
      // always clear state
    } finally {
      user.value = null
      initialized.value = false
      initializedAuthority.value = null
    }
    const { default: router } = await import('@/router')
    router.push(wasPlatform ? '/control/login' : '/login')
  }

  async function changePassword(oldPassword: string, newPassword: string) {
    await authApi.changePassword(oldPassword, newPassword)
  }

  async function updateProfile(input: authApi.UpdateProfileInput) {
    const data = await authApi.updateProfile(input)
    user.value = data.user
    return data
  }

  return {
    user, loading, initialized, initializedAuthority, initError,
    isAuthenticated, isTenantAuthority, isPlatformAuthority, tenantRole, platformRoles,
    isTenantAdmin, isTenantOwner, currentUser, hasCapability,
    init, login, logout, changePassword,
    updateProfile,
  }
})

// ═══════════════════════════════════════════════════════════════════════════
// Dev bypass — 控制台调用 __dev_bypass_auth(role?) 跳过后端登录校验
// 仅在 Vite dev server (import.meta.env.DEV) 下可用，生产构建自动移除
// ═══════════════════════════════════════════════════════════════════════════
if (import.meta.env.DEV) {
  (window as any).__dev_bypass_auth = (role: TenantRole | PlatformRole = 'platform_owner') => {
    const store = useAuthStore()
    const platform = role.startsWith('platform_') || ['support_engineer', 'business_operator', 'security_auditor'].includes(role)
    store.$patch({
      user: {
        id: 'dev-identity-001',
        username: `dev-${role}`,
        displayName: `DEV ${role}`,
        email: 'dev@talos.local',
        phone: '',
        authority: platform
          ? { kind: 'platform', membership_id: 'dev-platform-membership', roles: [role as PlatformRole] }
          : { kind: 'tenant', membership_id: 'dev-tenant-membership', tenant_id: 'dev-tenant-001', role: role as TenantRole },
        capabilities: platform ? [
          'platform_overview_read', 'platform_health_read', 'platform_operations_manage',
          'tenant_list', 'tenant_read', 'tenant_create', 'tenant_update', 'tenant_suspend', 'tenant_delete',
          'tenant_governance_read', 'tenant_governance_manage', 'tenant_preview_create', 'tenant_preview_read',
          'tenant_diagnostics_read', 'tenant_simulation_create', 'tenant_simulation_read', 'tenant_simulation_discard',
          'business_metrics_read', 'billing_read', 'billing_manage', 'contract_manage', 'support_case_read',
          'support_case_manage', 'audit_read', 'security_events_read', 'platform_identity_manage', 'platform_role_manage',
        ] : [],
      },
      initialized: true,
      initializedAuthority: platform ? 'platform' : 'tenant',
    })
    console.log(
      `%c[TALOS DEV]%c Auth bypassed as %c${role}%c | 页面现已可访问`,
      'color:#FF453A;font-weight:bold', '', 'color:#FF453A;font-weight:bold', ''
    )
  }
}
