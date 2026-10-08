<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useToast } from 'primevue/usetoast'
import {
  generateTwoFaSecret,
  verifyAndEnableTwoFa,
  disableTwoFa,
  getTwoFaStatus,
  requestDataDeletion,
} from '@/api/compliance'

const auth = useAuthStore()
const toast = useToast()

// ── 2FA ──
const twoFaEnabled = ref(false)
const twoFaHasSecret = ref(false)
const twoFaSecret = ref('')
const twoFaUrl = ref('')
const twoFaShowSetup = ref(false)
const twoFaCode = ref('')
const twoFaError = ref('')
const twoFaLoading = ref(false)

async function loadTwoFaStatus() {
  try {
    const r = await getTwoFaStatus()
    twoFaEnabled.value = r.enabled
    twoFaHasSecret.value = r.hasSecret
  } catch {}
}

async function startTwoFaSetup() {
  twoFaError.value = ''
  twoFaLoading.value = true
  try {
    const r = await generateTwoFaSecret()
    twoFaSecret.value = r.secret
    twoFaUrl.value = r.otpauthUrl
    twoFaShowSetup.value = true
    twoFaHasSecret.value = true
  } catch (e: unknown) {
    twoFaError.value = e instanceof Error ? e.message : '生成失败'
  } finally {
    twoFaLoading.value = false
  }
}

async function verifyEnableTwoFa() {
  if (!twoFaCode.value) {
    twoFaError.value = '请输入验证码'
    return
  }
  twoFaError.value = ''
  twoFaLoading.value = true
  try {
    await verifyAndEnableTwoFa({ code: twoFaCode.value })
    twoFaEnabled.value = true
    twoFaShowSetup.value = false
    twoFaCode.value = ''
    toast.add({ severity: 'success', summary: '2FA 已启用', life: 2000 })
  } catch (e: unknown) {
    twoFaError.value = e instanceof Error ? e.message : '验证失败'
  } finally {
    twoFaLoading.value = false
  }
}

async function handleDisableTwoFa() {
  if (!auth.currentUser) return
  twoFaLoading.value = true
  try {
    await disableTwoFa(auth.currentUser.id)
    twoFaEnabled.value = false
    twoFaHasSecret.value = false
    twoFaShowSetup.value = false
    twoFaSecret.value = ''
    twoFaUrl.value = ''
    toast.add({ severity: 'success', summary: '2FA 已禁用', life: 2000 })
  } catch (e: unknown) {
    twoFaError.value = e instanceof Error ? e.message : '禁用失败'
  } finally {
    twoFaLoading.value = false
  }
}

// ── Data Export ──
const exportLoading = ref(false)
async function handleExportData() {
  exportLoading.value = true
  try {
    // Export current user data as JSON
    const data = {
      username: auth.currentUser?.username,
      displayName: auth.currentUser?.displayName,
      email: auth.currentUser?.email,
      phone: auth.currentUser?.phone,
      authority: auth.currentUser?.authority,
      exportedAt: new Date().toISOString(),
    }
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `talos-data-export-${auth.currentUser?.username}.json`
    a.click()
    URL.revokeObjectURL(url)
    toast.add({ severity: 'success', summary: '数据导出完成', life: 2000 })
  } catch {
    toast.add({ severity: 'error', summary: '导出失败', life: 2000 })
  } finally {
    exportLoading.value = false
  }
}

// ── Account Deletion ──
const deletionConfirming = ref(false)
const deletionReason = ref('')
const deletionLoading = ref(false)
const deletionError = ref('')

async function handleRequestDeletion() {
  if (!deletionReason.value.trim()) {
    deletionError.value = '请填写删除原因'
    return
  }
  deletionError.value = ''
  deletionLoading.value = true
  try {
    await requestDataDeletion({
      requestType: 'account',
      reason: deletionReason.value,
    })
    deletionConfirming.value = false
    toast.add({
      severity: 'success',
      summary: '删除请求已提交',
      detail: '我们将在 15 个工作日内处理您的请求',
      life: 5000,
    })
  } catch (e: unknown) {
    deletionError.value = e instanceof Error ? e.message : '提交失败'
  } finally {
    deletionLoading.value = false
  }
}

