import { onMounted, onUnmounted } from 'vue'

export function useWakeLock() {
  let wakeLock: WakeLockSentinel | null = null

  async function acquire() {
    if (!('wakeLock' in navigator)) return
    try {
      wakeLock = await navigator.wakeLock.request('screen')
      wakeLock.addEventListener('release', () => {
        wakeLock = null
      })
    } catch {
      // Silently ignore - browser may deny or not support
    }
  }

  function release() {
    wakeLock?.release().catch(() => {})
    wakeLock = null
  }

  function onVisibilityChange() {
    if (document.visibilityState === 'visible' && !wakeLock) {
      acquire()
    }
  }

  onMounted(() => {
    acquire()
    document.addEventListener('visibilitychange', onVisibilityChange)
  })

  onUnmounted(() => {
    release()
    document.removeEventListener('visibilitychange', onVisibilityChange)
  })
}
