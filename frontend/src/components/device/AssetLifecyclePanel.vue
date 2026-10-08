<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import {
  getPurchase,
  calculateDepreciation,
  getDepreciation,
  calculateRoa,
  getRepairStats,
} from '@/api/assets'
import type {
  PurchaseInfo,
  DepreciationEntry,
  DepreciationSummary,
  NetBookValue,
  RoaCalculate,
  RepairStats,
} from '@/api/assets'

const props = defineProps<{
  visible: boolean
  deviceSerialNo: string
}>()

const emit = defineEmits<{
  (e: 'update:visible', v: boolean): void
}>()

// ── State ──────────────────────────────────────────────────────────

const loading = ref(false)
const error = ref('')
const activeTab = ref<'purchase' | 'depreciation' | 'repair' | 'roa'>('purchase')

const purchase = ref<PurchaseInfo | null>(null)
const netBook = ref<NetBookValue | null>(null)
const depreciationLog = ref<DepreciationEntry[]>([])
const depreciationSummary = ref<DepreciationSummary | null>(null)
const roa = ref<RoaCalculate | null>(null)
const repairStats = ref<RepairStats | null>(null)

// ── Load ───────────────────────────────────────────────────────────

async function loadAll() {
  if (!props.deviceSerialNo) return
  loading.value = true
  error.value = ''

  try {
    const [p, d, dep, r, s] = await Promise.allSettled([
      getPurchase(props.deviceSerialNo),
      calculateDepreciation(props.deviceSerialNo),
      getDepreciation(props.deviceSerialNo),
      calculateRoa(props.deviceSerialNo),
      getRepairStats(props.deviceSerialNo),
    ])

    purchase.value = p.status === 'fulfilled' ? p.value.purchase : null
    netBook.value = d.status === 'fulfilled' ? d.value : null
    if (dep.status === 'fulfilled') {
      depreciationLog.value = dep.value.depreciationLog || []
      depreciationSummary.value = dep.value.summary || null
    }
    roa.value = r.status === 'fulfilled' ? r.value : null
    repairStats.value = s.status === 'fulfilled' ? s.value : null
  } catch (e: any) {
    error.value = e.message || '加载失败'
  } finally {
    loading.value = false
  }
}

watch(() => [props.visible, props.deviceSerialNo], ([v]) => {
  if (v) loadAll()
})

// ── Format helpers ─────────────────────────────────────────────────

function fmtMoney(v: number | undefined | null): string {
  if (v == null) return '--'
  return '¥' + Number(v).toFixed(2)
}

function fmtPct(v: number | undefined | null): string {
  if (v == null) return '--'
  return Number(v).toFixed(1) + '%'
}

const tabs = [
  { key: 'purchase' as const, label: 'Purchase' },
  { key: 'depreciation' as const, label: 'Depreciation' },
  { key: 'repair' as const, label: 'Repair History' },
  { key: 'roa' as const, label: 'ROA' },
]
</script>

