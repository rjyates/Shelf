import { computed, ref, watch } from 'vue'
import type { Ref } from 'vue'
import { watchDebounced } from '@vueuse/core'
import { storage } from '@/services/storage'
import type { CustomMetadataFieldSummary, TableLayoutState, TableViewType } from '@bookorbit/types'
import {
  type ColumnId,
  type CellType,
  type ColumnDef,
  LOCK_ROW_COLUMN_DEF,
  COLUMN_DEFS,
  COLUMN_DEF_MAP,
  DEFAULT_ORDER,
  DEFAULT_HIDDEN,
  DEFAULT_WIDTHS,
  buildCustomColumnDef,
  isCustomColumnId,
} from './tableColumnSchema'

export type { ColumnId, CellType, ColumnDef }
export { LOCK_ROW_COLUMN_DEF, COLUMN_DEFS, COLUMN_DEF_MAP, DEFAULT_ORDER, DEFAULT_HIDDEN, DEFAULT_WIDTHS }

const FIXED_WIDTH_COLUMN_IDS = new Set<string>(['cover', 'read'])

function resolveColumnWidth(id: string, fallback: number, userWidths: Record<string, number>): number {
  if (FIXED_WIDTH_COLUMN_IDS.has(id)) return fallback
  return userWidths[id] ?? fallback
}

function storageKey(viewType: TableViewType): string {
  return `bookorbit:tableLayout:${viewType}`
}

function loadPinnedColumns(raw: unknown, knownIds: Set<string>): Record<string, 'left' | 'right' | null> {
  if (!raw || typeof raw !== 'object') return {}
  const result: Record<string, 'left' | 'right' | null> = {}
  for (const [id, dir] of Object.entries(raw as Record<string, unknown>)) {
    if (knownIds.has(id) && (dir === 'left' || dir === 'right' || dir === null)) {
      result[id] = dir
    }
  }
  return result
}

function resolvePinnedColumn(
  userPins: Record<string, 'left' | 'right' | null>,
  id: string,
  fallback: 'left' | 'right' | null,
): 'left' | 'right' | null {
  if (Object.prototype.hasOwnProperty.call(userPins, id)) {
    return userPins[id] ?? null
  }
  return fallback
}

function loadLayout(viewType: TableViewType): TableLayoutState {
  try {
    const raw = storage.get<TableLayoutState | null>(storageKey(viewType), null)
    if (!raw || !Array.isArray(raw.columnOrder) || !Array.isArray(raw.hiddenColumns)) {
      return { columnOrder: [...DEFAULT_ORDER], hiddenColumns: [...DEFAULT_HIDDEN], columnWidths: { ...DEFAULT_WIDTHS } }
    }
    const staticIds = new Set(DEFAULT_ORDER)
    // Keep static known IDs and any custom:N IDs (validated against active fields later)
    const order = (raw.columnOrder as unknown[]).filter((id): id is string => typeof id === 'string' && (staticIds.has(id) || isCustomColumnId(id)))
    const missing = DEFAULT_ORDER.filter((id) => !order.includes(id))
    const finalOrder = [...order, ...missing]
    const hidden = (raw.hiddenColumns as unknown[]).filter(
      (id): id is string => typeof id === 'string' && (staticIds.has(id) || isCustomColumnId(id)),
    )
    const widths: Record<string, number> = { ...DEFAULT_WIDTHS }
    for (const [id, w] of Object.entries(raw.columnWidths ?? {})) {
      if ((staticIds.has(id) || isCustomColumnId(id)) && typeof w === 'number' && w > 0) widths[id] = w
    }
    return {
      columnOrder: finalOrder,
      hiddenColumns: hidden,
      columnWidths: widths,
      pinnedColumns: loadPinnedColumns(raw.pinnedColumns, new Set(finalOrder)),
    }
  } catch {
    return { columnOrder: [...DEFAULT_ORDER], hiddenColumns: [...DEFAULT_HIDDEN], columnWidths: { ...DEFAULT_WIDTHS }, pinnedColumns: {} }
  }
}

