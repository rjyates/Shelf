import { type Ref, onActivated, onDeactivated } from 'vue'

export function useScrollRestoreOnActivate(containerRef: Ref<HTMLElement | null>) {
  let savedScrollTop = 0

  onDeactivated(() => {
    savedScrollTop = containerRef.value?.scrollTop ?? 0
  })

  onActivated(() => {
    requestAnimationFrame(() => {
      const el = containerRef.value
      if (!el) return
      const overflowY = getComputedStyle(el).overflowY
      if (overflowY !== 'auto' && overflowY !== 'scroll') return
      el.scrollTo({ top: savedScrollTop })
      el.dispatchEvent(new Event('scroll'))
    })
  })
}
