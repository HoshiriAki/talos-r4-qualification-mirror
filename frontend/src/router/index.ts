import { createRouter, createWebHistory, type RouteRecordRaw } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useTenantPreviewStore } from '@/stores/tenantPreview'
import { useUiModeStore } from '@/stores/uiMode'
import type { PlatformCapability } from '@/api/auth'

const tenantMeta = (title: string, requireTenantAdmin = false) => ({
  requiresAuth: true,
  authority: 'tenant',
  shell: 'tenant',
  requireTenantAdmin,
  title,
})

const controlMeta = (title: string, capability?: PlatformCapability) => ({
  requiresAuth: true,
  authority: 'platform',
  shell: 'control',
  capability,
  title,
})

const uiLabEnabled = import.meta.env.DEV || import.meta.env.VITE_ENABLE_UI_LAB === 'true'

const uiLabRoutes: RouteRecordRaw[] = uiLabEnabled
  ? [
      {
        path: '/ui-lab',
        name: 'UiLab',
        component: () => import('@/ui-lab/UiLabPage.vue'),
        meta: { layout: 'blank', title: 'TALOS UI Lab · Component System' },
      },
      {
        path: '/ui-lab/sandbox',
        name: 'UiLabSandbox',
        component: () => import('@/ui-lab/sandbox/UiLabSandboxPage.vue'),
        meta: { layout: 'blank', title: 'TALOS UI Lab Sandbox' },
      },
      { path: '/app/design-system', redirect: '/ui-lab' },
      { path: '/app/ui-lab', redirect: '/ui-lab' },
    ]
  : []