onMounted(loadTwoFaStatus)
</script>

<template>
  <div class="space-y-5">
    <!-- 2FA Section -->
    <div>
      <h3 class="font-mono text-sm font-bold text-text-primary mb-2 pb-1 border-b border-border">
        双因素认证 (2FA)
      </h3>

      <div v-if="!twoFaShowSetup" class="flex items-center gap-3">
        <span
          class="badge text-xs"
          :class="twoFaEnabled ? 'badge-success' : 'badge-muted'"
        >
          {{ twoFaEnabled ? '已启用' : '未启用' }}
        </span>

        <button
          v-if="!twoFaEnabled"
          class="btn-secondary text-xs"
          :disabled="twoFaLoading"
          @click="startTwoFaSetup"
        >
          {{ twoFaLoading ? '处理中...' : '启用 2FA' }}
        </button>

        <button
          v-if="twoFaEnabled"
          class="btn-danger-outline text-xs"
          :disabled="twoFaLoading"
          @click="handleDisableTwoFa"
        >
          禁用 2FA
        </button>
      </div>

      <!-- 2FA Setup panel -->
      <div v-if="twoFaShowSetup" class="mt-3 p-3 border border-border">
        <p class="text-sm text-text-secondary mb-2 font-mono">
          请使用 TOTP 应用（如 Google Authenticator、Authy）扫描二维码或手动输入密钥：
        </p>

        <div class="bg-input px-3 py-2 mb-2 font-mono text-xs text-text-primary break-all select-all">
          {{ twoFaSecret }}
        </div>

        <p class="text-xs text-text-muted mb-3 font-mono">
          输入应用生成的 6 位验证码以完成启用。
        </p>

        <div class="flex items-center gap-2">
          <input
            v-model="twoFaCode"
            class="input w-32 font-mono text-center"
            placeholder="000000"
            maxlength="6"
          />
          <button
            class="btn-primary text-xs"
            :disabled="twoFaLoading"
            @click="verifyEnableTwoFa"
          >
            {{ twoFaLoading ? '验证中...' : '验证并启用' }}
          </button>
        </div>
      </div>

      <p v-if="twoFaError" class="text-xs font-mono text-btn-danger-text mt-2">{{ twoFaError }}</p>
    </div>

    <!-- Data Export Section -->
    <div>
      <h3 class="font-mono text-sm font-bold text-text-primary mb-2 pb-1 border-b border-border">
        数据导出
      </h3>
      <p class="text-sm text-text-secondary mb-2 font-mono">
        下载您的个人数据副本（JSON 格式）。
      </p>
      <button
        class="btn-secondary text-xs"
        :disabled="exportLoading"
        @click="handleExportData"
      >
        {{ exportLoading ? '导出中...' : '导出我的数据' }}
      </button>
    </div>

    <!-- Account Deletion Section -->
    <div>
      <h3 class="font-mono text-sm font-bold text-text-primary mb-2 pb-1 border-b border-border text-btn-danger-text">
        账号删除
      </h3>
      <p class="text-sm text-text-secondary mb-2 font-mono">
        请求删除您的账号及所有关联数据。我们将在 15 个工作日内完成处理。
        数据将被匿名化处理，无法恢复。
      </p>

      <div v-if="!deletionConfirming">
        <button
          class="btn-danger text-xs"
          @click="deletionConfirming = true"
        >
          请求删除账号
        </button>
      </div>

      <div v-else class="p-3 border border-border mt-2">
        <label class="mono-label block mb-1">删除原因（必填）</label>
        <textarea
          v-model="deletionReason"
          class="input w-full font-mono text-sm"
          rows="3"
          placeholder="请说明删除原因..."
        />
        <p v-if="deletionError" class="text-xs font-mono text-btn-danger-text mt-2">{{ deletionError }}</p>
        <div class="flex gap-2 mt-3">
          <button
            class="btn-danger text-xs"
            :disabled="deletionLoading"
            @click="handleRequestDeletion"
          >
            {{ deletionLoading ? '提交中...' : '确认提交删除请求' }}
          </button>
          <button
            class="btn-secondary text-xs"
            :disabled="deletionLoading"
            @click="deletionConfirming = false; deletionReason = ''; deletionError = ''"
          >
            取消
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
