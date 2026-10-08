<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useThemeStore } from '@/stores/theme'
import { useToast } from 'primevue/usetoast'
import { settingsState, saveSettings, applyGlassOpacity, applyScanlines, applyDotGrid, applyBorderRadius } from '@/utils/settings'
import defaultAvatar from '@/assets/images/default-avatar.svg'

const props = defineProps<{ visible: boolean; initialTab?: string }>()
const emit = defineEmits<{ 'update:visible': [value: boolean] }>()

const auth = useAuthStore()
const themeStore = useThemeStore()
const toast = useToast()

// ── Navigation ──
const sections = [
  { key: 'profile', label: '个人资料' },
  { key: 'security', label: '安全' },
  { key: 'preferences', label: '偏好设置' },
] as const
type SectionKey = (typeof sections)[number]['key']
const activeSection = ref<SectionKey>('profile')

// Reset tab when dialog opens
watch(
  () => props.visible,
  (v) => {
    if (v) {
      const tab = (props.initialTab as SectionKey | undefined) ?? 'profile'
      if (sections.some(s => s.key === tab)) activeSection.value = tab
    }
  }
)

// ── Profile form ──
const profileForm = ref({ displayName: '', email: '', phone: '' })
const profileSaving = ref(false)
const profileError = ref('')

watch(
  () => props.visible,
  (v) => {
    if (v && auth.currentUser) {
      profileForm.value = {
        displayName: auth.currentUser.displayName || '',
        email: auth.currentUser.email || '',
        phone: auth.currentUser.phone || '',
      }
      profileError.value = ''
      // Reset password form fields (HIGH audit finding #11)
      oldPassword.value = ''
      newPassword.value = ''
      confirmPassword.value = ''
      pwdError.value = ''
      pwdFieldError.value = ''
    }
  }
)

async function saveProfile() {
  profileSaving.value = true
  profileError.value = ''
  try {
    const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/
    const phoneRegex = /^[+]?[0-9\s-]{7,20}$/
    if (profileForm.value.email && !emailRegex.test(profileForm.value.email)) {
      profileError.value = '邮箱格式不正确'
      return
    }
    if (profileForm.value.phone && !phoneRegex.test(profileForm.value.phone)) {
      profileError.value = '手机号格式不正确'
      return
    }
    await auth.updateProfile(profileForm.value)
    toast.add({ severity: 'success', summary: '资料已保存', life: 2000 })
  } catch (e: unknown) {
    profileError.value = e instanceof Error ? e.message : '保存失败'
  } finally {
    profileSaving.value = false
  }
}

// ── Password form ──
const oldPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const pwdError = ref('')
const pwdFieldError = ref('')
const pwdSaving = ref(false)

function validatePwd(): boolean {
  pwdFieldError.value = ''
  if (newPassword.value.length < 8) {
    pwdFieldError.value = '密码长度至少 8 位'
    return false
  }
  if (newPassword.value !== confirmPassword.value) {
    pwdFieldError.value = '两次输入的密码不一致'
    return false
  }
  return true
}

async function changePassword() {
  if (!oldPassword.value || !newPassword.value) {
    pwdError.value = '请输入旧密码和新密码'
    return
  }
  if (!validatePwd()) return
  pwdSaving.value = true
  pwdError.value = ''
  try {
    await auth.changePassword(oldPassword.value, newPassword.value)
    toast.add({ severity: 'success', summary: '密码修改成功，请重新登录', life: 3000 })
    emit('update:visible', false)
    setTimeout(() => auth.logout(), 1500)
  } catch (e: unknown) {
    pwdError.value = e instanceof Error ? e.message : '密码修改失败'
  } finally {
    pwdSaving.value = false
  }
}

// ── Preferences ──
const themeOptions = [
  { label: '暗色', value: 'dark' as const },
  { label: '亮色', value: 'light' as const },
  { label: '跟随系统', value: 'system' as const },
]

function setCurrentTheme(v: 'dark' | 'light' | 'system') {
  settingsState.theme = v
  themeStore.setTheme(v)
  // Server sync handled by themeStore.setTheme → syncThemeToServer
  // (saveSettings not called here — avoids double-write audit finding #12)
}

