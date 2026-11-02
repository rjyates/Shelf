import type { SortSpec } from '@bookorbit/types'
import type { ColumnDef } from './tableColumnSchema'
import { DEFAULT_SORT, copySort } from '../lib/sort-defaults'

/**
 * `getDefaultSort` is where clearing a column sort lands. It has to follow the view rather than be
 * hardcoded: a collection clears back to its membership order, and without that the third click on
 * any header would strand the view on title with no way back.
 */
export function useTableSorting(
  getSort: () => SortSpec[],
  isSortEnabled: () => boolean,
  emitSort: (sort: SortSpec[]) => void,
  getDefaultSort: () => SortSpec[] = () => DEFAULT_SORT,
) {
  function clearedSort(): SortSpec[] {
    return copySort(getDefaultSort())
  }

  function isSortableColumn(col: ColumnDef): boolean {
    return !!col.sortField && isSortEnabled()
  }

  function getSortDir(sortField: string): 'asc' | 'desc' | null {
    return getSort().find((s) => s.field === sortField)?.dir ?? null
  }

  function getSortPriority(sortField: string | null): number {
    if (!sortField) return 0
    return getSort().findIndex((s) => s.field === sortField) + 1
  }

  function handleColumnSort(sortField: string | null, event: MouseEvent): void {
    if (!sortField || !isSortEnabled()) return

    if (event.shiftKey && getSort().length > 0) {
      const existing = getSort().find((s) => s.field === sortField)
      let next: SortSpec[]
      if (!existing) {
        next = [...getSort(), { field: sortField as SortSpec['field'], dir: 'asc' }]
      } else if (existing.dir === 'asc') {
        next = getSort().map((s) => (s.field === sortField ? { ...s, dir: 'desc' as const } : s))
      } else {
        next = getSort().filter((s) => s.field !== sortField)
        if (next.length === 0) next = clearedSort()
      }
      emitSort(next)
      return
    }

    const current = getSort().find((s) => s.field === sortField)
    let next: SortSpec[]
    if (!current) {
      next = [{ field: sortField as SortSpec['field'], dir: 'asc' }]
    } else if (current.dir === 'asc') {
      next = [{ field: sortField as SortSpec['field'], dir: 'desc' }]
    } else {
      next = clearedSort()
    }
    emitSort(next)
  }

  function removeSortField(field: string): void {
    const next = getSort().filter((s) => s.field !== field)
    emitSort(next.length > 0 ? next : clearedSort())
  }

  return { isSortableColumn, getSortDir, getSortPriority, handleColumnSort, removeSortField }
}
