import type { InjectionKey, Component, ShallowRef } from 'vue'

/** Header actions component — provided by App.vue, written by page, read by DefaultLayout */
export const HEADER_ACTIONS_KEY: InjectionKey<ShallowRef<Component | null>> = Symbol('headerActions')