<template>
  <div v-if="visible" class="fixed inset-0 z-50 flex items-center justify-center" style="background: rgba(0,0,0,0.5)">
    <div class="panel" style="width: 720px; max-height: 85vh; display: flex; flex-direction: column">
      <!-- Header -->
      <div style="display: flex; align-items: center; justify-content: space-between; padding: 1rem; border-bottom: 1px solid var(--border-primary)">
        <h3 style="margin: 0; font-size: 1rem; font-weight: 600; color: var(--text-primary)">
          Asset Lifecycle — {{ deviceSerialNo }}
        </h3>
        <button class="btn" @click="emit('update:visible', false)" style="padding: 0.25rem 0.75rem; font-size: 0.8rem">X</button>
      </div>

      <!-- Loading / Error -->
      <div v-if="loading" style="padding: 2rem; text-align: center; color: var(--text-secondary)">Loading...</div>
      <div v-else-if="error" style="padding: 2rem; text-align: center; color: var(--text-error)">{{ error }}</div>

      <template v-else>
        <!-- Tabs -->
        <div style="display: flex; border-bottom: 1px solid var(--border-primary); padding: 0 1rem">
          <button
            v-for="t in tabs"
            :key="t.key"
            class="btn"
            @click="activeTab = t.key"
            :style="{
              padding: '0.5rem 1rem',
              borderBottom: activeTab === t.key ? '2px solid var(--accent-primary)' : '2px solid transparent',
              borderRadius: '0',
              fontWeight: activeTab === t.key ? '600' : '400',
              color: activeTab === t.key ? 'var(--accent-primary)' : 'var(--text-secondary)',
            }"
          >{{ t.label }}</button>
        </div>

        <!-- Content -->
        <div style="flex: 1; overflow-y: auto; padding: 1rem">

          <!-- Purchase Tab -->
          <div v-if="activeTab === 'purchase'">
            <div v-if="purchase" class="panel" style="padding: 0.75rem; margin-bottom: 0.75rem">
              <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 0.5rem; font-size: 0.8rem">
                <div>
                  <span style="color: var(--text-secondary)">Purchase Price: </span>
                  <strong>{{ fmtMoney(purchase.purchasePrice) }}</strong>
                </div>
                <div>
                  <span style="color: var(--text-secondary)">Replacement Value: </span>
                  <strong>{{ fmtMoney(purchase.replacementValue) }}</strong>
                </div>
                <div>
                  <span style="color: var(--text-secondary)">Purchase Date: </span>
                  <span>{{ purchase.purchaseDate || '--' }}</span>
                </div>
                <div>
                  <span style="color: var(--text-secondary)">Vendor: </span>
                  <span>{{ purchase.vendor || '--' }}</span>
                </div>
                <div>
                  <span style="color: var(--text-secondary)">Invoice No: </span>
                  <span>{{ purchase.invoiceNo || '--' }}</span>
                </div>
                <div>
                  <span style="color: var(--text-secondary)">Notes: </span>
                  <span>{{ purchase.notes || '--' }}</span>
                </div>
              </div>
            </div>
            <div v-else style="color: var(--text-secondary); padding: 1rem; text-align: center">
              No purchase record. Admin can add one via the procurement API.
            </div>
          </div>

          <!-- Depreciation Tab -->
          <div v-if="activeTab === 'depreciation'">
            <!-- Net Book Value Card -->
            <div v-if="netBook" class="panel" style="padding: 0.75rem; margin-bottom: 0.75rem">
              <div style="display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 0.5rem; font-size: 0.8rem">
                <div style="text-align: center">
                  <div style="color: var(--text-secondary); font-size: 0.7rem">Purchase Price</div>
                  <strong>{{ fmtMoney(netBook.purchasePrice) }}</strong>
                </div>
                <div style="text-align: center">
                  <div style="color: var(--text-secondary); font-size: 0.7rem">Accumulated Dep.</div>
                  <strong>{{ fmtMoney(netBook.totalDepreciation) }}</strong>
                </div>
                <div style="text-align: center">
                  <div style="color: var(--text-secondary); font-size: 0.7rem">Net Book Value</div>
                  <strong style="color: var(--accent-primary)">{{ fmtMoney(netBook.netBookValue) }}</strong>
                </div>
              </div>
              <div style="display: grid; grid-template-columns: 1fr 1fr 1fr 1fr; gap: 0.5rem; margin-top: 0.5rem; font-size: 0.75rem; color: var(--text-secondary)">
                <div>Monthly: {{ fmtMoney(netBook.monthlyDepreciation) }}</div>
                <div>Life: {{ netBook.usefulLifeMonths }}mo</div>
                <div>Applied: {{ netBook.depreciationMonths }}mo</div>
                <div>Remaining: {{ netBook.remainingMonths }}mo</div>
              </div>
            </div>
            <div v-else style="color: var(--text-secondary); padding: 1rem; text-align: center">
              No purchase record. Depreciation requires a purchase record.
            </div>

            <!-- Depreciation Log Table -->
            <div v-if="depreciationLog.length > 0">
              <div style="font-size: 0.8rem; font-weight: 600; color: var(--text-primary); margin-bottom: 0.5rem">
                Monthly Log ({{ depreciationSummary?.months || 0 }} entries)
              </div>
              <table style="width: 100%; font-size: 0.75rem; border-collapse: collapse">
                <thead>
                  <tr style="text-align: left; border-bottom: 1px solid var(--border-primary)">
                    <th style="padding: 0.25rem 0.5rem; color: var(--text-secondary)">Period</th>
                    <th style="padding: 0.25rem 0.5rem; color: var(--text-secondary); text-align: right">Opening</th>
                    <th style="padding: 0.25rem 0.5rem; color: var(--text-secondary); text-align: right">Dep.</th>
                    <th style="padding: 0.25rem 0.5rem; color: var(--text-secondary); text-align: right">Closing</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="entry in depreciationLog" :key="entry.id" style="border-bottom: 1px solid var(--border-secondary)">
                    <td style="padding: 0.25rem 0.5rem">{{ entry.period }}</td>
                    <td style="padding: 0.25rem 0.5rem; text-align: right">{{ fmtMoney(entry.openingValue) }}</td>
                    <td style="padding: 0.25rem 0.5rem; text-align: right; color: var(--text-error)">{{ fmtMoney(entry.depreciationAmount) }}</td>
                    <td style="padding: 0.25rem 0.5rem; text-align: right">{{ fmtMoney(entry.closingValue) }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>

          <!-- Repair Tab -->
          <div v-if="activeTab === 'repair'">
            <div v-if="repairStats" class="panel" style="padding: 0.75rem; margin-bottom: 0.75rem">
              <!-- Warning banner -->
              <div v-if="repairStats.exceedsReplacementThreshold" style="padding: 0.5rem; margin-bottom: 0.5rem; background: rgba(255, 100, 100, 0.1); border: 1px solid var(--text-error); font-size: 0.75rem; color: var(--text-error)">
                {{ repairStats.warning || 'Cumulative repair cost exceeds 75% of replacement value — consider replacement' }}
              </div>
              <div style="display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 0.5rem; font-size: 0.8rem">
                <div style="text-align: center">
                  <div style="color: var(--text-secondary); font-size: 0.7rem">Total Repairs</div>
                  <strong>{{ repairStats.totalRepairs }}</strong>
                </div>
                <div style="text-align: center">
                  <div style="color: var(--text-secondary); font-size: 0.7rem">Total Repair Cost</div>
                  <strong :style="{ color: repairStats.exceedsReplacementThreshold ? 'var(--text-error)' : 'var(--text-primary)' }">{{ fmtMoney(repairStats.totalRepairCost) }}</strong>
                </div>
                <div style="text-align: center">
                  <div style="color: var(--text-secondary); font-size: 0.7rem">Threshold Ratio</div>
                  <strong :style="{ color: repairStats.exceedsReplacementThreshold ? 'var(--text-error)' : 'var(--text-primary)' }">{{ fmtPct(repairStats.thresholdRatio * 100) }}</strong>
                </div>
              </div>
            </div>
            <div v-else style="color: var(--text-secondary); padding: 1rem; text-align: center">
              No repair history for this device.
            </div>

            <!-- Recent repairs -->
            <div v-if="repairStats?.recentRepairs?.length">
              <div style="font-size: 0.8rem; font-weight: 600; color: var(--text-primary); margin-bottom: 0.5rem">Recent Repairs</div>
              <table style="width: 100%; font-size: 0.75rem; border-collapse: collapse">
                <thead>
                  <tr style="text-align: left; border-bottom: 1px solid var(--border-primary)">
                    <th style="padding: 0.25rem 0.5rem; color: var(--text-secondary)">ID</th>
                    <th style="padding: 0.25rem 0.5rem; color: var(--text-secondary)">Status</th>
                    <th style="padding: 0.25rem 0.5rem; color: var(--text-secondary); text-align: right">Cost</th>
                    <th style="padding: 0.25rem 0.5rem; color: var(--text-secondary)">Vendor</th>
                    <th style="padding: 0.25rem 0.5rem; color: var(--text-secondary)">Date</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="r in repairStats.recentRepairs" :key="r.id" style="border-bottom: 1px solid var(--border-secondary)">
                    <td style="padding: 0.25rem 0.5rem; font-family: monospace; font-size: 0.65rem">{{ r.id.slice(0, 8) }}</td>
                    <td style="padding: 0.25rem 0.5rem">
                      <span class="badge" :class="{
                        'border': r.status === 'completed' || r.status === 'returned',
                      }">{{ r.status }}</span>
                    </td>
                    <td style="padding: 0.25rem 0.5rem; text-align: right">{{ fmtMoney(r.repairCost) }}</td>
                    <td style="padding: 0.25rem 0.5rem">{{ r.vendor || '--' }}</td>
                    <td style="padding: 0.25rem 0.5rem; font-size: 0.7rem">{{ r.createdAt?.slice(0, 10) || '--' }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>

          <!-- ROA Tab -->
          <div v-if="activeTab === 'roa'">
            <div v-if="roa" class="panel" style="padding: 0.75rem; margin-bottom: 0.75rem">
              <div style="display: grid; grid-template-columns: 1fr 1fr 1fr; gap: 0.5rem; font-size: 0.8rem; margin-bottom: 0.75rem">
                <div style="text-align: center">
                  <div style="color: var(--text-secondary); font-size: 0.7rem">Total Revenue</div>
                  <strong>{{ fmtMoney(roa.totalRevenue) }}</strong>
                </div>
                <div style="text-align: center">
                  <div style="color: var(--text-secondary); font-size: 0.7rem">Annual Revenue</div>
                  <strong>{{ fmtMoney(roa.annualRevenue) }}</strong>
                </div>
                <div style="text-align: center">
                  <div style="color: var(--text-secondary); font-size: 0.7rem">Years in Service</div>
                  <strong>{{ roa.yearsInService.toFixed(1) }}</strong>
                </div>
              </div>
              <div style="text-align: center; padding: 0.5rem; border: 1px solid var(--border-primary)">
                <div style="font-size: 0.7rem; color: var(--text-secondary)">ROA</div>
                <div style="font-size: 1.5rem; font-weight: 700; color: var(--accent-primary)">
                  {{ fmtPct(roa.roaPercent) }}
                </div>
                <div style="font-size: 0.75rem; margin-top: 0.25rem; color: var(--text-secondary)">
                  {{ roa.assessment }}
                </div>
              </div>
            </div>
            <div v-else style="color: var(--text-secondary); padding: 1rem; text-align: center">
              No ROA data. Requires purchase record and order history.
            </div>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>
