import { ref, onMounted, onUnmounted, type Ref } from 'vue'
import type { ColumnDef } from './useTableColumns'

const NON_RESIZABLE_COLUMNS = new Set<string>(['lockRow', 'cover', 'read'])

export function useTableResize(
  scrollContainerRef: Ref<HTMLDivElement | null>,
  displayColumns: Ref<ColumnDef[]>,
  setColumnWidth: (id: string, px: number) => void,
  isReadOnly: Ref<boolean>,
) {
  const resizingColumnId = ref<string | null>(null)
  const resizeStartX = ref(0)
  const resizeStartWidth = ref(0)

  function isResizableCol(col: ColumnDef): boolean {
    return !isReadOnly.value && !NON_RESIZABLE_COLUMNS.has(col.id)
  }

  function startResize(e: MouseEvent, colId: string, currentWidth: number) {
    if (isReadOnly.value) return
    if (NON_RESIZABLE_COLUMNS.has(colId)) return
    resizingColumnId.value = colId
    resizeStartX.value = e.clientX
    resizeStartWidth.value = currentWidth
    e.preventDefault()
  }

  function onResizeMove(e: MouseEvent) {
    if (!resizingColumnId.value) return
    const delta = e.clientX - resizeStartX.value
    setColumnWidth(resizingColumnId.value, resizeStartWidth.value + delta)
  }

  function stopResize() {
    resizingColumnId.value = null
  }

  function autoFitColumn(colId: string): void {
    if (NON_RESIZABLE_COLUMNS.has(colId)) return
    if (!scrollContainerRef.value) return
    const table = scrollContainerRef.value.querySelector('table')
    if (!table) return

    const column = displayColumns.value.find((currentColumn) => currentColumn.id === colId)
    if (!column) return

    let maxWidth = column.minWidth
    const headerCell = table.querySelector(`thead [data-col-id="${colId}"] [data-col-label]`) as HTMLElement | null
    if (headerCell) {
      maxWidth = Math.max(maxWidth, headerCell.scrollWidth + 32)
    }

    const cells = table.querySelectorAll(`tbody tr td[data-col-id="${colId}"]`)
    cells.forEach((cell) => {
      const element = cell as HTMLElement
      maxWidth = Math.max(maxWidth, element.scrollWidth + 16)
    })

    setColumnWidth(colId, Math.min(400, maxWidth))
  }

  function autoFitAllColumns(): void {
    for (const col of displayColumns.value) {
      if (!NON_RESIZABLE_COLUMNS.has(col.id)) autoFitColumn(col.id)
    }
  }

  onMounted(() => {
    document.addEventListener('mousemove', onResizeMove)
    document.addEventListener('mouseup', stopResize)
  })

  onUnmounted(() => {
    document.removeEventListener('mousemove', onResizeMove)
    document.removeEventListener('mouseup', stopResize)
  })

  return {
    resizingColumnId,
    isResizableCol,
    startResize,
    autoFitColumn,
    autoFitAllColumns,
  }
}