// ── Glass opacity ──
const glassOpacity = ref(settingsState.glassOpacity)

function setGlassOpacity(v: number) {
  glassOpacity.value = v
  settingsState.glassOpacity = v
  applyGlassOpacity(v)
  saveSettings(settingsState)
}

// ── Scanlines / Dot grid ──
const showScanlines = ref(settingsState.showScanlines)
const showDotGrid = ref(settingsState.showDotGrid)

function toggleScanlines() {
  showScanlines.value = !showScanlines.value
  settingsState.showScanlines = showScanlines.value
  applyScanlines(showScanlines.value)
  saveSettings(settingsState)
}

function toggleDotGrid() {
  showDotGrid.value = !showDotGrid.value
  settingsState.showDotGrid = showDotGrid.value
  applyDotGrid(showDotGrid.value)
  saveSettings(settingsState)
}

// ── Border radius ──
const borderRadius = ref(settingsState.borderRadius)

function setBorderRadius(v: number) {
  borderRadius.value = v
  settingsState.borderRadius = v
  applyBorderRadius(v)
  saveSettings(settingsState)
}

const ALL_HOME_OPTIONS = [
  { label: '仪表盘', value: '/' },
  { label: '价格计算', value: '/price' },
  { label: '客户信息', value: '/customers' },
  { label: '扫码入库', value: '/checkin' },
  { label: '发货管理', value: '/shipping' },
  { label: '操作日志', value: '/audit' },
]

const homeOptions = computed(() =>
  ALL_HOME_OPTIONS.filter(h => !settingsState.hiddenPaths.includes(h.value))
)

function setHomePage(v: string) {
  settingsState.homePage = v
  saveSettings(settingsState)
}

const sidebarNavDefs = [
  { path: '/', label: '仪表盘' },
  { path: '/price', label: '价格计算' },
  { path: '/customers', label: '客户信息' },
  { path: '/checkin', label: '扫码入库' },
  { path: '/shipping', label: '发货管理' },
  { path: '/audit', label: '操作日志' },
]

function isPathHidden(path: string): boolean {
  return settingsState.hiddenPaths.includes(path)
}

function togglePathHidden(path: string) {
  const set = new Set(settingsState.hiddenPaths)
  if (set.has(path)) {
    set.delete(path)
  } else {
    set.add(path)
    // If hiding the current home page, switch to first available option
    if (settingsState.homePage === path) {
      const fb = homeOptions.value[0]
      if (fb) settingsState.homePage = fb.value
    }
  }
  settingsState.hiddenPaths = [...set]
  saveSettings(settingsState)
}
</script>

