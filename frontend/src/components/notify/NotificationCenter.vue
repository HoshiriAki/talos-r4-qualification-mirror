<script setup lang="ts">
import { ref, onMounted } from 'vue'
import OverlayPanel from 'primevue/overlaypanel'
import Badge from 'primevue/badge'
import Button from 'primevue/button'
import { listNotifications, markRead, type NotificationMessage } from '@/api/notify'

const overlay = ref<InstanceType<typeof OverlayPanel> | null>(null)
const messages = ref<NotificationMessage[]>([])
const unreadCount = ref(0)
const loading = ref(false)

const EVENT_LABELS: Record<string, string> = {
  shipped: '已发货',
  return_reminder: '归还提醒',
  damage_forfeit: '损坏定损',
}

function eventLabel(type: string): string {
  return EVENT_LABELS[type] || type
}

function formatTime(iso: string): string {
  if (!iso) return ''
  const d = new Date(iso)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

async function fetchMessages() {
  loading.value = true
  try {
    const res = await listNotifications({ page: 1, pageSize: 20 })
    messages.value = res.messages
    unreadCount.value = res.unreadCount
  } catch {
    // silently fail
  } finally {
    loading.value = false
  }
}

async function handleMarkRead(msgId?: string) {
  await markRead(msgId)
  await fetchMessages()
}

function toggleOverlay(event: Event) {
  fetchMessages()
  overlay.value?.toggle(event)
}

defineExpose({ toggleOverlay })
</script>

<template>
  <div class="relative">
    <Button
      severity="secondary"
      variant="text"
      class="!p-2 relative"
      @click="toggleOverlay"
      aria-label="通知中心"
    >
      <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
        <path d="M13.73 21a2 2 0 0 1-3.46 0" />
      </svg>
      <Badge
        v-if="unreadCount > 0"
        :value="unreadCount > 99 ? '99+' : String(unreadCount)"
        severity="danger"
        class="absolute -top-1 -right-1"
      />
    </Button>

    <OverlayPanel ref="overlay" appendTo="body" class="!w-[360px] !max-h-[480px]" :pt="{
      root: { class: 'notification-panel' },
      content: { class: '!p-0' },
    }">
      <div class="flex flex-col max-h-[440px]">
        <!-- Header -->
        <div class="flex items-center justify-between px-4 py-3 border-b border-border">
          <span class="font-mono text-sm text-text-primary font-bold">通知中心</span>
          <Button
            v-if="unreadCount > 0"
            severity="secondary"
            variant="text"
            size="small"
            class="!text-xs !text-text-muted hover:!text-text-primary"
            @click="handleMarkRead()"
          >
            全部已读
          </Button>
        </div>

        <!-- Messages -->
        <div v-if="loading" class="px-4 py-8 text-center">
          <span class="font-mono text-xs text-text-muted">加载中...</span>
        </div>

        <div
          v-else-if="messages.length === 0"
          class="px-4 py-8 text-center"
        >
          <span class="font-mono text-xs text-text-muted">暂无通知</span>
        </div>

        <div
          v-else
          class="overflow-y-auto"
        >
          <div
            v-for="msg in messages"
            :key="msg.id"
            class="px-4 py-3 border-b border-border cursor-pointer transition-colors duration-100"
            :class="msg.readAt ? 'bg-bg-base hover:bg-bg-base' : 'bg-bg-base font-bold hover:bg-bg-base'"
            @click="handleMarkRead(msg.id)"
          >
            <div class="flex items-start justify-between gap-2">
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2 mb-1">
                  <span class="font-mono text-xs text-text-muted">{{ eventLabel(msg.eventType) }}</span>
                  <span
                    v-if="!msg.readAt"
                    class="h-2 w-2 bg-accent shrink-0"
                  />
                </div>
                <p class="text-sm text-text-primary leading-relaxed break-words">
                  {{ msg.body }}
                </p>
              </div>
            </div>
            <div class="mt-1">
              <span class="font-mono text-[11px] text-text-muted">{{ formatTime(msg.createdAt) }}</span>
            </div>
          </div>
        </div>
      </div>
    </OverlayPanel>
  </div>
</template>

<style scoped>
.notification-panel {
  border: 1px solid var(--border-color, #333) !important;
  box-shadow: none !important;
  border-radius: var(--radius-sm) !important;
  background: var(--bg-base, #161618) !important;
}
</style>
