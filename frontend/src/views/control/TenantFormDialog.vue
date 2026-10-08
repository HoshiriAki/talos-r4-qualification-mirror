<template>
  <div v-if="visible" class="dialog-overlay" @click.self="close">
    <div class="dialog-container">
      <div class="dialog-header">
        <h2>{{ isEdit ? '编辑租户' : '创建租户' }}</h2>
        <button class="btn-close" @click="close">×</button>
      </div>

      <form @submit.prevent="handleSubmit" class="dialog-body">
        <!-- 租户名称 -->
        <div class="form-group">
          <label for="tenant-name" class="required">租户名称</label>
          <input
            id="tenant-name"
            v-model="form.name"
            type="text"
            class="form-control"
            placeholder="例如: ACME 公司"
            required
          />
          <p class="form-hint">租户的显示名称</p>
        </div>

        <!-- Slug -->
        <div class="form-group">
          <label for="tenant-slug" class="required">Slug</label>
          <input
            id="tenant-slug"
            v-model="form.slug"
            type="text"
            class="form-control"
            placeholder="例如: acme"
            pattern="[a-z0-9-]+"
            required
            :disabled="isEdit"
          />
          <p class="form-hint">
            只能包含小写字母、数字和连字符。用于子域名（如 acme.talos.app）
            {{ isEdit ? '（创建后不可修改）' : '' }}
          </p>
        </div>

        <!-- 状态 -->
        <div class="form-group">
          <label for="tenant-status">状态</label>
          <select
            id="tenant-status"
            v-model="form.status"
            class="form-control"
            :disabled="!isEdit || props.tenant?.status === 'deleted'"
          >
            <option value="active">活跃</option>
            <option value="suspended">已暂停</option>
            <option value="inactive">已停用</option>
          </select>
          <p class="form-hint">租户的当前状态</p>
        </div>

        <!-- 订阅计划 -->
        <div class="form-group">
          <label for="tenant-plan">订阅计划</label>
          <select
            id="tenant-plan"
            v-model="form.plan"
            class="form-control"
          >
            <option value="free">免费版</option>
            <option value="pro">专业版</option>
            <option value="enterprise">企业版</option>
          </select>
          <p class="form-hint">租户的订阅计划</p>
        </div>

        <!-- 设置 (JSON) -->
        <div class="form-group">
          <label for="tenant-settings">设置（可选）</label>
          <textarea
            id="tenant-settings"
            v-model="settingsText"
            class="form-control"
            rows="4"
            placeholder='{"key": "value"}'
          ></textarea>
          <p class="form-hint">JSON 格式的配置信息</p>
          <p v-if="settingsError" class="form-error">{{ settingsError }}</p>
        </div>

        <!-- 错误提示 -->
        <div v-if="error" class="alert alert-error">
          {{ error }}
        </div>

        <!-- 操作按钮 -->
        <div class="dialog-footer">
          <button
            type="button"
            class="btn btn-secondary"
            @click="close"
            :disabled="loading"
          >
            取消
          </button>
          <button
            type="submit"
            class="btn btn-primary"
            :disabled="loading || !!settingsError"
          >
            <span v-if="loading">保存中...</span>
            <span v-else>{{ isEdit ? '保存' : '创建' }}</span>
          </button>
        </div>
      </form>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useTenantStore } from '@/stores/tenant'
import type { Tenant } from '@/api/tenant'

// ── Props & Emits ──

interface Props {
  visible: boolean
  tenant?: Tenant | null
}

const props = withDefaults(defineProps<Props>(), {
  tenant: null,
})

const emit = defineEmits<{
  (e: 'update:visible', value: boolean): void
  (e: 'success'): void
}>()

// ── Store ──

const tenantStore = useTenantStore()

// ── State ──

const form = ref({
  name: '',
  slug: '',
  status: 'active' as 'active' | 'suspended' | 'inactive',
  plan: 'free',
})

const settingsText = ref('')
const settingsError = ref('')
const loading = ref(false)
const error = ref('')

// ── Computed ──

const isEdit = computed(() => !!props.tenant)

// ── Watchers ──

// 当对话框打开时，填充表单
watch(() => props.visible, (visible) => {
  if (visible) {
    if (props.tenant) {
      // 编辑模式
      form.value = {
        name: props.tenant.name,
        slug: props.tenant.slug,
        status: props.tenant.status === 'deleted' ? 'inactive' : props.tenant.status,
        plan: props.tenant.plan || 'free',
      }
      settingsText.value = props.tenant.settings || ''
    } else {
      // 创建模式
      resetForm()
    }
    error.value = ''
  }
})

