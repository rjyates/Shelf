import { ref } from 'vue'

const SIDEBAR_PIN_KEY_PREFIX = 'reader:sidebar:pinned'

export function getReaderSidebarPinStorageKey(bookFileId: number): string {
  return `${SIDEBAR_PIN_KEY_PREFIX}:${bookFileId}`
}

function getLocalStorage(): Storage | null {
  try {
    return typeof window === 'undefined' ? null : window.localStorage
  } catch {
    return null
  }
}

function readStoredSidebarPinned(key: string): boolean {
  try {
    return getLocalStorage()?.getItem(key) === 'true'
  } catch {
    return false
  }
}

function writeStoredSidebarPinned(key: string, pinned: boolean): void {
  try {
    const storage = getLocalStorage()
    if (!storage) return
    if (pinned) {
      storage.setItem(key, 'true')
    } else {
      storage.removeItem(key)
    }
  } catch {
    // Local storage can be unavailable in private windows.
  }
}

export function useReaderSidebarPin(bookFileId: number) {
  const storageKey = getReaderSidebarPinStorageKey(bookFileId)
  const isSidebarPinned = ref(readStoredSidebarPinned(storageKey))

  function setSidebarPinned(pinned: boolean): void {
    isSidebarPinned.value = pinned
    writeStoredSidebarPinned(storageKey, pinned)
  }

  function toggleSidebarPinned(): void {
    setSidebarPinned(!isSidebarPinned.value)
  }

  function shouldCloseAfterNavigation(): boolean {
    return !isSidebarPinned.value
  }

  return {
    isSidebarPinned,
    setSidebarPinned,
    toggleSidebarPinned,
    shouldCloseAfterNavigation,
  }
}
