import { reactive } from 'vue'
import type { UserSettings } from '@/api/user-settings'

const KEY = 'talos-settings'

export interface Settings {
  sidebarOrder: string[]
  hiddenPaths: string[]
  homePage: string
  theme: 'dark' | 'light' | 'system'
  glassOpacity: number
  showScanlines: boolean
  showDotGrid: boolean
  borderRadius: number
}

function defaults(): Settings {
  return { sidebarOrder: [], hiddenPaths: [], homePage: '/app/overview', theme: 'dark', glassOpacity: 0.7, showScanlines: true, showDotGrid: true, borderRadius: 8 }
}

/** Shared reactive settings singleton — AppSidebar + SettingsPage both use it */
export const settingsState = reactive<Settings>(loadSettings())

export function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem(KEY)
    if (raw) {
      const parsed = JSON.parse(raw)
      return {
        sidebarOrder: Array.isArray(parsed.sidebarOrder) ? parsed.sidebarOrder : [],
        hiddenPaths: Array.isArray(parsed.hiddenPaths) ? parsed.hiddenPaths : [],
        homePage: typeof parsed.homePage === 'string' ? parsed.homePage : '/app/overview',
        theme: (parsed.theme === 'dark' || parsed.theme === 'light' || parsed.theme === 'system') ? parsed.theme : 'dark',
        glassOpacity: typeof parsed.glassOpacity === 'number' ? Math.max(0.2, Math.min(1, parsed.glassOpacity)) : 0.7,
        showScanlines: typeof parsed.showScanlines === 'boolean' ? parsed.showScanlines : true,
        showDotGrid: typeof parsed.showDotGrid === 'boolean' ? parsed.showDotGrid : true,
        borderRadius: typeof parsed.borderRadius === 'number' ? Math.max(0, Math.min(20, parsed.borderRadius)) : 8,
      }
    }
  } catch {}
  return defaults()
}

export function saveSettings(s: Settings) {
  localStorage.setItem(KEY, JSON.stringify(s))
  // Sync reactive singleton
  settingsState.sidebarOrder = s.sidebarOrder
  settingsState.hiddenPaths = s.hiddenPaths
  settingsState.homePage = s.homePage
  settingsState.theme = s.theme
  settingsState.glassOpacity = s.glassOpacity
  settingsState.showScanlines = s.showScanlines
  settingsState.showDotGrid = s.showDotGrid
  settingsState.borderRadius = s.borderRadius
  applyGlassOpacity(s.glassOpacity)
  applyScanlines(s.showScanlines)
  applyDotGrid(s.showDotGrid)
  applyBorderRadius(s.borderRadius)
  import('@/api/user-settings').then(m => m.saveUserSettings(s)).catch(() => {})
}

export function applyGlassOpacity(value: number) {
  document.documentElement.style.setProperty('--glass-opacity', String(value))
}

export function applyScanlines(show: boolean) {
  document.documentElement.style.setProperty('--show-scanlines', show ? '1' : '0')
}

export function applyDotGrid(show: boolean) {
  document.documentElement.style.setProperty('--show-dots', show ? '1' : '0')
}

export function applyBorderRadius(value: number) {
  document.documentElement.style.setProperty('--radius-md', `${value}px`)
  document.documentElement.style.setProperty('--radius-sm', `${Math.max(0, value - 2)}px`)
  document.documentElement.style.setProperty('--radius-lg', `${value + 4}px`)
}

/**
 * Pull remote settings from the server and merge into localStorage + reactive state.
 * Called after login to sync settings across devices/domains.
 * Returns the merged settings.
 */
export async function fetchAndMergeRemoteSettings(): Promise<Settings> {
  const local = { ...settingsState }
  try {
    const { fetchUserSettings } = await import('@/api/user-settings')
    const remote = await fetchUserSettings()
    if (remote) {
      // Remote is the canonical source of truth — always wins.
      // (Catch block falls back to local on network error.)
      const merged: Settings = {
        sidebarOrder: remote.sidebarOrder,
        hiddenPaths: remote.hiddenPaths,
        homePage: remote.homePage,
        theme: remote.theme,
        glassOpacity: typeof (remote as any).glassOpacity === 'number' ? (remote as any).glassOpacity : 0.7,
        showScanlines: typeof (remote as any).showScanlines === 'boolean' ? (remote as any).showScanlines : true,
        showDotGrid: typeof (remote as any).showDotGrid === 'boolean' ? (remote as any).showDotGrid : true,
        borderRadius: typeof (remote as any).borderRadius === 'number' ? (remote as any).borderRadius : 8,
      }
      // Persist merged result
      localStorage.setItem(KEY, JSON.stringify(merged))
      settingsState.sidebarOrder = merged.sidebarOrder
      settingsState.hiddenPaths = merged.hiddenPaths
      settingsState.homePage = merged.homePage
      settingsState.theme = merged.theme
      settingsState.glassOpacity = merged.glassOpacity
      settingsState.showScanlines = merged.showScanlines
      settingsState.showDotGrid = merged.showDotGrid
      settingsState.borderRadius = merged.borderRadius
      applyGlassOpacity(merged.glassOpacity)
      applyScanlines(merged.showScanlines)
      applyDotGrid(merged.showDotGrid)
      applyBorderRadius(merged.borderRadius)
      return merged
    }
  } catch {
    // Offline — keep local
  }
  return local
}

/**
 * Priority-ordered fallback pages used when the saved homePage is hidden.
 * /price is the main operational entry point; /customers is the fallback.
 */
const HOME_FALLBACKS = ['/app/pricing', '/app/orders', '/app/checkin', '/app/shipping', '/app/audit']
const HOME_ROUTES = new Set([
  '/app/overview', ...HOME_FALLBACKS, '/app/devices', '/app/settings/dynamic-pricing',
  '/app/settings/staff', '/app/settings/device-models', '/app/settings/warehouses',
  '/app/barcode', '/app/contracts', '/app/optical-sop',
  '/app/credit', '/app/overdue', '/app/settings/finance',
])

/** Resolve saved home page, ignoring invalid, login, or hidden paths. */
export function resolveHomePage(): string {
  const s = settingsState
  const candidate = HOME_ROUTES.has(s.homePage) ? s.homePage : '/app/overview'
  if (!s.hiddenPaths.includes(candidate)) return candidate
  // Home page is hidden — fall back to first non-hidden operational page
  for (const fb of HOME_FALLBACKS) {
    if (!s.hiddenPaths.includes(fb)) return fb
  }
  return '/app/overview'
}
