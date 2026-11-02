import { computed, ref } from 'vue'
import { isBookPlaceholder, type BookSlot } from './useBookWindow'

// Each entry pairs a loaded book id with its absolute position in the full
// listing. For fully-loaded lists the index equals the array position; for a
// windowed listing it equals the book's slot index, so a book reached via the
// jump rail keeps its true position even though only a sparse subset is loaded.
type NavEntry = { id: number; index: number }

const entries = ref<NavEntry[]>([])
const total = ref(0)
let loadMoreCallback: (() => Promise<void>) | null = null

const positionById = computed(() => {
  const map = new Map<number, number>()
  entries.value.forEach((entry, pos) => map.set(entry.id, pos))
  return map
})

// Skip the reassignment when the listing is structurally identical. A re-sync
// with the same ids/indexes (e.g. while a lazy block is still in flight) must
// not invalidate `entries`, or watchers that recompute the next/prev id would
// fire in a loop and starve the in-flight fetch.
function sameEntries(a: NavEntry[], b: NavEntry[]): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (a[i]!.id !== b[i]!.id || a[i]!.index !== b[i]!.index) return false
  }
  return true
}

export function useBookNavigation() {
  const setBookContext = (ids: number[], totalCount: number) => {
    const next = ids.map((id, index) => ({ id, index }))
    if (!sameEntries(entries.value, next)) entries.value = next
    total.value = totalCount
  }

  const setBookSlotContext = (slots: BookSlot[], totalCount: number) => {
    const next: NavEntry[] = []
    for (let i = 0; i < slots.length; i++) {
      const slot = slots[i]!
      if (!isBookPlaceholder(slot)) next.push({ id: slot.id, index: i })
    }
    if (!sameEntries(entries.value, next)) entries.value = next
    total.value = totalCount
  }

  const registerLoadMore = (cb: (() => Promise<void>) | null) => {
    loadMoreCallback = cb
  }

  const getNextId = async (currentId: number) => {
    const pos = positionById.value.get(currentId)
    if (pos === undefined) return null
    const current = entries.value[pos]!
    const next = entries.value[pos + 1]
    if (next && next.index === current.index + 1) return next.id
    if (current.index + 1 < total.value && loadMoreCallback) {
      const beforeLength = entries.value.length
      await loadMoreCallback()
      if (entries.value.length === beforeLength) {
        // No more books were loaded, avoid infinite loop
        return null
      }
      const newPos = positionById.value.get(currentId)
      if (newPos === undefined) return null
      const reloaded = entries.value[newPos]!
      const after = entries.value[newPos + 1]
      if (after && after.index === reloaded.index + 1) return after.id
    }
    return null
  }

  const getPrevId = (currentId: number) => {
    const pos = positionById.value.get(currentId)
    if (pos === undefined || pos === 0) return null
    const current = entries.value[pos]!
    const prev = entries.value[pos - 1]!
    return prev.index === current.index - 1 ? prev.id : null
  }

  const hasContext = computed(() => entries.value.length > 0)
  const currentIndex = (currentId: number) => {
    const pos = positionById.value.get(currentId)
    return pos === undefined ? -1 : entries.value[pos]!.index
  }

  return {
    bookIds: computed(() => entries.value.map((entry) => entry.id)),
    total: computed(() => total.value),
    setBookContext,
    setBookSlotContext,
    registerLoadMore,
    getNextId,
    getPrevId,
    hasContext,
    currentIndex,
  }
}
