import type { SortSpec } from '@bookorbit/types'

/** What a book view sorts by when nothing is stored and the user has not chosen. */
export const DEFAULT_SORT: SortSpec[] = [{ field: 'title', dir: 'asc' }]

/** Manual collections are curated, so they open in the order books were added to them. */
export const COLLECTION_DEFAULT_SORT: SortSpec[] = [{ field: 'collectionOrder', dir: 'asc' }]

/** Defaults are shared module constants, so hand out copies rather than the array itself. */
export function copySort(sort: SortSpec[]): SortSpec[] {
  return sort.map((item) => ({ ...item }))
}

export function sortsEqual(left: SortSpec[], right: SortSpec[]): boolean {
  return left.length === right.length && left.every((item, index) => item.field === right[index]?.field && item.dir === right[index]?.dir)
}
