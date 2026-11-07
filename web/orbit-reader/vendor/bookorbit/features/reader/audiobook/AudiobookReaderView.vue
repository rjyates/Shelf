<!--
  Shelf single-file audiobook player (mp3 / m4b).
  Not the upstream Book Orbit audiobook chrome — that needs /audiobooks/* APIs we do not ship yet.
  Upstream stub slot: https://github.com/bookorbit/bookorbit (AGPL-3.0-only)
-->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

const props = defineProps<{ bookId: number; fileId: number }>()
const router = useRouter()

const title = ref('Audiobook')
const author = ref('')
const coverUrl = ref<string | null>(null)
const error = ref<string | null>(null)
const audioRef = ref<HTMLAudioElement | null>(null)
const rate = ref(1)
const currentMs = ref(0)
const durationMs = ref(0)
const playing = ref(false)

const rates = [0.75, 1, 1.25, 1.5, 1.75, 2]

const progressLabel = computed(() => {
  const cur = formatClock(currentMs.value)
  const dur = durationMs.value > 0 ? formatClock(durationMs.value) : '—'
  return `${cur} / ${dur}`
})

function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  if (h > 0) {
    return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  }
  return `${m}:${String(s).padStart(2, '0')}`
}

function parseAudioms(position: string | null | undefined): number | null {
  if (!position?.startsWith('audioms(') || !position.endsWith(')')) return null
  const n = Number.parseInt(position.slice('audioms('.length, -1), 10)
  return Number.isFinite(n) && n >= 0 ? n : null
}

async function loadMeta() {
  const res = await fetch('/api/v1/books', { credentials: 'same-origin' })
  if (!res.ok) {
    error.value = 'Could not load the audiobook.'
    return
  }
  const books = (await res.json()) as Array<{
    id: number
    title: string
    author: string
    position?: string | null
    has_cover?: boolean
    epub?: { position?: string | null; has_cover?: boolean } | null
  }>
  const book = books.find((b) => b.id === props.bookId)
  if (!book) {
    error.value = 'Audiobook not found.'
    return
  }
  title.value = book.title || 'Audiobook'
  author.value = book.author || ''
  if (book.has_cover || book.epub?.has_cover) {
    coverUrl.value = `/api/v1/books/${props.bookId}/cover`
  }
  const resumeMs = parseAudioms(book.position ?? book.epub?.position ?? null)
  const audio = audioRef.value
  if (audio && resumeMs != null && resumeMs > 0) {
    const apply = () => {
      audio.currentTime = resumeMs / 1000
      currentMs.value = resumeMs
    }
    if (audio.readyState >= 1) apply()
    else audio.addEventListener('loadedmetadata', apply, { once: true })
  }
  setupMediaSession()
}

let saveTimer: number | null = null

function scheduleSave() {
  if (saveTimer != null) window.clearTimeout(saveTimer)
  saveTimer = window.setTimeout(() => {
    void savePosition()
  }, 1500)
}

async function savePosition() {
  const audio = audioRef.value
  if (!audio || !Number.isFinite(audio.currentTime)) return
  const ms = Math.max(0, Math.round(audio.currentTime * 1000))
  const duration = audio.duration
  const percentage =
    Number.isFinite(duration) && duration > 0 ? Math.min(1, Math.max(0, audio.currentTime / duration)) : 0
  await fetch(`/api/v1/books/${props.bookId}/location`, {
    method: 'POST',
    credentials: 'same-origin',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ position: `audioms(${ms})`, percentage }),
  })
}

function setupMediaSession() {
  if (!('mediaSession' in navigator)) return
  navigator.mediaSession.metadata = new MediaMetadata({
    title: title.value,
    artist: author.value || 'Shelf',
    album: 'Audiobook',
    artwork: coverUrl.value
      ? [{ src: coverUrl.value, sizes: '512x512', type: 'image/jpeg' }]
      : [],
  })
  navigator.mediaSession.setActionHandler('play', () => void audioRef.value?.play())
  navigator.mediaSession.setActionHandler('pause', () => audioRef.value?.pause())
  navigator.mediaSession.setActionHandler('seekbackward', (details) => {
    const audio = audioRef.value
    if (!audio) return
    audio.currentTime = Math.max(0, audio.currentTime - (details.seekOffset ?? 15))
  })
  navigator.mediaSession.setActionHandler('seekforward', (details) => {
    const audio = audioRef.value
    if (!audio) return
    audio.currentTime = Math.min(audio.duration || audio.currentTime, audio.currentTime + (details.seekOffset ?? 15))
  })
  navigator.mediaSession.setActionHandler('seekto', (details) => {
    const audio = audioRef.value
    if (!audio || details.seekTime == null) return
    audio.currentTime = details.seekTime
  })
}

function onTimeUpdate() {
  const audio = audioRef.value
  if (!audio) return
  currentMs.value = Math.round(audio.currentTime * 1000)
  if (Number.isFinite(audio.duration)) {
    durationMs.value = Math.round(audio.duration * 1000)
  }
  if ('mediaSession' in navigator && Number.isFinite(audio.duration)) {
    navigator.mediaSession.setPositionState({
      duration: audio.duration,
      playbackRate: audio.playbackRate,
      position: audio.currentTime,
    })
  }
  scheduleSave()
}

function togglePlay() {
  const audio = audioRef.value
  if (!audio) return
  if (audio.paused) void audio.play()
  else audio.pause()
}

function seekBy(seconds: number) {
  const audio = audioRef.value
  if (!audio) return
  audio.currentTime = Math.max(0, Math.min(audio.duration || 0, audio.currentTime + seconds))
}

function onRateChange() {
  const audio = audioRef.value
  if (!audio) return
  audio.playbackRate = rate.value
}

function goBack() {
  void savePosition().finally(() => router.push('/'))
}

watch(rate, onRateChange)

onMounted(() => {
  void loadMeta()
})

onUnmounted(() => {
  if (saveTimer != null) window.clearTimeout(saveTimer)
  void savePosition()
  if ('mediaSession' in navigator) {
    navigator.mediaSession.setActionHandler('play', null)
    navigator.mediaSession.setActionHandler('pause', null)
    navigator.mediaSession.setActionHandler('seekbackward', null)
    navigator.mediaSession.setActionHandler('seekforward', null)
    navigator.mediaSession.setActionHandler('seekto', null)
  }
})
</script>

<template>
  <div class="flex h-full flex-col bg-background text-foreground">
    <header class="flex items-center gap-3 border-b px-4 py-3">
      <button type="button" class="text-sm underline-offset-2 hover:underline" @click="goBack">
        Back
      </button>
      <div class="min-w-0 flex-1">
        <h1 class="truncate text-base font-medium">{{ title }}</h1>
        <p v-if="author" class="truncate text-xs text-muted-foreground">{{ author }}</p>
      </div>
    </header>

    <div class="flex flex-1 flex-col items-center justify-center gap-6 p-6">
      <p v-if="error" class="text-sm text-destructive">{{ error }}</p>
      <img
        v-if="coverUrl"
        :src="coverUrl"
        alt=""
        class="h-48 w-48 rounded object-cover shadow-sm"
      />
      <div
        v-else
        class="flex h-48 w-48 items-center justify-center rounded bg-muted text-sm text-muted-foreground"
      >
        Audio
      </div>

      <audio
        ref="audioRef"
        class="hidden"
        preload="metadata"
        :src="`/api/v1/books/${bookId}/file`"
        @timeupdate="onTimeUpdate"
        @play="playing = true"
        @pause="playing = false; void savePosition()"
        @ended="playing = false; void savePosition()"
        @loadedmetadata="onTimeUpdate"
      />

      <p class="font-mono text-sm tabular-nums">{{ progressLabel }}</p>

      <div class="flex items-center gap-3">
        <button type="button" class="rounded border px-3 py-2 text-sm" @click="seekBy(-15)">
          −15s
        </button>
        <button
          type="button"
          class="rounded bg-primary px-5 py-2 text-sm text-primary-foreground"
          @click="togglePlay"
        >
          {{ playing ? 'Pause' : 'Play' }}
        </button>
        <button type="button" class="rounded border px-3 py-2 text-sm" @click="seekBy(15)">
          +15s
        </button>
      </div>

      <label class="flex items-center gap-2 text-sm">
        Speed
        <select v-model.number="rate" class="rounded border bg-background px-2 py-1">
          <option v-for="r in rates" :key="r" :value="r">{{ r }}×</option>
        </select>
      </label>
    </div>
  </div>
</template>