<template>
  <Dialog
    :visible="visible"
    @update:visible="emit('update:visible', $event)"
    modal
    header="个人中心"
    :style="{ width: '48rem' }"
  >
    <div class="flex" style="min-height: 28rem">
      <!-- Left nav -->
      <nav class="w-40 border-r border-border py-2 shrink-0">
        <button
          v-for="s in sections"
          :key="s.key"
          class="w-full text-left px-4 py-2 font-mono text-sm transition-colors duration-100"
          :class="
            activeSection === s.key
              ? 'text-text-primary bg-surface-hover border-r-2 border-text-primary'
              : 'text-text-muted hover:text-text-primary hover:bg-surface-hover'
          "
          @click="activeSection = s.key"
        >
          {{ s.label }}
        </button>
      </nav>

      <!-- Right content -->
      <div class="flex-1 px-5 py-5 overflow-y-auto">
        <!-- ─── Profile Tab ─── -->
        <div v-if="activeSection === 'profile'" class="space-y-4">
          <div class="flex items-center gap-4 mb-4">
            <div class="w-16 h-16 rounded flex items-center justify-center select-none shrink-0 overflow-hidden bg-bg-elevated border border-border">
              <img class="w-full h-full object-cover block" :src="defaultAvatar" alt="" />
            </div>
            <div>
              <div class="font-mono font-bold text-text-primary text-sm">
                {{ auth.currentUser?.username }}
              </div>
              <span
                class="badge text-xs"
                :class="auth.isTenantAdmin ? 'badge-admin' : 'badge-staff'"
              >
                {{ auth.tenantRole === 'owner' ? '所有者' : auth.tenantRole === 'admin' ? '管理员' : auth.isPlatformAuthority ? '平台成员' : '员工' }}
              </span>
            </div>
          </div>

          <div class="space-y-3">
            <div>
              <label class="mono-label block mb-1">显示名称</label>
              <InputText
                v-model="profileForm.displayName"
                class="w-full"
                placeholder="输入显示名称"
                maxlength="64"
              />
            </div>
            <div>
              <label class="mono-label block mb-1">邮箱</label>
              <InputText
                v-model="profileForm.email"
                class="w-full"
                placeholder="example@mail.com"
                maxlength="128"
              />
            </div>
            <div>
              <label class="mono-label block mb-1">手机号</label>
              <InputText
                v-model="profileForm.phone"
                class="w-full"
                placeholder="+86 13800138000"
                maxlength="32"
              />
            </div>
          </div>

          <p v-if="profileError" class="text-xs font-mono text-btn-danger-text">
            {{ profileError }}
          </p>

          <div class="pt-2">
            <Button
              severity="primary"
              :label="profileSaving ? '保存中...' : '保存'"
              :disabled="profileSaving"
              @click="saveProfile"
            />
          </div>
        </div>

        <!-- ─── Security Tab ─── -->
        <div v-if="activeSection === 'security'" class="space-y-3">
          <div>
            <label class="mono-label block mb-1">旧密码</label>
            <InputText
              v-model="oldPassword"
              type="password"
              class="w-full"
              placeholder="请输入旧密码"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">新密码</label>
            <InputText
              v-model="newPassword"
              type="password"
              class="w-full"
              placeholder="至少 8 位"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">确认新密码</label>
            <InputText
              v-model="confirmPassword"
              type="password"
              class="w-full"
              placeholder="请再次输入新密码"
            />
          </div>

          <small v-if="pwdFieldError" class="text-status-error text-xs">{{ pwdFieldError }}</small>
          <p v-if="pwdError" class="text-xs font-mono text-btn-danger-text">{{ pwdError }}</p>

          <div class="pt-2">
            <Button
              severity="primary"
              :label="pwdSaving ? '提交中...' : '修改密码'"
              :disabled="pwdSaving"
              @click="changePassword"
            />
          </div>
        </div>

        <!-- ─── Preferences Tab ─── -->
        <div v-if="activeSection === 'preferences'" class="space-y-5">
          <!-- Theme -->
          <div>
            <h3 class="font-mono text-sm font-bold text-text-primary mb-2 pb-1 border-b border-border">
              默认主题
            </h3>
            <div class="flex gap-2">
              <button
                v-for="opt in themeOptions"
                :key="opt.value"
                class="btn-secondary text-xs px-3 py-1"
                :class="{ 'btn-primary': settingsState.theme === opt.value }"
                @click="setCurrentTheme(opt.value)"
              >
                {{ opt.label }}
              </button>
            </div>
          </div>

          <!-- Glass opacity -->
          <div>
            <h3 class="font-mono text-sm font-bold text-text-primary mb-2 pb-1 border-b border-border">
              玻璃透明度
            </h3>
            <div class="flex items-center gap-3">
              <input
                type="range"
                min="0.2"
                max="1"
                step="0.05"
                :value="glassOpacity"
                @input="setGlassOpacity(parseFloat(($event.target as HTMLInputElement).value))"
                class="glass-slider"
              />
              <span class="font-mono text-xs text-text-muted w-10 text-right">
                {{ Math.round(glassOpacity * 100) }}%
              </span>
            </div>
          </div>

          <!-- Scanlines / Dot Grid -->
          <div>
            <h3 class="font-mono text-sm font-bold text-text-primary mb-2 pb-1 border-b border-border">
              背景特效
            </h3>
            <div class="space-y-2 max-w-xs">
              <div class="flex items-center justify-between py-1">
                <span class="text-sm text-text-primary font-mono">扫描线</span>
                <div
                  class="w-8 h-4 border transition-colors duration-micro flex items-center px-px cursor-pointer select-none"
                  :class="showScanlines ? 'bg-[var(--status-success)] border-[var(--status-success)]' : 'bg-border border-border'"
                  @click="toggleScanlines()"
                >
                  <div class="w-3 h-3 bg-white transition-transform duration-micro" :class="showScanlines ? 'translate-x-4' : 'translate-x-0'" />
                </div>
              </div>
              <div class="flex items-center justify-between py-1">
                <span class="text-sm text-text-primary font-mono">点阵网格</span>
                <div
                  class="w-8 h-4 border transition-colors duration-micro flex items-center px-px cursor-pointer select-none"
                  :class="showDotGrid ? 'bg-[var(--status-success)] border-[var(--status-success)]' : 'bg-border border-border'"
                  @click="toggleDotGrid()"
                >
                  <div class="w-3 h-3 bg-white transition-transform duration-micro" :class="showDotGrid ? 'translate-x-4' : 'translate-x-0'" />
                </div>
              </div>
            </div>
          </div>

          <!-- Border radius -->
          <div>
            <h3 class="font-mono text-sm font-bold text-text-primary mb-2 pb-1 border-b border-border">
              圆角大小
            </h3>
            <div class="flex items-center gap-3">
              <input
                type="range"
                min="0"
                max="20"
                step="1"
                :value="borderRadius"
                @input="setBorderRadius(parseInt(($event.target as HTMLInputElement).value))"
                class="glass-slider"
              />
              <span class="font-mono text-xs text-text-muted w-8 text-right">
                {{ borderRadius }}px
              </span>
            </div>
          </div>
          <div>
            <h3 class="font-mono text-sm font-bold text-text-primary mb-2 pb-1 border-b border-border">
              开屏首页
            </h3>
            <Select
              :model-value="settingsState.homePage"
              :options="homeOptions"
              option-label="label"
              option-value="value"
              class="max-w-xs"
              @change="setHomePage(($event as any).value || '/')"
            />
          </div>

          <!-- Sidebar visibility -->
          <div>
            <h3 class="font-mono text-sm font-bold text-text-primary mb-2 pb-1 border-b border-border">
              侧边栏菜单
            </h3>
            <div class="space-y-1 max-w-xs">
              <div
                v-for="item in sidebarNavDefs"
                :key="item.path"
                class="flex items-center justify-between py-1"
              >
                <span class="text-sm text-text-primary font-mono">{{ item.label }}</span>
                <div
                  class="w-8 h-4 border transition-colors duration-micro flex items-center px-px cursor-pointer select-none"
                  :class="
                    isPathHidden(item.path)
                      ? 'bg-border border-border'
                      : 'bg-[var(--status-success)] border-[var(--status-success)]'
                  "
                  @click="togglePathHidden(item.path)"
                >
                  <div
                    class="w-3 h-3 bg-white transition-transform duration-micro"
                    :class="isPathHidden(item.path) ? 'translate-x-0' : 'translate-x-4'"
                  />
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </Dialog>
</template>

<style scoped>
.glass-slider {
  -webkit-appearance: none;
  appearance: none;
  flex: 1;
  height: 4px;
  background: var(--border-default);
  border-radius: var(--radius-sm);
  outline: none;
  cursor: pointer;
}

.glass-slider::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 16px;
  height: 16px;
  border-radius: var(--radius-full);
  background: var(--accent);
  border: 2px solid var(--bg-elevated);
  cursor: pointer;
  transition: background var(--duration-micro);
}

.glass-slider::-webkit-slider-thumb:hover {
  background: var(--accent-hover);
}

.glass-slider::-moz-range-thumb {
  width: 16px;
  height: 16px;
  border-radius: var(--radius-full);
  background: var(--accent);
  border: 2px solid var(--bg-elevated);
  cursor: pointer;
}
</style>
