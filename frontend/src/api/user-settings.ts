import { requestJson } from './client'

export interface UserSettings {
  sidebarOrder: string[]
  hiddenPaths: string[]
  homePage: string
  theme: 'dark' | 'light' | 'system'
  glassOpacity: number
  showScanlines: boolean
  showDotGrid: boolean
  borderRadius: number
}

/** Fetch persisted settings for the current user (requires auth). */
export async function fetchUserSettings(): Promise<UserSettings | null> {
  try {
    const res = await requestJson('/api/user-settings', '获取设置失败')
    if (res.ok && res.settings) {
      return {
        sidebarOrder: Array.isArray(res.settings.sidebarOrder) ? res.settings.sidebarOrder : [],
        hiddenPaths: Array.isArray(res.settings.hiddenPaths) ? res.settings.hiddenPaths : [],
        homePage: typeof res.settings.homePage === 'string' ? res.settings.homePage : '/',
        theme: (res.settings.theme === 'dark' || res.settings.theme === 'light' || res.settings.theme === 'system') ? res.settings.theme : 'dark',
        glassOpacity: typeof res.settings.glassOpacity === 'number' ? res.settings.glassOpacity : 0.7,
        showScanlines: typeof res.settings.showScanlines === 'boolean' ? res.settings.showScanlines : true,
        showDotGrid: typeof res.settings.showDotGrid === 'boolean' ? res.settings.showDotGrid : true,
        borderRadius: typeof res.settings.borderRadius === 'number' ? res.settings.borderRadius : 8,
      }
    }
  } catch {
    // Not logged in or network error — silently fall back to localStorage
  }
  return null
}

/** Save settings to the server (requires auth). */
export async function saveUserSettings(settings: UserSettings): Promise<boolean> {
  try {
    await requestJson('/api/user-settings', {
      method: 'PUT',
      body: JSON.stringify(settings),
    }, '保存设置失败')
    return true
  } catch {
    // Offline or auth lost — localStorage still has a copy
    return false
  }
}
