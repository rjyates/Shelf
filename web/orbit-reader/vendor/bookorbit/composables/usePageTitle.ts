/**
 * Shelf stub for Book Orbit's usePageTitle.
 * Derived from Book Orbit (AGPL-3.0-only): https://github.com/bookorbit/bookorbit
 */
import { onBeforeUnmount, toValue, watch, type MaybeRefOrGetter } from 'vue'

export function usePageTitle(title: MaybeRefOrGetter<string | null | undefined>) {
  const stop = watch(
    () => toValue(title),
    (value) => {
      if (typeof document === 'undefined') return
      document.title = value?.trim() ? `${value} · Shelf` : 'Shelf · Reader'
    },
    { immediate: true },
  )
  onBeforeUnmount(stop)
}
