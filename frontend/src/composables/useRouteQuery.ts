import { reactive, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryValue } from 'vue-router'

/**
 * Syncs a reactive filter object with URL query parameters.
 *
 * - On creation: reads initial values from route.query, falling back to defaults
 * - On filter change: pushes changed params to URL via router.replace
 * - On browser back/forward: reads new values from route.query into the reactive
 * - Preserves query params not managed by this composable
 * - Bookmarks and shared links restore filter state
 */
export function useRouteQuery<T extends Record<string, string>>(defaults: T): T {
  const route = useRoute()
  const router = useRouter()

  const filters = reactive({ ...defaults }) as T

  // Initialize from current route query, falling back to defaults
  for (const key of Object.keys(defaults)) {
    const val = route.query[key]
    ;(filters as any)[key] = typeof val === 'string' ? val : defaults[key]
  }

  // Watch route.query for browser back/forward navigation.
  // Uses a shallow snapshot to detect when the query object changes.
  watch(
    () => ({ ...route.query }),
    (newQuery) => {
      for (const key of Object.keys(defaults)) {
        const val = (newQuery as any)[key]
        ;(filters as any)[key] = typeof val === 'string' ? val : defaults[key]
      }
    },
  )

  // Watch filters and push changes to URL.
  // Only pushes when the query would actually change to avoid redundant navigation.
  watch(
    () => ({ ...filters }),
    (newFilters) => {
      const query: Record<string, LocationQueryValue | LocationQueryValue[]> = {}

      // Preserve query params not managed by this composable
      for (const [k, v] of Object.entries(route.query)) {
        if (!(k in defaults)) {
          query[k] = v
        }
      }

      let changed = false
      for (const key of Object.keys(defaults)) {
        const val = (newFilters as any)[key]
        const current = route.query[key]
        if (val) {
          if (current !== val) changed = true
          query[key] = val
        } else if (current !== undefined) {
          changed = true
        }
      }

      if (changed) {
        router.replace({ query })
      }
    },
  )

  return filters
}