const routes: RouteRecordRaw[] = [
  { path: '/', redirect: '/app/overview' },
  {
    path: '/login',
    name: 'Login',
    component: () => import('@/pages/LoginPage.vue'),
    meta: { guest: true, authority: 'tenant', layout: 'blank', title: '登录' },
  },
  {
    path: '/control/login',
    name: 'ControlLogin',
    component: () => import('@/pages/LoginPage.vue'),
    meta: { guest: true, authority: 'platform', layout: 'blank', title: 'Talos Control 登录' },
  },
  { path: '/register', name: 'Register', component: () => import('@/pages/RegisterPage.vue'), meta: { guest: true, authority: 'tenant', layout: 'blank', title: '创建账号' } },
  { path: '/forgot-password', name: 'ForgotPassword', component: () => import('@/pages/ForgotPasswordPage.vue'), meta: { guest: true, authority: 'tenant', layout: 'blank', title: '找回密码' } },
  { path: '/reset-password', name: 'ResetPassword', component: () => import('@/pages/ResetPasswordPage.vue'), meta: { guest: true, authority: 'tenant', layout: 'blank', title: '重置密码' } },

  ...uiLabRoutes,

  { path: '/app/overview', name: 'Dashboard', component: () => import('@/pages/DashboardPage.vue'), meta: tenantMeta('仪表盘') },
  { path: '/app/pricing', name: 'PriceCalc', component: () => import('@/pages/PricePage.vue'), meta: tenantMeta('价格计算与订单录入') },
  { path: '/app/orders', name: 'Customers', component: () => import('@/pages/CustomersPage.vue'), meta: tenantMeta('订单工作台') },
  { path: '/app/devices', name: 'Devices', component: () => import('@/pages/DevicesPage.vue'), meta: tenantMeta('设备信息') },
  { path: '/app/checkin', name: 'Checkin', component: () => import('@/pages/CheckinPage.vue'), meta: tenantMeta('扫码入库') },
  { path: '/app/shipping', name: 'Shipping', component: () => import('@/pages/ShippingPage.vue'), meta: tenantMeta('发货管理') },
  { path: '/app/barcode', name: 'Barcode', component: () => import('@/pages/BarcodePage.vue'), meta: tenantMeta('条码管理') },
  { path: '/app/audit', name: 'Audit', component: () => import('@/pages/AuditPage.vue'), meta: tenantMeta('操作日志') },
  { path: '/app/booking', redirect: '/app/orders' },
  { path: '/app/credit', name: 'Credit', component: () => import('@/pages/CreditPage.vue'), meta: tenantMeta('信用管理') },
  { path: '/app/overdue', name: 'Overdue', component: () => import('@/pages/OverduePage.vue'), meta: tenantMeta('逾期管理') },
  { path: '/app/contracts', name: 'Contract', component: () => import('@/pages/ContractPage.vue'), meta: tenantMeta('合同管理', true) },
  { path: '/app/optical-sop', name: 'OpticalSop', component: () => import('@/pages/OpticalSopPage.vue'), meta: tenantMeta('光学检测') },
  { path: '/app/settings/dynamic-pricing', name: 'DynamicPricing', component: () => import('@/pages/DynamicPricingPage.vue'), meta: tenantMeta('动态调价', true) },
  { path: '/app/settings/staff', name: 'TenantMembers', component: () => import('@/pages/TenantMembersPage.vue'), meta: tenantMeta('租户成员', true) },
  { path: '/app/settings/device-models', name: 'DeviceModels', component: () => import('@/pages/DeviceModelsPage.vue'), meta: tenantMeta('型号管理', true) },
  { path: '/app/settings/warehouses', name: 'Warehouses', component: () => import('@/pages/WarehousePage.vue'), meta: tenantMeta('仓库管理', true) },
  { path: '/app/settings/finance', name: 'Finance', component: () => import('@/pages/FinancePage.vue'), meta: tenantMeta('财务税务', true) },
  { path: '/app/settings/integrations', name: 'IntegrationSettings', component: () => import('@/pages/IntegrationSettingsPage.vue'), meta: tenantMeta('Provider 集成配置', true) },

  { path: '/control/overview', name: 'ControlOverview', component: () => import('@/pages/control/ControlSectionPage.vue'), meta: { ...controlMeta('控制平面总览', 'platform_overview_read'), controlSection: 'overview' } },
  { path: '/control/operations', name: 'ControlOperations', component: () => import('@/pages/control/ControlSectionPage.vue'), meta: { ...controlMeta('平台运行', 'platform_health_read'), controlSection: 'operations' } },
  { path: '/control/business', name: 'ControlBusiness', component: () => import('@/pages/control/ControlSectionPage.vue'), meta: { ...controlMeta('平台经营', 'business_metrics_read'), controlSection: 'business' } },
  { path: '/control/support', name: 'ControlSupport', component: () => import('@/pages/control/ControlSectionPage.vue'), meta: { ...controlMeta('售后支持', 'support_case_read'), controlSection: 'support' } },
  { path: '/control/audit', name: 'ControlAudit', component: () => import('@/pages/control/ControlSectionPage.vue'), meta: { ...controlMeta('平台审计', 'audit_read'), controlSection: 'audit' } },
  { path: '/control/settings', name: 'ControlSettings', component: () => import('@/pages/control/ControlSectionPage.vue'), meta: { ...controlMeta('平台设置', 'platform_identity_manage'), controlSection: 'settings' } },
  { path: '/control/tenants', name: 'TenantManagement', component: () => import('@/views/control/TenantList.vue'), meta: controlMeta('租户目录', 'tenant_list') },
  { path: '/control/tenants/:id', name: 'TenantDetail', component: () => import('@/pages/TenantGovernanceDetailPage.vue'), meta: controlMeta('租户详情', 'tenant_read') },
  { path: '/control/governance', name: 'TenantGovernance', component: () => import('@/pages/TenantGovernancePage.vue'), meta: controlMeta('租户治理', 'tenant_governance_read') },
  { path: '/control/governance/:id', name: 'TenantGovernanceDetail', component: () => import('@/pages/TenantGovernanceDetailPage.vue'), meta: controlMeta('租户治理详情', 'tenant_governance_read') },
  { path: '/control/simulation', name: 'TenantSimulationCreate', component: () => import('@/pages/TenantSimulationPage.vue'), meta: controlMeta('租户模拟 · 创建', 'tenant_simulation_create') },
  { path: '/control/simulation/:simulationId', name: 'TenantSimulation', component: () => import('@/pages/TenantSimulationPage.vue'), meta: { ...controlMeta('租户模拟', 'tenant_simulation_read'), tenantSimulation: true } },
  { path: '/control/simulation/:simulationId/pricing', name: 'TenantSimulationPricing', component: () => import('@/pages/TenantSimulationPricingPage.vue'), meta: { ...controlMeta('隔离定价实验', 'tenant_simulation_read'), tenantSimulation: true } },
  { path: '/control/workspace/:workspaceSessionId', name: 'TenantWorkspace', component: () => import('@/pages/control/TenantWorkspacePage.vue'), meta: { ...controlMeta('Tenant Workspace'), capabilitiesAny: ['tenant_preview_read', 'tenant_diagnostics_read', 'tenant_simulation_read'] } },
  { path: '/control/platform-members', name: 'PlatformMembers', component: () => import('@/pages/control/PlatformMembersPage.vue'), meta: controlMeta('平台成员', 'platform_identity_manage') },

  { path: '/embedded/preview/:previewSessionId/dashboard', name: 'TenantPreviewDashboard', component: () => import('@/pages/DashboardPage.vue'), meta: { ...controlMeta('租户工作区 · 仪表盘'), capabilitiesAny: ['tenant_preview_read', 'tenant_diagnostics_read'], shell: 'embedded', tenantPreview: true } },
  { path: '/embedded/preview/:previewSessionId/orders', name: 'TenantPreviewOrders', component: () => import('@/pages/CustomersPage.vue'), meta: { ...controlMeta('租户工作区 · 订单'), capabilitiesAny: ['tenant_preview_read', 'tenant_diagnostics_read'], shell: 'embedded', tenantPreview: true } },
  { path: '/embedded/preview/:previewSessionId/devices', name: 'TenantPreviewDevices', component: () => import('@/pages/DevicesPage.vue'), meta: { ...controlMeta('租户工作区 · 设备'), capabilitiesAny: ['tenant_preview_read', 'tenant_diagnostics_read'], shell: 'embedded', tenantPreview: true } },
  { path: '/embedded/preview/:previewSessionId/audit', name: 'TenantPreviewAudit', component: () => import('@/pages/AuditPage.vue'), meta: { ...controlMeta('租户工作区 · 审计'), capabilitiesAny: ['tenant_preview_read', 'tenant_diagnostics_read'], shell: 'embedded', tenantPreview: true } },

  { path: '/:pathMatch(.*)*', name: 'NotFound', component: () => import('@/pages/NotFoundPage.vue'), meta: { layout: 'blank', title: '404 - 页面不存在' } },
]

