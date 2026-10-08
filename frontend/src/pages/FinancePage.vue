<script setup lang="ts">
import FinanceDashboard from '@/components/finance/FinanceDashboard.vue'
import InvoiceList from '@/components/finance/InvoiceList.vue'
import SettlementList from '@/components/finance/SettlementList.vue'
</script>

<template>
  <div class="finance-page">
    <!-- MODULE-08 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-08</span>
      <div class="structure-line" />
      <span class="module-page-label">财务税务</span>
    </div>

    <FinanceDashboard />
    <InvoiceList />
    <SettlementList />
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>

<style scoped>
.finance-page {
  padding: 1rem 0;
}
</style>
