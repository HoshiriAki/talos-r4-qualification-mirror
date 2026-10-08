<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { checkConsent, recordConsent } from '@/api/compliance'

const emit = defineEmits<{ consented: [] }>()

const auth = useAuthStore()
const visible = ref(false)
const loading = ref(false)
const error = ref('')

const CONSENT_VERSION = '1.0'

async function init() {
  if (!auth.currentUser) return
  try {
    const r = await checkConsent({ consentType: 'privacy_policy', version: CONSENT_VERSION })
    if (!r.hasConsented) {
      visible.value = true
    }
  } catch {
    // If check fails, show consent dialog to be safe
    visible.value = true
  }
}

async function agree() {
  loading.value = true
  error.value = ''
  try {
    await recordConsent({
      consentType: 'privacy_policy',
      version: CONSENT_VERSION,
      ipAddress: '',
      userAgent: navigator.userAgent,
    })
    visible.value = false
    emit('consented')
  } catch (e: unknown) {
    error.value = e instanceof Error ? e.message : '记录失败，请重试'
  } finally {
    loading.value = false
  }
}

onMounted(init)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="visible"
      class="fixed inset-0 z-60 flex items-center justify-center"
      style="background: rgba(0,0,0,0.6)"
    >
      <div class="panel max-w-lg w-full mx-4 max-h-[80vh] overflow-y-auto">
        <h2 class="font-mono text-lg font-bold text-text-primary mb-3 pb-2 border-b border-border">
          隐私政策与数据处理同意书
        </h2>

        <div class="font-mono text-sm text-text-secondary space-y-3 mb-5 leading-relaxed">
          <p>
            欢迎使用 Talos 设备租赁管理系统。为保障您的合法权益，请仔细阅读以下隐私政策条款。
          </p>

          <h3 class="text-text-primary font-bold">一、信息收集</h3>
          <p>
            我们仅收集提供服务所必需的个人信息，包括：账号名称、显示名称、邮箱地址、手机号码、
            IP 地址及浏览器 User-Agent 信息。
          </p>

          <h3 class="text-text-primary font-bold">二、信息使用</h3>
          <p>
            您的个人信息仅用于：(1) 身份验证与账号安全；(2) 系统操作审计日志；
            (3) 双因素认证（如启用）。未经您的明确同意，我们不会将个人信息用于其他目的。
          </p>

          <h3 class="text-text-primary font-bold">三、数据存储与安全</h3>
          <p>
            您的数据存储在加密的数据库中，访问受严格的权限控制。
            我们实施 AES-256 加密备份、异地容灾等安全措施保护您的数据。
          </p>

          <h3 class="text-text-primary font-bold">四、数据删除权</h3>
          <p>
            您有权随时请求删除您的个人数据。在收到请求后，我们将在 15 个工作日内完成数据删除或匿名化处理。
            您可以通过「个人中心」→「安全设置」提交数据删除请求。
          </p>

          <h3 class="text-text-primary font-bold">五、合规标准</h3>
          <p>
            本系统按照信息安全等级保护三级（等保三级）基础要求设计，包含完整的访问控制、
            审计追踪、数据加密和业务连续性/灾难恢复计划。
          </p>
        </div>

        <p v-if="error" class="text-xs font-mono text-btn-danger-text mb-3">{{ error }}</p>

        <div class="flex justify-end gap-3 pt-2 border-t border-border">
          <button
            class="btn-primary"
            :disabled="loading"
            @click="agree"
          >
            {{ loading ? '处理中...' : '同意并继续' }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
