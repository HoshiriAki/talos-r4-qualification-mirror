import type { PlatformCapability } from '@/api/auth'

export interface SidebarGroupItem {
  path: string
  labelKey: string
  adminOnly?: boolean
  capability?: PlatformCapability
}

export interface SidebarGroup {
  id: string
  labelKey: string
  namespace: string
  items: SidebarGroupItem[]
}

export const TENANT_SIDEBAR_GROUPS: SidebarGroup[] = [
  {
    id: 'system', labelKey: 'nav.system', namespace: 'tenant/application', items: [
      { path: '/app/overview', labelKey: 'nav.status' },
      { path: '/app/audit', labelKey: 'nav.eventLog' },
      { path: '/app/design-system', labelKey: 'nav.designSystem' },
    ],
  },
  {
    id: 'operations', labelKey: 'nav.operations', namespace: 'official/order+logistics+pricing', items: [
      { path: '/app/pricing', labelKey: 'nav.priceEngine' },
      { path: '/app/orders', labelKey: 'nav.processCtrl' },
      { path: '/app/shipping', labelKey: 'nav.dispatch' },
      { path: '/app/checkin', labelKey: 'nav.intake' },
      { path: '/app/contracts', labelKey: 'nav.contract', adminOnly: true },
      { path: '/app/optical-sop', labelKey: 'nav.inspection' },
    ],
  },
  {
    id: 'resources', labelKey: 'nav.resources', namespace: 'official/device+warehouse', items: [
      { path: '/app/devices', labelKey: 'nav.deviceReg' },
      { path: '/app/barcode', labelKey: 'nav.barcode' },
      { path: '/app/settings/warehouses', labelKey: 'nav.warehouse', adminOnly: true },
      { path: '/app/settings/device-models', labelKey: 'nav.models', adminOnly: true },
    ],
  },
  {
    id: 'admin', labelKey: 'nav.admin', namespace: 'tenant/settings', items: [
      { path: '/app/credit', labelKey: 'nav.credit' },
      { path: '/app/overdue', labelKey: 'nav.faultMon' },
      { path: '/app/settings/finance', labelKey: 'nav.finance', adminOnly: true },
      { path: '/app/settings/dynamic-pricing', labelKey: 'nav.pricing', adminOnly: true },
      { path: '/app/settings/staff', labelKey: 'nav.staff', adminOnly: true },
      { path: '/app/settings/integrations', labelKey: 'nav.integrations', adminOnly: true },
    ],
  },
]

export const CONTROL_SIDEBAR_GROUPS: SidebarGroup[] = [
  {
    id: 'control', labelKey: 'nav.control', namespace: 'platform/control', items: [
      { path: '/control/overview', labelKey: 'nav.controlOverview', capability: 'platform_overview_read' },
      { path: '/control/operations', labelKey: 'nav.controlOperations', capability: 'platform_health_read' },
      { path: '/control/business', labelKey: 'nav.controlBusiness', capability: 'business_metrics_read' },
    ],
  },
  {
    id: 'tenants', labelKey: 'nav.tenants', namespace: 'platform/tenant-governance', items: [
      { path: '/control/tenants', labelKey: 'nav.tenantDirectory', capability: 'tenant_list' },
      { path: '/control/governance', labelKey: 'nav.tenantGovernance', capability: 'tenant_governance_read' },
      { path: '/control/simulation', labelKey: 'nav.tenantSimulation', capability: 'tenant_simulation_read' },
    ],
  },
  {
    id: 'support', labelKey: 'nav.support', namespace: 'platform/support', items: [
      { path: '/control/support', labelKey: 'nav.supportCases', capability: 'support_case_read' },
      { path: '/control/audit', labelKey: 'nav.platformAudit', capability: 'audit_read' },
    ],
  },
  {
    id: 'control-settings', labelKey: 'nav.controlSettings', namespace: 'platform/identity', items: [
      { path: '/control/platform-members', labelKey: 'nav.platformMembers', capability: 'platform_identity_manage' },
      { path: '/control/settings', labelKey: 'nav.controlSettings', capability: 'platform_identity_manage' },
    ],
  },
]

export const SIDEBAR_GROUPS = TENANT_SIDEBAR_GROUPS
export const ALL_SIDEBAR_ITEMS: SidebarGroupItem[] = [
  ...TENANT_SIDEBAR_GROUPS.flatMap(group => group.items),
  ...CONTROL_SIDEBAR_GROUPS.flatMap(group => group.items),
]