// 验证 settings JSON
watch(settingsText, (value) => {
  if (!value.trim()) {
    settingsError.value = ''
    return
  }

  try {
    JSON.parse(value)
    settingsError.value = ''
  } catch (e) {
    settingsError.value = 'JSON 格式错误'
  }
})

// ── Methods ──

function close() {
  emit('update:visible', false)
}

function resetForm() {
  form.value = {
    name: '',
    slug: '',
    status: 'active',
    plan: 'free',
  }
  settingsText.value = ''
  settingsError.value = ''
  error.value = ''
}

async function handleSubmit() {
  if (loading.value || settingsError.value) {
    return
  }

  loading.value = true
  error.value = ''

  try {
    // 解析 settings
    let settings: Record<string, any> | undefined
    if (settingsText.value.trim()) {
      try {
        settings = JSON.parse(settingsText.value)
      } catch (e) {
        error.value = 'settings JSON 格式错误'
        loading.value = false
        return
      }
    }

    if (isEdit.value && props.tenant) {
      // 更新租户
      await tenantStore.updateTenant(props.tenant.id, {
        name: form.value.name,
        settings,
      })
      if (props.tenant.status !== 'deleted' && form.value.status !== props.tenant.status) {
        await tenantStore.updateTenantStatus(props.tenant.id, form.value.status)
      }
    } else {
      // 创建租户
      await tenantStore.createTenant({
        name: form.value.name,
        slug: form.value.slug,
        settings,
      })
    }

    emit('success')
    close()
  } catch (e: any) {
    error.value = e.response?.data?.error || e.message || '操作失败'
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.dialog-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: color-mix(in srgb, var(--bg-base) 78%, transparent);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.dialog-container {
  background: var(--bg-surface);
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-offset-accent);
  width: 90%;
  max-width: 600px;
  max-height: 90vh;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.dialog-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 20px 24px;
  border-bottom: 1px solid var(--border-default);
}

.dialog-header h2 {
  font-family: var(--font-mono);
  font-size: var(--font-size-2xl);
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
}

.btn-close {
  background: none;
  border: none;
  font-size: var(--font-size-3xl);
  color: var(--text-secondary);
  cursor: pointer;
  padding: 0;
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  transition: background 0.2s;
}

.btn-close:hover {
  background: var(--bg-elevated);
  color: var(--text-accent);
}

.dialog-body {
  padding: 24px;
  overflow-y: auto;
}

.form-group {
  margin-bottom: 20px;
}

/* Plan/contract governance belongs to the Business control module. */
.form-group:has(#tenant-plan) {
  display: none;
}

.form-group label {
  display: block;
  font-size: var(--font-size-base);
  font-weight: 500;
  color: var(--text-secondary);
  margin-bottom: 8px;
}

.form-group label.required::after {
  content: ' *';
  color: var(--accent);
}

.form-control {
  width: 100%;
  padding: 10px 12px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  font-family: var(--font-sans);
  font-size: var(--font-size-base);
  color: var(--text-primary);
  background: var(--bg-field);
  transition: border-color 0.2s;
}

.form-control:focus {
  outline: none;
  border-color: var(--border-focus);
  box-shadow: var(--shadow-offset-accent);
}

.form-control:disabled {
  background: var(--bg-subtle);
  color: var(--text-muted);
  cursor: not-allowed;
}

.form-hint {
  margin-top: 4px;
  font-size: var(--font-size-xs);
  color: var(--text-muted);
}

.form-error {
  margin-top: 4px;
  font-size: var(--font-size-xs);
  color: var(--accent);
}

.alert {
  padding: 12px 16px;
  border-radius: var(--radius-md);
  margin-bottom: 16px;
}

.alert-error {
  background: var(--accent-muted);
  color: var(--text-primary);
  border: 1px solid var(--border-accent);
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding: 20px 24px;
  border-top: 1px solid var(--border-default);
}

.btn {
  padding: 10px 20px;
  border-radius: var(--radius-lg);
  font-family: var(--font-mono);
  font-size: var(--font-size-base);
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  border: none;
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--btn-primary-bg);
  color: var(--text-inverse);
  box-shadow: var(--shadow-offset-default);
}

.btn-primary:hover:not(:disabled) {
  background: var(--accent-hover);
  box-shadow: var(--shadow-offset-hover);
}

.btn-secondary {
  background: var(--btn-secondary-bg);
  color: var(--btn-secondary-text);
  border: 1px solid var(--border-default);
}

.btn-secondary:hover:not(:disabled) {
  background: var(--bg-elevated);
  border-color: var(--border-hover);
}
</style>
