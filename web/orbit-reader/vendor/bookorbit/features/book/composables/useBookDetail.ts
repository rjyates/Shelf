import { ref } from 'vue'
import { api } from '@/lib/api'
import type { BookDetail } from '@bookorbit/types'

export function useBookDetail() {
  const detail = ref<BookDetail | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)
  const notFound = ref(false)
  let activeRequestId = 0

  async function fetch(bookId: number) {
    const requestId = ++activeRequestId
    const shouldClearDetail = detail.value?.id !== bookId
    loading.value = true
    error.value = null
    notFound.value = false
    if (shouldClearDetail) detail.value = null

    try {
      const res = await api(`/api/v1/books/${bookId}`)
      if (requestId === activeRequestId && res.status === 404) {
        notFound.value = true
      }
      if (requestId === activeRequestId && res.status !== 404) {
        if (!res.ok) throw new Error(`HTTP ${res.status}`)

        const nextDetail = (await res.json()) as BookDetail
        if (requestId === activeRequestId) {
          detail.value = nextDetail
        }
      }
    } catch (e) {
      if (requestId === activeRequestId) {
        error.value = e instanceof Error ? e.message : 'Failed to load book'
      }
    }

    if (requestId === activeRequestId) {
      loading.value = false
    }
  }

  return { detail, loading, error, notFound, fetch }
}
