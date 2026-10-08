import { ref, watch, onUnmounted } from 'vue'
import { Chart } from '@antv/g2'
import { useChartTheme, type ThemeMode } from './useChartTheme'

/**
 * Composable for @antv/g2 v5 chart lifecycle with theme support.
 *
 * Chart is NOT created on mount — the component must call renderChart(data)
 * after data loads. The createSpec callback receives (data, theme) so charts
 * can use theme-appropriate colors.
 *
 * When the user toggles light/dark mode, the chart is re-created automatically
 * with the new theme, using the last-rendered data.
 */
export function useG2(createSpec: (data: any[], theme: ThemeMode) => Record<string, unknown>) {
  const containerRef = ref<HTMLElement>()
  let chart: Chart | null = null
  let lastData: any[] = []
  const error = ref('')
  const { mode: themeMode, g2Theme } = useChartTheme()

  function renderChart(data: any[]) {
    lastData = data
    const el = containerRef.value
    if (!el) { error.value = 'Container not found'; return }
    if (!data || data.length === 0) return
    try {
      chart?.destroy()
      chart = new Chart({ container: el, autoFit: true, theme: g2Theme.value as any })
      chart.options(createSpec(data, themeMode.value!))
      chart.render()
      error.value = ''
    } catch (e: any) {
      error.value = e.message || 'Chart init failed'
    }
  }

  // Re-render on theme toggle
  watch(themeMode, () => {
    if (lastData.length > 0) renderChart(lastData)
  })

  onUnmounted(() => {
    chart?.destroy()
    chart = null
  })

  return { containerRef, error, renderChart }
}
