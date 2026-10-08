import { createApp } from 'vue'
import { createPinia } from 'pinia'
import PrimeVue from 'primevue/config'
import ToastService from 'primevue/toastservice'
import ConfirmationService from 'primevue/confirmationservice'
import App from './App.vue'
import router from './router'
import { setRouter } from './api/client'
import { useThemeStore } from './stores/theme'
import { i18n } from './i18n'
import TerminalBrutalist from './theme/preset'

import '@unocss/reset/tailwind.css'
import 'virtual:uno.css'
import './assets/styles/base.css'

// Suppress benign ResizeObserver loop error from polluting Vite error overlay.
// This is a browser-internal timing issue — not an application bug.
// Must use capture phase to intercept before Vite's own window.error handler.
// See: https://github.com/WICG/resize-observer/issues/38
window.addEventListener('error', (e) => {
  if (e.message?.includes('ResizeObserver loop')) {
    e.stopImmediatePropagation()
    e.preventDefault()
  }
}, true) // capture phase

const app = createApp(App)
const pinia = createPinia()

app.use(pinia)
app.use(router)
app.use(i18n)
app.use(PrimeVue, {
  theme: {
    preset: TerminalBrutalist,
    options: {
      prefix: 'p',
      darkModeSelector: '[data-theme="dark"]',
    },
  },
  ripple: false,
  locale: {
    dayNames: ['星期日', '星期一', '星期二', '星期三', '星期四', '星期五', '星期六'],
    dayNamesShort: ['周日', '周一', '周二', '周三', '周四', '周五', '周六'],
    dayNamesMin: ['日', '一', '二', '三', '四', '五', '六'],
    monthNames: ['一月', '二月', '三月', '四月', '五月', '六月', '七月', '八月', '九月', '十月', '十一月', '十二月'],
    monthNamesShort: ['1月', '2月', '3月', '4月', '5月', '6月', '7月', '8月', '9月', '10月', '11月', '12月'],
    today: '今天',
    weekHeader: '周',
    firstDayOfWeek: 0,
    clear: '清除',
    chooseDate: '选择日期',
    chooseMonth: '选择月份',
    chooseYear: '选择年份',
    prevMonth: '上个月',
    nextMonth: '下个月',
    prevYear: '上一年',
    nextYear: '下一年',
    prevDecade: '上个十年',
    nextDecade: '下个十年',
    dateFormat: 'yy-mm-dd',
    fileSizeTypes: ['B', 'KB', 'MB', 'GB'],
  },
})
app.use(ToastService)
app.use(ConfirmationService)

setRouter(router)

const themeStore = useThemeStore()
themeStore.init()

app.mount('#app')
