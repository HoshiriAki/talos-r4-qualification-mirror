<template>
  <div class="tenant-list-page">
    <!-- 页面头部 -->
    <div class="page-header">
      <h1>租户管理</h1>
      <button class="btn btn-primary" @click="showCreateDialog = true">
        <i class="icon-plus"></i>
        创建租户
      </button>
    </div>

    <!-- 统计卡片 -->
    <div class="stats-cards">
      <div class="stat-card">
        <div class="stat-label">总租户数</div>
        <div class="stat-value">{{ tenantStore.totalCount }}</div>
      </div>
      <div class="stat-card active">
        <div class="stat-label">活跃</div>
        <div class="stat-value">{{ tenantStore.activeTenants.length }}</div>
      </div>
      <div class="stat-card suspended">
        <div class="stat-label">已暂停</div>
        <div class="stat-value">{{ tenantStore.suspendedTenants.length }}</div>
      </div>
      <div class="stat-card inactive">
        <div class="stat-label">已停用</div>
        <div class="stat-value">{{ tenantStore.inactiveTenants.length }}</div>
      </div>
    </div>

    <!-- 加载状态 -->
    <div v-if="tenantStore.loading" class="loading-state">
      <div class="spinner"></div>
      <p>加载中...</p>
    </div>

    <!-- 错误提示 -->
    <div v-else-if="tenantStore.error" class="error-state">
      <p class="error-message">{{ tenantStore.error }}</p>
      <button class="btn btn-secondary" @click="loadTenants">重试</button>
    </div>

    <!-- 租户列表 -->
    <div v-else class="tenant-table-container">
      <table class="tenant-table">
        <thead>
          <tr>
            <th>租户名称</th>
            <th>Slug</th>
            <th>状态</th>
            <th>创建时间</th>
            <th>更新时间</th>
            <th>操作</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="tenant in tenantStore.tenants" :key="tenant.id">
            <td>
              <div class="tenant-name">
                {{ tenant.name }}
              </div>
            </td>
            <td>
              <code class="tenant-slug">{{ tenant.slug }}</code>
            </td>
            <td>
              <span :class="['status-badge', `status-${tenant.status}`]">
                {{ statusText(tenant.status) }}
              </span>
            </td>
            <td class="text-muted">{{ formatDate(tenant.createdAt) }}</td>
            <td class="text-muted">{{ formatDate(tenant.updatedAt) }}</td>
            <td>
              <div class="action-buttons">
                <button
                  class="btn-icon"
                  title="查看详情"
                  @click="viewTenant(tenant.id)"
                >
                  <i class="icon-eye"></i>
                </button>
                <button
                  class="btn-icon"
                  title="编辑"
                  @click="editTenant(tenant)"
                >
                  <i class="icon-edit"></i>
                </button>
                <button
                  v-if="tenant.status === 'active'"
                  class="btn-icon btn-warning"
                  title="暂停"
                  @click="updateStatus(tenant.id, 'suspended')"
                >
                  <i class="icon-pause"></i>
                </button>
                <button
                  v-else-if="tenant.status === 'suspended'"
                  class="btn-icon btn-success"
                  title="激活"
                  @click="updateStatus(tenant.id, 'active')"
                >
                  <i class="icon-play"></i>
                </button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>

      <!-- 空状态 -->
      <div v-if="tenantStore.tenants.length === 0" class="empty-state">
        <p>暂无租户</p>
        <button class="btn btn-primary" @click="showCreateDialog = true">
          创建第一个租户
        </button>
      </div>
    </div>

    <!-- 创建/编辑租户对话框 -->
    <TenantFormDialog
      v-model:visible="showCreateDialog"
      :tenant="editingTenant"
      @success="onTenantSaved"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useTenantStore } from '@/stores/tenant'
import type { Tenant } from '@/api/tenant'
import TenantFormDialog from './TenantFormDialog.vue'

const router = useRouter()
const tenantStore = useTenantStore()

const showCreateDialog = ref(false)
const editingTenant = ref<Tenant | null>(null)

// ── 生命周期 ──

onMounted(() => {
  loadTenants()
})

// ── 方法 ──

async function loadTenants() {
  try {
    await tenantStore.fetchTenants()
  } catch (error) {
    console.error('加载租户列表失败:', error)
  }
}

function viewTenant(tenantId: string) {
  router.push(`/control/tenants/${tenantId}`)
}

function editTenant(tenant: Tenant) {
  editingTenant.value = tenant
  showCreateDialog.value = true
}