export function useTableColumns(viewType: TableViewType, customFields?: Ref<CustomMetadataFieldSummary[]>) {
  const layout = ref<TableLayoutState>(loadLayout(viewType))

  const customColumnDefs = computed<ColumnDef[]>(() => {
    if (!customFields?.value?.length) return []
    return customFields.value
      .filter((f) => !f.archivedAt)
      .sort((a, b) => a.displayOrder - b.displayOrder || a.label.localeCompare(b.label))
      .map(buildCustomColumnDef)
  })

  const combinedColumnMap = computed<Map<string, ColumnDef>>(() => {
    const map = new Map<string, ColumnDef>(COLUMN_DEF_MAP)
    for (const def of customColumnDefs.value) {
      map.set(def.id, def)
    }
    return map
  })

  // Reconcile layout when custom fields change: add new fields hidden, remove stale custom IDs.
  // Uses { immediate: true } so custom columns are added on mount when fields are pre-loaded,
  // but skips cleanup when newDefs is empty (fields may still be loading - don't clobber storage).
  watch(
    customColumnDefs,
    (newDefs, oldDefs) => {
      const activeCustomIds = new Set(newDefs.map((d) => d.id))
      const isInitialRun = oldDefs === undefined

      let staleIds: string[]
      if (isInitialRun) {
        // On immediate run: only clean up orphaned IDs if we actually have field data.
        // If the field list is still empty, fields are still loading - preserve stored layout as-is.
        staleIds = newDefs.length > 0 ? layout.value.columnOrder.filter((id) => isCustomColumnId(id) && !activeCustomIds.has(id)) : []
      } else {
        // Subsequent changes: only remove IDs that were previously active but are now gone.
        const previousCustomIds = new Set(oldDefs.map((d) => d.id))
        staleIds = layout.value.columnOrder.filter((id) => isCustomColumnId(id) && previousCustomIds.has(id) && !activeCustomIds.has(id))
      }

      const newIds = [...activeCustomIds].filter((id) => !layout.value.columnOrder.includes(id))

      if (staleIds.length === 0 && newIds.length === 0) return

      const staleSet = new Set(staleIds)
      const pinnedColumns = layout.value.pinnedColumns ?? {}
      layout.value = {
        ...layout.value,
        columnOrder: [...layout.value.columnOrder.filter((id) => !staleSet.has(id)), ...newIds],
        hiddenColumns: [...layout.value.hiddenColumns.filter((id) => !staleSet.has(id)), ...newIds],
        columnWidths: Object.fromEntries(Object.entries(layout.value.columnWidths).filter(([id]) => !staleSet.has(id))),
        pinnedColumns: Object.fromEntries(Object.entries(pinnedColumns).filter(([id]) => !staleSet.has(id))),
      }
    },
    { immediate: true },
  )

  watchDebounced(layout, (v) => storage.set(storageKey(viewType), v), { deep: true, debounce: 500 })

  const visibleColumns = computed<ColumnDef[]>(() => {
    const hiddenSet = new Set(layout.value.hiddenColumns)
    const userPins = layout.value.pinnedColumns ?? {}
    return layout.value.columnOrder
      .filter((id) => !hiddenSet.has(id))
      .map((id) => {
        const def = combinedColumnMap.value.get(id)
        if (!def) return null
        return {
          ...def,
          defaultWidth: resolveColumnWidth(id, def.defaultWidth, layout.value.columnWidths),
          pinned: resolvePinnedColumn(userPins, id, def.pinned),
        }
      })
      .filter((def): def is ColumnDef => def !== null)
  })

  const allColumns = computed<(ColumnDef & { visible: boolean })[]>(() => {
    const hiddenSet = new Set(layout.value.hiddenColumns)
    const userPins = layout.value.pinnedColumns ?? {}
    return layout.value.columnOrder
      .map((id) => {
        const def = combinedColumnMap.value.get(id)
        if (!def) return null
        return { ...def, visible: !hiddenSet.has(id), pinned: resolvePinnedColumn(userPins, id, def.pinned) }
      })
      .filter((def): def is ColumnDef & { visible: boolean } => def !== null)
  })

  function toggleColumn(id: string): void {
    const hidden = new Set(layout.value.hiddenColumns)
    if (hidden.has(id)) {
      hidden.delete(id)
    } else {
      hidden.add(id)
    }
    layout.value = { ...layout.value, hiddenColumns: [...hidden] }
  }

  function setColumnOrder(order: string[]): void {
    const seen = new Set<string>()
    const allKnown = combinedColumnMap.value
    const deduped = order.filter((id) => allKnown.has(id) && !seen.has(id) && seen.add(id) !== undefined)
    const missing = [...allKnown.keys()].filter((id) => !seen.has(id))
    layout.value = { ...layout.value, columnOrder: [...deduped, ...missing] }
  }

  function setColumnWidth(id: string, px: number): void {
    if (FIXED_WIDTH_COLUMN_IDS.has(id)) return
    const def = combinedColumnMap.value.get(id)
    const min = def?.minWidth ?? 40
    const max = 800
    const clamped = Math.min(Math.max(px, min), max)
    layout.value = {
      ...layout.value,
      columnWidths: { ...layout.value.columnWidths, [id]: clamped },
    }
  }

  function setLayout(nextLayout: TableLayoutState): void {
    const allKnown = combinedColumnMap.value
    const order = nextLayout.columnOrder.filter((id) => allKnown.has(id))
    const missing = [...allKnown.keys()].filter((id) => !order.includes(id))
    const hidden = nextLayout.hiddenColumns.filter((id) => allKnown.has(id))
    const widths: Record<string, number> = { ...DEFAULT_WIDTHS }
    for (const [id, width] of Object.entries(nextLayout.columnWidths ?? {})) {
      if (allKnown.has(id) && typeof width === 'number' && width > 0) {
        widths[id] = width
      }
    }
    const pins: Record<string, 'left' | 'right' | null> = {}
    if (nextLayout.pinnedColumns) {
      for (const [id, dir] of Object.entries(nextLayout.pinnedColumns)) {
        if (allKnown.has(id) && (dir === 'left' || dir === 'right' || dir === null)) {
          pins[id] = dir
        }
      }
    }
    layout.value = {
      columnOrder: [...order, ...missing],
      hiddenColumns: hidden,
      columnWidths: widths,
      pinnedColumns: pins,
    }
  }

  function pinColumn(id: string, side: 'left' | 'right'): void {
    const currentPins = { ...layout.value.pinnedColumns }
    if (currentPins[id] === side) return
    const MAX_PINNED = 3
    const pinnedOnSide = Object.values(currentPins).filter((s) => s === side).length
    if (pinnedOnSide >= MAX_PINNED) return
    currentPins[id] = side
    layout.value = { ...layout.value, pinnedColumns: currentPins }
  }

  function unpinColumn(id: string): void {
    const currentPins = { ...layout.value.pinnedColumns }
    currentPins[id] = null
    layout.value = { ...layout.value, pinnedColumns: currentPins }
  }

  function resetLayout(): void {
    storage.remove(storageKey(viewType))
    // Keep active custom columns in the picker but reset them to hidden.
    const customIds = customColumnDefs.value.map((d) => d.id)
    layout.value = {
      columnOrder: [...DEFAULT_ORDER, ...customIds],
      hiddenColumns: [...DEFAULT_HIDDEN, ...customIds],
      columnWidths: { ...DEFAULT_WIDTHS },
      pinnedColumns: {},
    }
  }

  return {
    layout,
    combinedColumnMap,
    visibleColumns,
    allColumns,
    toggleColumn,
    setColumnOrder,
    setColumnWidth,
    setLayout,
    pinColumn,
    unpinColumn,
    resetLayout,
  }
}
