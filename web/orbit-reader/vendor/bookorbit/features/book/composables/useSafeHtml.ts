import { computed } from 'vue'
import { sanitizeDescriptionHtml } from '@/features/book/lib/description-html'

export function useSafeHtml(rawHtml: () => string | null | undefined) {
  return computed(() => {
    const content = rawHtml()
    if (!content) return ''
    return sanitizeDescriptionHtml(content)
  })
}
