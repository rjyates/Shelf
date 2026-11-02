import { onMounted, onUnmounted, ref } from 'vue'

const AUTO_HIDE_DELAY_MS = 3000
export const READER_CHROME_PIN_KEY = 'reader:chrome:pinned'

function getLocalStorage(): Storage | null {
  try {
    return typeof window === 'undefined' ? null : window.localStorage
  } catch {
    return null
  }
}

/** Default pinned (true). Explicit `'false'` means the user unpinned. */
export function readStoredChromePinned(): boolean {
  try {
    const raw = getLocalStorage()?.getItem(READER_CHROME_PIN_KEY)
    if (raw === null || raw === undefined) return true
    return raw !== 'false'
  } catch {
    return true
  }
}

export function writeStoredChromePinned(pinned: boolean): void {
  try {
    getLocalStorage()?.setItem(READER_CHROME_PIN_KEY, pinned ? 'true' : 'false')
  } catch {
    // Private mode / quota — keep in-memory only.
  }
}

export function useVisibility() {
  const isPinned = ref(readStoredChromePinned())
  // Show chrome on open; unpinned sessions still auto-hide after idle.
  const headerVisible = ref(true)
  const footerVisible = ref(true)

  let isVisibilityLocked = false
  let hideTimer: ReturnType<typeof setTimeout> | null = null

  function clearHideTimer() {
    if (!hideTimer) return
    clearTimeout(hideTimer)
    hideTimer = null
  }

  function scheduleHide() {
    clearHideTimer()
    hideTimer = setTimeout(() => {
      if (!isPinned.value && !isVisibilityLocked) {
        headerVisible.value = false
        footerVisible.value = false
      }
      hideTimer = null
    }, AUTO_HIDE_DELAY_MS)
  }

  function handleMiddleTap() {
    if (isVisibilityLocked) return
    if (isPinned.value) return

    if (headerVisible.value || footerVisible.value) {
      clearHideTimer()
      headerVisible.value = false
      footerVisible.value = false
      return
    }

    headerVisible.value = true
    footerVisible.value = true
    scheduleHide()
  }

  function togglePinned() {
    isPinned.value = !isPinned.value
    writeStoredChromePinned(isPinned.value)
    headerVisible.value = true
    footerVisible.value = true

    if (isPinned.value) {
      clearHideTimer()
    } else {
      scheduleHide()
    }
  }

  function showHeader() {
    if (isVisibilityLocked) {
      headerVisible.value = true
      return
    }

    if (!isPinned.value) {
      headerVisible.value = true
      scheduleHide()
    }
  }

  function showFooter() {
    if (isVisibilityLocked) {
      footerVisible.value = true
      return
    }

    if (!isPinned.value) {
      footerVisible.value = true
      scheduleHide()
    }
  }

  function hideOverlays(force = false) {
    if (isVisibilityLocked && !force) return

    clearHideTimer()
    // Temporary for TTS/narration — do not persist; remount restores last toggle.
    isPinned.value = false
    headerVisible.value = false
    footerVisible.value = false
  }

  function setVisibilityLock(locked: boolean) {
    isVisibilityLocked = locked

    clearHideTimer()

    if (locked) {
      headerVisible.value = true
      return
    }

    if (isPinned.value) {
      headerVisible.value = true
      footerVisible.value = true
      return
    }

    headerVisible.value = false
    footerVisible.value = false
  }

  onMounted(() => {
    if (!isPinned.value) scheduleHide()
  })

  onUnmounted(clearHideTimer)

  return { headerVisible, footerVisible, isPinned, handleMiddleTap, togglePinned, showHeader, showFooter, hideOverlays, setVisibilityLock }
}