const router = createRouter({ history: createWebHistory(), routes })

router.beforeEach(async (to) => {
  const auth = useAuthStore()
  const preview = useTenantPreviewStore()
  const requiredAuthority = to.meta.authority as 'tenant' | 'platform' | undefined

  if ((to.meta.requiresAuth || to.meta.guest) && (!auth.initialized || auth.initializedAuthority !== requiredAuthority)) {
    await auth.init(requiredAuthority)
  }

  if (to.meta.requiresAuth && !auth.isAuthenticated) {
    return { name: requiredAuthority === 'platform' ? 'ControlLogin' : 'Login', query: { redirect: to.fullPath } }
  }
  if (to.meta.requiresAuth && requiredAuthority === 'tenant' && !auth.isTenantAuthority) {
    return { name: 'Login', query: { redirect: to.fullPath } }
  }
  if (to.meta.requiresAuth && requiredAuthority === 'platform' && !auth.isPlatformAuthority) {
    return { name: 'ControlLogin', query: { redirect: to.fullPath } }
  }
  if (to.meta.guest && auth.isAuthenticated) {
    if (requiredAuthority === 'platform' && auth.isPlatformAuthority) return '/control/overview'
    if (requiredAuthority === 'tenant' && auth.isTenantAuthority) return '/app/overview'
  }
  if (to.meta.requireTenantAdmin && !auth.isTenantAdmin) {
    sessionStorage.setItem('routeGuardMessage', '此页面需要租户 admin 或 owner 权限')
    return '/app/overview'
  }
  const capability = to.meta.capability as PlatformCapability | undefined
  if (capability && !auth.hasCapability(capability)) {
    sessionStorage.setItem('routeGuardMessage', `缺少平台 capability: ${capability}`)
    return '/control/overview'
  }
  const capabilitiesAny = to.meta.capabilitiesAny as PlatformCapability[] | undefined
  if (capabilitiesAny?.length && !capabilitiesAny.some(auth.hasCapability)) {
    sessionStorage.setItem('routeGuardMessage', `缺少工作区 capability: ${capabilitiesAny.join(' / ')}`)
    return '/control/overview'
  }

  if (to.meta.tenantPreview) {
    const sessionId = String(to.params.previewSessionId || '')
    try {
      await preview.hydrate(sessionId)
      useUiModeStore().setMode('work')
    } catch {
      sessionStorage.setItem('routeGuardMessage', '租户预览会话已过期或不可用')
      return '/control/governance'
    }
  } else if (preview.active && !to.path.startsWith('/control/workspace/')) {
    preview.clear()
  }

  const complexPages = ['Customers', 'Devices', 'Finance', 'ApiKeys', 'IntegrationSettings', 'Credit', 'Overdue', 'Contract']
  if (to.name && complexPages.includes(String(to.name)) && window.innerWidth < 600) {
    sessionStorage.setItem('routeGuardMessage', '此页面不适配手机屏幕，请使用 PC 或平板访问')
    return requiredAuthority === 'platform' ? '/control/overview' : '/app/overview'
  }
})

router.afterEach((to) => {
  document.title = (to.meta.title as string) || 'TALOS'
})

export default router