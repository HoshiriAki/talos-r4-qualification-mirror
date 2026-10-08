<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { useRouter } from 'vue-router'
import { Icon } from '@iconify/vue'
import { useTenantPreviewStore } from '@/stores/tenantPreview'

const preview = useTenantPreviewStore()
const router = useRouter()
const now = ref(Date.now())
const exiting = ref(false)
const timer = window.setInterval(() => { now.value = Date.now() }, 1000)

const remaining = computed(() => {
  const expires = Date.parse(preview.session?.expiresAt ?? '')
  if (!Number.isFinite(expires)) return '--:--'
  const seconds = Math.max(0, Math.ceil((expires - now.value) / 1000))
  const minutes = Math.floor(seconds / 60)
  return `${String(minutes).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
})

async function exitPreview() {
  const tenantId = preview.session?.tenantId
  exiting.value = true
  try {
    await preview.exit()
  } finally {
    exiting.value = false
    await router.replace(tenantId ? `/control/governance/${tenantId}` : '/control/governance')
  }
}

onBeforeUnmount(() => window.clearInterval(timer))
</script>

<template>
  <section v-if="preview.active && preview.session" class="preview-banner" role="status" aria-live="polite">
    <div class="preview-banner__identity">
      <Icon icon="material-symbols:visibility-outline" width="18" height="18" />
      <strong>READ-ONLY TENANT PREVIEW</strong>
      <span>{{ preview.session.tenantName }}</span>
      <code>{{ preview.session.tenantSlug }}</code>
    </div>
    <div class="preview-banner__controls">
      <span>剩余 {{ remaining }}</span>
      <button type="button" :disabled="exiting" @click="exitPreview">
        {{ exiting ? '正在退出…' : '退出预览' }}
      </button>
    </div>
  </section>
</template>

<style scoped>
.preview-banner {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  min-height: 44px;
  padding: 8px 32px;
  color: var(--status-info);
  background: var(--bg-elevated);
  border-bottom: 1px solid var(--status-info);
  box-shadow: 0 3px 0 color-mix(in srgb, var(--status-info) 28%, transparent);
  font: 12px var(--font-mono);
}
.preview-banner__identity,
.preview-banner__controls {
  display: flex;
  align-items: center;
  gap: 10px;
}
.preview-banner strong { letter-spacing: .08em; }
.preview-banner code { color: var(--text-secondary); }
.preview-banner button {
  padding: 6px 12px;
  color: var(--text-primary);
  background: transparent;
  border: 1px solid var(--status-info);
  border-radius: var(--radius-md);
  cursor: pointer;
}
.preview-banner button:disabled { cursor: wait; opacity: .6; }
@media (max-width: 720px) {
  .preview-banner { align-items: stretch; padding: 10px 16px; flex-direction: column; }
  .preview-banner__identity { flex-wrap: wrap; }
  .preview-banner__controls { justify-content: space-between; }
}
</style>
