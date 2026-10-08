<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { listInvoices, listSettlements, calculateTax } from '@/api/finance-tax'
import { listRefunds } from '@/api/finance'
import { useToast } from 'primevue/usetoast'

const toast = useToast()

// ── Card data ──────────────────────────────────────────────────────
const revenueTotal = ref(0)
const receivableTotal = ref(0)
const depositPoolTotal = ref(0)
const taxOverview = ref({ rate: 0.13, totalTax: 0, invoiceCount: 0 })
const loading = ref(true)

// Summary stats
const issuedCount = ref(0)
const voidedCount = ref(0)
const pendingRefunds = ref(0)

async function loadDashboard() {
  loading.value = true
  try {
    // Load invoices for revenue/AR/tax overview
    const invResult = await listInvoices({ page: 1, pageSize: 1000 })
    if (invResult.ok && invResult.invoices) {
      issuedCount.value = invResult.invoices.filter(i => i.status === 'issued').length
      voidedCount.value = invResult.invoices.filter(i => i.status === 'voided').length

      // Revenue = sum of issued invoice amounts
      revenueTotal.value = invResult.invoices
        .filter(i => i.status === 'issued')
        .reduce((sum, i) => sum + i.amount, 0)

      // Receivable = issued amounts (simplified: assume all issued are receivable)
      receivableTotal.value = invResult.invoices
        .filter(i => i.status === 'issued')
        .reduce((sum, i) => sum + i.amount, 0)

      // Tax overview
      taxOverview.value.invoiceCount = issuedCount.value
      taxOverview.value.totalTax = invResult.invoices
        .filter(i => i.status === 'issued')
        .reduce((sum, i) => sum + i.taxAmount, 0)
    }

    // Load refunds for deposit pool
    const refundResult = await listRefunds({ status: 'pending', pageSize: 100 })
    if (refundResult.ok && refundResult.refunds) {
      pendingRefunds.value = refundResult.pagination.total
      depositPoolTotal.value = refundResult.refunds.reduce((sum, r) => sum + r.amount, 0)
    }

    // Try tax config for rate
    try {
      const taxRes = await calculateTax(100) // any amount, just to check rate
      if (taxRes.ok) {
        taxOverview.value.rate = taxRes.taxRate
      }
    } catch {
      // Use default 0.13
    }
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载失败', detail: e.message || '未能加载财务概览', life: 4000 })
  } finally {
    loading.value = false
  }
}

onMounted(loadDashboard)

// ── Format helpers ─────────────────────────────────────────────────
function fmt(v: number): string {
  return '¥' + v.toLocaleString('zh-CN', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

function fmtRate(r: number): string {
  return (r * 100).toFixed(0) + '%'
}
</script>

<template>
  <div class="finance-dashboard">
    <h2 class="section-title">财务概览</h2>

    <div v-if="loading" class="loading-placeholder">加载中...</div>

    <template v-else>
      <div class="dashboard-cards">
        <!-- 收入 -->
        <div class="finance-card">
          <div class="card-icon revenue-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="12" y1="1" x2="12" y2="23"/><path d="M17 5H9.5a3.5 3.5 0 0 0 0 7h5a3.5 3.5 0 0 1 0 7H6"/>
            </svg>
          </div>
          <div class="card-body">
            <div class="card-label">本期收入</div>
            <div class="card-value text-success">{{ fmt(revenueTotal) }}</div>
            <div class="card-sub">已开发票 {{ issuedCount }} 张</div>
          </div>
        </div>

        <!-- 应收账款 -->
        <div class="finance-card">
          <div class="card-icon receivable-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <rect x="2" y="3" width="20" height="14" rx="2" ry="2"/><line x1="8" y1="21" x2="16" y2="21"/><line x1="12" y1="17" x2="12" y2="21"/>
            </svg>
          </div>
          <div class="card-body">
            <div class="card-label">应收账款</div>
            <div class="card-value text-warning">{{ fmt(receivableTotal) }}</div>
            <div class="card-sub">已作废 {{ voidedCount }} 张</div>
          </div>
        </div>

        <!-- 押金池 -->
        <div class="finance-card">
          <div class="card-icon deposit-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M21 12V7H5a2 2 0 0 1 0-4h14v4"/><path d="M3 5v14a2 2 0 0 0 2 2h16v-5"/><path d="M18 12a2 2 0 0 0 0 4h-1"/>
            </svg>
          </div>
          <div class="card-body">
            <div class="card-label">押金池</div>
            <div class="card-value text-info">{{ fmt(depositPoolTotal) }}</div>
            <div class="card-sub">待处理退款 {{ pendingRefunds }} 笔</div>
          </div>
        </div>

        <!-- 税务概览 -->
        <div class="finance-card">
          <div class="card-icon tax-icon">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M22 12h-4l-3 9L9 3l-3 9H2"/>
            </svg>
          </div>
          <div class="card-body">
            <div class="card-label">税务概览</div>
            <div class="card-value">{{ fmt(taxOverview.totalTax) }}</div>
            <div class="card-sub">税率 {{ fmtRate(taxOverview.rate) }} | 发票 {{ taxOverview.invoiceCount }} 张</div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.finance-dashboard {
  margin-bottom: 1.5rem;
}

.section-title {
  font-family: 'Space Mono', monospace;
  font-size: 1.125rem;
  font-weight: 600;
  margin-bottom: 1rem;
  color: var(--text-primary);
}

.loading-placeholder {
  padding: 2rem;
  text-align: center;
  color: var(--text-secondary);
}

.dashboard-cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 1rem;
}

.finance-card {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 1rem 1.25rem;
  background: var(--panel-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
}

.card-icon {
  flex-shrink: 0;
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  background: var(--bg-base);
  border: 1px solid var(--border);
}

.card-icon svg {
  width: 20px;
  height: 20px;
}

.revenue-icon { color: #22c55e; }
.receivable-icon { color: #eab308; }
.deposit-icon { color: #3b82f6; }
.tax-icon { color: #a855f7; }

.card-body {
  flex: 1;
  min-width: 0;
}

.card-label {
  font-family: 'Space Mono', monospace;
  font-size: 0.75rem;
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  margin-bottom: 0.25rem;
}

.card-value {
  font-family: 'Space Mono', monospace;
  font-size: 1.125rem;
  font-weight: 700;
  color: var(--text-primary);
  margin-bottom: 0.25rem;
}

.card-sub {
  font-size: 0.75rem;
  color: var(--text-secondary);
}

.text-success { color: #22c55e; }
.text-warning { color: #eab308; }
.text-info { color: #3b82f6; }
</style>