async function updateStatus(tenantId: string, status: 'active' | 'suspended' | 'inactive') {
  if (!confirm(`确定要${statusText(status)}该租户吗？`)) {
    return
  }

  try {
    await tenantStore.updateTenantStatus(tenantId, status)
  } catch (error) {
    console.error('更新租户状态失败:', error)
    alert('操作失败，请重试')
  }
}

function onTenantSaved() {
  showCreateDialog.value = false
  editingTenant.value = null
  loadTenants()
}

function statusText(status: string): string {
  const map: Record<string, string> = {
    active: '活跃',
    suspended: '已暂停',
    inactive: '已停用',
  }
  return map[status] || status
}

function formatDate(dateStr: string): string {
  const date = new Date(dateStr)
  return date.toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}
</script>

<style scoped>
.tenant-list-page {
  padding: 24px;
  max-width: 1400px;
  margin: 0 auto;
}

.page-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 24px;
}

.page-header h1 {
  font-size: 24px;
  font-weight: 600;
  margin: 0;
}

.stats-cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 16px;
  margin-bottom: 24px;
}

.stat-card {
  background: white;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  padding: 20px;
}

.stat-card.active {
  border-left: 4px solid #10b981;
}

.stat-card.suspended {
  border-left: 4px solid #f59e0b;
}

.stat-card.inactive {
  border-left: 4px solid #6b7280;
}

.stat-label {
  font-size: 14px;
  color: #6b7280;
  margin-bottom: 8px;
}

.stat-value {
  font-size: 32px;
  font-weight: 600;
  color: #111827;
}

.loading-state,
.error-state {
  text-align: center;
  padding: 60px 20px;
}

.spinner {
  width: 40px;
  height: 40px;
  border: 4px solid #e5e7eb;
  border-top-color: #3b82f6;
  border-radius: 50%;
  animation: spin 1s linear infinite;
  margin: 0 auto 16px;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.error-message {
  color: #dc2626;
  margin-bottom: 16px;
}

.tenant-table-container {
  background: white;
  border: 1px solid #e5e7eb;
  border-radius: 8px;
  overflow: hidden;
}

.tenant-table {
  width: 100%;
  border-collapse: collapse;
}

.tenant-table thead {
  background: #f9fafb;
}

.tenant-table th {
  text-align: left;
  padding: 12px 16px;
  font-size: 12px;
  font-weight: 600;
  color: #6b7280;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.tenant-table td {
  padding: 16px;
  border-top: 1px solid #e5e7eb;
}

.tenant-name {
  font-weight: 500;
  color: #111827;
}

.tenant-slug {
  background: #f3f4f6;
  padding: 2px 8px;
  border-radius: 4px;
  font-family: 'Courier New', monospace;
  font-size: 13px;
}

.status-badge {
  display: inline-block;
  padding: 4px 12px;
  border-radius: 12px;
  font-size: 12px;
  font-weight: 500;
}

.status-active {
  background: #d1fae5;
  color: #065f46;
}

.status-suspended {
  background: #fef3c7;
  color: #92400e;
}

.status-inactive {
  background: #e5e7eb;
  color: #374151;
}

.text-muted {
  color: #6b7280;
  font-size: 14px;
}

.action-buttons {
  display: flex;
  gap: 8px;
}

.btn-icon {
  padding: 6px 10px;
  border: 1px solid #e5e7eb;
  background: white;
  border-radius: 6px;
  cursor: pointer;
  transition: all 0.2s;
}

.btn-icon:hover {
  background: #f9fafb;
  border-color: #d1d5db;
}

.btn-icon.btn-warning:hover {
  background: #fef3c7;
  border-color: #f59e0b;
}

.btn-icon.btn-success:hover {
  background: #d1fae5;
  border-color: #10b981;
}

.empty-state {
  text-align: center;
  padding: 60px 20px;
  color: #6b7280;
}

.btn {
  padding: 10px 20px;
  border-radius: 6px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
  border: none;
}

.btn-primary {
  background: #3b82f6;
  color: white;
}

.btn-primary:hover {
  background: #2563eb;
}

.btn-secondary {
  background: #f3f4f6;
  color: #374151;
}

.btn-secondary:hover {
  background: #e5e7eb;
}

.icon-plus::before { content: '+'; margin-right: 4px; }
.icon-eye::before { content: '👁'; }
.icon-edit::before { content: '✏️'; }
.icon-pause::before { content: '⏸'; }
.icon-play::before { content: '▶️'; }
</style>
