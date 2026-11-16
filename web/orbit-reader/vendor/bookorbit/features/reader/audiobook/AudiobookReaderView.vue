<!--
  Shelf audiobook player — BO manifest + asset content + playback-state.
  Upstream slot: https://github.com/bookorbit/bookorbit (AGPL-3.0-only)
-->
<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  ArrowLeft,
  ChevronRight,
  List,
  Moon,
  Pause,
  Play,
  RotateCcw,
  RotateCw,
  SkipBack,
  SkipForward,
  Sun,
  X,
} from '@lucide/vue'

const props = defineProps<{ bookId: number; fileId: number }>()
const router = useRouter()

type ManifestAsset = {
  assetId: string
  format: string
  durationMs: number | null
  sequence: number
}

type Manifest = {
  revision: string
  book: { title: string; authors: string[]; hasCover: boolean }
  assets: ManifestAsset[]
  chapters: Array<{ title: string; assetId: string; startMs?: number; endMs?: number }>
}

const THEME_KEY = 'shelf-audiobook-theme'

const title = ref('Audiobook')
const author = ref('')
const coverUrl = ref<string | null>(null)
const error = ref<string | null>(null)
const audioRef = ref<HTMLAudioElement | null>(null)
const rate = ref(1)
const currentMs = ref(0)
const durationMs = ref(0)
const playing = ref(false)
const assetIndex = ref(0)
const assets = ref<ManifestAsset[]>([])
const chapters = ref<Array<{ title: string; assetId: string; startMs?: number; endMs?: number }>>([])
const manifestRevision = ref('')
const playbackRevision = ref(0)
const rates = [0.75, 1, 1.25, 1.5, 1.75, 2]
const showToc = ref(false)
/** `system` follows prefers-color-scheme; otherwise forced. */
const themePref = ref<'system' | 'light' | 'dark'>('system')
const systemDark = ref(true)
const scrubbing = ref(false)

const isDark = computed(() =>
  themePref.value === 'system' ? systemDark.value : themePref.value === 'dark',
)

const chaptersAreSeekPoints = computed(() => {
  if (chapters.value.length < 2) return false
  const first = chapters.value[0]?.assetId
  return chapters.value.every((c) => c.assetId === first)
})

const chapterIndex = computed(() => {
  const asset = assets.value[assetIndex.value]
  if (!asset || chapters.value.length === 0) return 0
  let best = 0
  for (let i = 0; i < chapters.value.length; i++) {
    const ch = chapters.value[i]
    if (ch.assetId !== asset.assetId) continue
    if ((ch.startMs ?? 0) <= currentMs.value) best = i
  }
  return best
})

const chapterLabel = computed(() => {
  const ch = chapters.value[chapterIndex.value]
  if (ch) return ch.title
  const asset = assets.value[assetIndex.value]
  if (!asset) return ''
  return `Track ${assetIndex.value + 1}`
})

const chapterScopedScrubber = computed(
  () =>
    chaptersAreSeekPoints.value ||
    (chapters.value.length === 1 && (chapters.value[0]?.endMs ?? 0) > 0),
)

const chapterStartMs = computed(() => {
  if (!chapterScopedScrubber.value) return 0
  return chapters.value[chapterIndex.value]?.startMs ?? 0
})

const chapterEndMs = computed(() => {
  if (!chapterScopedScrubber.value) return durationMs.value
  const ch = chapters.value[chapterIndex.value]
  const end = ch?.endMs ?? 0
  if (end > chapterStartMs.value) return end
  if (chapterIndex.value >= chapters.value.length - 1) {
    return durationMs.value > 0 ? durationMs.value : chapterStartMs.value
  }
  return chapters.value[chapterIndex.value + 1]?.startMs ?? durationMs.value
})

const chapterDurationMs = computed(() =>
  Math.max(0, chapterEndMs.value - chapterStartMs.value),
)

const chapterPositionMs = computed(() =>
  Math.max(0, Math.min(chapterDurationMs.value, currentMs.value - chapterStartMs.value)),
)

const chapterRemainingMs = computed(() =>
  Math.max(0, chapterDurationMs.value - chapterPositionMs.value),
)

const scrubberValue = computed(() => {
  if (chapterScopedScrubber.value) {
    if (chapterDurationMs.value <= 0) return 0
    return chapterPositionMs.value / chapterDurationMs.value
  }
  if (durationMs.value <= 0) return 0
  return currentMs.value / durationMs.value
})

const bookRemainingLabel = computed(() => {
  if (!chapterScopedScrubber.value || durationMs.value <= 0) return ''
  const leftMs = Math.max(0, durationMs.value - currentMs.value) / Math.max(0.1, rate.value)
  const total = Math.floor(leftMs / 1000)
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  if (h > 0) return `${h}h ${m}m left (${rate.value}×)`
  return `${m}m left (${rate.value}×)`
})

function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  if (h > 0) return `${h}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
  return `${m}:${String(s).padStart(2, '0')}`
}

function formatRemaining(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(total / 3600)
  const m = Math.floor((total % 3600) / 60)
  const s = total % 60
  if (h > 0) return `−${h}h ${String(m).padStart(2, '0')}m`
  if (m > 0) return `−${m}:${String(s).padStart(2, '0')}`
  return `−${formatClock(ms)}`
}

function onScrubInput(event: Event) {
  scrubbing.value = true
  const value = Number((event.target as HTMLInputElement).value)
  if (chapterScopedScrubber.value) {
    currentMs.value = chapterStartMs.value + Math.round(chapterDurationMs.value * value)
    return
  }
  currentMs.value = Math.round(durationMs.value * value)
}

function onScrubChange(event: Event) {
  const audio = audioRef.value
  if (!audio) return
  const value = Number((event.target as HTMLInputElement).value)
  if (chapterScopedScrubber.value) {
    const abs =
      chapterStartMs.value +
      Math.round(chapterDurationMs.value * Math.min(1, Math.max(0, value)))
    audio.currentTime = abs / 1000
    currentMs.value = abs
  } else {
    audio.currentTime = (durationMs.value * value) / 1000
  }
  scrubbing.value = false
  void savePosition()
}

function assetUrl(assetId: string): string {
  return `/api/v1/audiobooks/${props.bookId}/assets/${encodeURIComponent(assetId)}/content`
}

async function load() {
  const res = await fetch(`/api/v1/audiobooks/${props.bookId}/manifest`, {
    credentials: 'same-origin',
  })
  if (!res.ok) {
    error.value = 'Could not load the audiobook manifest.'
    return
  }
  const manifest = (await res.json()) as Manifest
  assets.value = manifest.assets ?? []
  chapters.value = manifest.chapters ?? []
  manifestRevision.value = manifest.revision
  title.value = manifest.book?.title || 'Audiobook'
  author.value = manifest.book?.authors?.[0] || ''
  if (manifest.book?.hasCover) {
    coverUrl.value = `/api/v1/books/${props.bookId}/cover`
  }
  if (assets.value.length === 0) {
    error.value = 'This audiobook has no playable tracks.'
    return
  }

  let startIndex = 0
  let startMs = 0
  const stateRes = await fetch(`/api/v1/audiobooks/${props.bookId}/playback-state`, {
    credentials: 'same-origin',
  })
  if (stateRes.ok) {
    const state = (await stateRes.json()) as {
      assetId: string
      positionMs: number
      revision: number
    }
    playbackRevision.value = state.revision ?? 0
    const idx = assets.value.findIndex((a) => a.assetId === state.assetId)
    if (idx >= 0) {
      startIndex = idx
      startMs = Math.max(0, state.positionMs || 0)
    }
  }

  assetIndex.value = startIndex
  await loadAsset(startIndex, startMs)
  setupMediaSession()
}

async function loadAsset(index: number, startMs = 0) {
  const asset = assets.value[index]
  const audio = audioRef.value
  if (!asset || !audio) return
  assetIndex.value = index
  audio.src = assetUrl(asset.assetId)
  audio.load()
  const apply = () => {
    if (startMs > 0) audio.currentTime = startMs / 1000
    currentMs.value = startMs
    onTimeUpdate()
  }
  if (audio.readyState >= 1) apply()
  else audio.addEventListener('loadedmetadata', apply, { once: true })
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
  const asset = assets.value[assetIndex.value]
  if (!audio || !asset || !manifestRevision.value) return
  const ms = Math.max(0, Math.round(audio.currentTime * 1000))
  const res = await fetch(`/api/v1/audiobooks/${props.bookId}/playback-state`, {
    method: 'PUT',
    credentials: 'same-origin',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      assetId: asset.assetId,
      positionMs: ms,
      capturedAt: new Date().toISOString(),
      operationId: crypto.randomUUID(),
      baseRevision: playbackRevision.value,
      manifestRevision: manifestRevision.value,
    }),
  })
  if (res.ok) {
    const state = (await res.json()) as { revision: number }
    playbackRevision.value = state.revision ?? playbackRevision.value
  }
}

function setupMediaSession() {
  if (!('mediaSession' in navigator)) return
  navigator.mediaSession.metadata = new MediaMetadata({
    title: title.value,
    artist: author.value || 'Shelf',
    album: chapterLabel.value || 'Audiobook',
    artwork: coverUrl.value
      ? [{ src: coverUrl.value, sizes: '512x512', type: 'image/jpeg' }]
      : [],
  })
  navigator.mediaSession.setActionHandler('play', () => void audioRef.value?.play())
  navigator.mediaSession.setActionHandler('pause', () => audioRef.value?.pause())
  navigator.mediaSession.setActionHandler('previoustrack', () => void prevTrack())
  navigator.mediaSession.setActionHandler('nexttrack', () => void nextTrack())
  navigator.mediaSession.setActionHandler('seekbackward', (details) => {
    const audio = audioRef.value
    if (!audio) return
    audio.currentTime = Math.max(0, audio.currentTime - (details.seekOffset ?? 15))
  })
  navigator.mediaSession.setActionHandler('seekforward', (details) => {
    const audio = audioRef.value
    if (!audio) return
    audio.currentTime = Math.min(
      audio.duration || audio.currentTime,
      audio.currentTime + (details.seekOffset ?? 15),
    )
  })
}

function onTimeUpdate() {
  const audio = audioRef.value
  if (!audio || scrubbing.value) return
  currentMs.value = Math.round(audio.currentTime * 1000)
  if (Number.isFinite(audio.duration)) durationMs.value = Math.round(audio.duration * 1000)
  if ('mediaSession' in navigator && Number.isFinite(audio.duration)) {
    navigator.mediaSession.setPositionState({
      duration: audio.duration,
      playbackRate: audio.playbackRate,
      position: audio.currentTime,
    })
  }
  scheduleSave()
}

async function seekChapter(index: number) {
  const ch = chapters.value[index]
  if (!ch) return
  const assetIdx = assets.value.findIndex((a) => a.assetId === ch.assetId)
  const startMs = Math.max(0, ch.startMs ?? 0)
  const wasPlaying = !(audioRef.value?.paused ?? true)
  if (assetIdx >= 0 && assetIdx !== assetIndex.value) {
    await loadAsset(assetIdx, startMs)
  } else if (audioRef.value) {
    audioRef.value.currentTime = startMs / 1000
    currentMs.value = startMs
  }
  showToc.value = false
  if (wasPlaying) void audioRef.value?.play()
  void savePosition()
}

async function onEnded() {
  playing.value = false
  if (chaptersAreSeekPoints.value && chapterIndex.value < chapters.value.length - 1) {
    await seekChapter(chapterIndex.value + 1)
    void audioRef.value?.play()
    return
  }
  if (assetIndex.value < assets.value.length - 1) {
    await loadAsset(assetIndex.value + 1, 0)
    void audioRef.value?.play()
  } else {
    void savePosition()
  }
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

async function prevTrack() {
  if (chapterScopedScrubber.value) {
    if (chapterPositionMs.value > 3000) {
      await seekChapter(chapterIndex.value)
      return
    }
    if (chapterIndex.value > 0) {
      await seekChapter(chapterIndex.value - 1)
      return
    }
    if (audioRef.value) audioRef.value.currentTime = 0
    return
  }
  if (assetIndex.value <= 0) {
    if (audioRef.value) audioRef.value.currentTime = 0
    return
  }
  const wasPlaying = !(audioRef.value?.paused ?? true)
  await loadAsset(assetIndex.value - 1, 0)
  if (wasPlaying) void audioRef.value?.play()
}

async function nextTrack() {
  if (chapterScopedScrubber.value) {
    if (chapterIndex.value < chapters.value.length - 1) {
      await seekChapter(chapterIndex.value + 1)
    }
    return
  }
  if (assetIndex.value >= assets.value.length - 1) return
  const wasPlaying = !(audioRef.value?.paused ?? true)
  await loadAsset(assetIndex.value + 1, 0)
  if (wasPlaying) void audioRef.value?.play()
}

function onRateChange() {
  const audio = audioRef.value
  if (!audio) return
  audio.playbackRate = rate.value
}

function goBack() {
  void savePosition().finally(() => router.push('/'))
}

function cycleTheme() {
  // system → dark → light → system
  if (themePref.value === 'system') themePref.value = 'dark'
  else if (themePref.value === 'dark') themePref.value = 'light'
  else themePref.value = 'system'
  try {
    localStorage.setItem(THEME_KEY, themePref.value)
  } catch {
    /* ignore */
  }
}

const themeTitle = computed(() => {
  if (themePref.value === 'system') return 'Theme: system'
  if (themePref.value === 'dark') return 'Theme: dark'
  return 'Theme: light'
})

let mediaQuery: MediaQueryList | null = null
function onSystemTheme(e: MediaQueryListEvent) {
  systemDark.value = e.matches
}

watch(rate, onRateChange)
onMounted(() => {
  try {
    const stored = localStorage.getItem(THEME_KEY)
    if (stored === 'light' || stored === 'dark' || stored === 'system') {
      themePref.value = stored
    }
  } catch {
    /* ignore */
  }
  mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
  systemDark.value = mediaQuery.matches
  mediaQuery.addEventListener('change', onSystemTheme)
  void load()
})
onUnmounted(() => {
  mediaQuery?.removeEventListener('change', onSystemTheme)
  if (saveTimer != null) window.clearTimeout(saveTimer)
  void savePosition()
})
</script>

<template>
  <div class="abook" :class="isDark ? 'abook--dark' : 'abook--light'" data-shelf-audiobook>
    <header class="abook__header">
      <button type="button" class="abook__icon-btn" title="Back to library" @click="goBack">
        <ArrowLeft :size="20" :stroke-width="1.75" />
      </button>
      <div class="abook__titles">
        <h1>{{ title }}</h1>
        <p v-if="author">{{ author }}</p>
      </div>
      <button
        type="button"
        class="abook__icon-btn"
        :title="themeTitle"
        :aria-label="themeTitle"
        @click="cycleTheme"
      >
        <Sun v-if="isDark" :size="20" :stroke-width="1.75" />
        <Moon v-else :size="20" :stroke-width="1.75" />
      </button>
    </header>

    <div class="abook__stage">
      <p v-if="error" class="abook__error">{{ error }}</p>

      <img v-if="coverUrl" :src="coverUrl" alt="" class="abook__cover" />
      <div v-else class="abook__cover abook__cover--empty" aria-hidden="true">
        <Play :size="40" :stroke-width="1.5" />
      </div>

      <button
        v-if="chapters.length > 1"
        type="button"
        class="abook__chapter"
        title="Chapters"
        @click="showToc = true"
      >
        <List :size="18" :stroke-width="1.75" />
        <span class="abook__chapter-label">{{ chapterLabel }}</span>
        <ChevronRight :size="16" :stroke-width="1.75" />
      </button>
      <p v-else class="abook__chapter abook__chapter--static">{{ chapterLabel }}</p>

      <audio
        ref="audioRef"
        class="sr-only"
        preload="metadata"
        @timeupdate="onTimeUpdate"
        @play="playing = true"
        @pause="playing = false; void savePosition()"
        @ended="onEnded"
        @loadedmetadata="onTimeUpdate"
      />

      <div class="abook__scrub">
        <div class="abook__times">
          <span>{{ formatClock(chapterScopedScrubber ? chapterPositionMs : currentMs) }}</span>
          <span>{{
            chapterScopedScrubber
              ? formatRemaining(chapterRemainingMs)
              : formatClock(durationMs)
          }}</span>
        </div>
        <input
          type="range"
          min="0"
          max="1"
          step="0.001"
          class="abook__range"
          :value="scrubberValue"
          :aria-label="chapterScopedScrubber ? 'Seek within chapter' : 'Seek'"
          @input="onScrubInput"
          @change="onScrubChange"
        />
        <p v-if="bookRemainingLabel" class="abook__book-left">{{ bookRemainingLabel }}</p>
      </div>

      <div class="abook__transport" role="group" aria-label="Playback">
        <button type="button" class="abook__icon-btn" title="Previous chapter" @click="prevTrack">
          <SkipBack :size="22" :stroke-width="1.75" />
        </button>
        <button type="button" class="abook__icon-btn" title="Back 15 seconds" @click="seekBy(-15)">
          <RotateCcw :size="22" :stroke-width="1.75" />
        </button>
        <button
          type="button"
          class="abook__play"
          :title="playing ? 'Pause' : 'Play'"
          @click="togglePlay"
        >
          <Pause v-if="playing" :size="28" :stroke-width="1.75" />
          <Play v-else :size="28" :stroke-width="1.75" />
        </button>
        <button type="button" class="abook__icon-btn" title="Forward 15 seconds" @click="seekBy(15)">
          <RotateCw :size="22" :stroke-width="1.75" />
        </button>
        <button type="button" class="abook__icon-btn" title="Next chapter" @click="nextTrack">
          <SkipForward :size="22" :stroke-width="1.75" />
        </button>
      </div>

      <div class="abook__rates" role="group" aria-label="Playback speed">
        <button
          v-for="r in rates"
          :key="r"
          type="button"
          class="abook__rate"
          :class="{ 'abook__rate--on': rate === r }"
          :aria-pressed="rate === r"
          @click="rate = r"
        >
          {{ r }}×
        </button>
      </div>
    </div>

    <Teleport to="body">
      <div
        v-if="showToc"
        class="abook-toc"
        :class="isDark ? 'abook--dark' : 'abook--light'"
        role="dialog"
        aria-modal="true"
        aria-label="Chapters"
      >
        <button type="button" class="abook-toc__scrim" aria-label="Close chapters" @click="showToc = false" />
        <aside class="abook-toc__panel">
          <div class="abook-toc__head">
            <h2>Chapters</h2>
            <button type="button" class="abook__icon-btn" title="Close" @click="showToc = false">
              <X :size="20" :stroke-width="1.75" />
            </button>
          </div>
          <ul class="abook-toc__list">
            <li v-for="(ch, i) in chapters" :key="i">
              <button
                type="button"
                class="abook-toc__item"
                :class="{ 'abook-toc__item--current': i === chapterIndex }"
                @click="seekChapter(i)"
              >
                <span class="abook-toc__num">{{ i + 1 }}</span>
                <span class="abook-toc__title">{{ ch.title }}</span>
                <span class="abook-toc__time">{{ formatClock(ch.startMs ?? 0) }}</span>
              </button>
            </li>
          </ul>
        </aside>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.abook {
  --ab-bg: #171916;
  --ab-panel: #21241f;
  --ab-text: #e4dcc8;
  --ab-muted: #b4b6a5;
  --ab-line: #484b3d;
  --ab-accent: #b5a16b;
  --ab-play: #e4dcc8;
  --ab-play-fg: #171916;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 100dvh;
  background: var(--ab-bg);
  color: var(--ab-text);
  font-family: 'IBM Plex Sans', Plex, 'Trebuchet MS', sans-serif;
}

.abook--light {
  --ab-bg: #f3efe4;
  --ab-panel: #fffdf7;
  --ab-text: #1c1f1a;
  --ab-muted: #5c5f54;
  --ab-line: #c9c4b4;
  --ab-accent: #8a7340;
  --ab-play: #1c1f1a;
  --ab-play-fg: #f3efe4;
}

.abook__header {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.75rem 1rem;
  border-bottom: 1px solid var(--ab-line);
}

.abook__titles {
  min-width: 0;
  flex: 1;
}

.abook__titles h1 {
  margin: 0;
  font-size: 1rem;
  font-weight: 500;
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.abook__titles p {
  margin: 0.15rem 0 0;
  font-size: 0.75rem;
  color: var(--ab-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.abook__icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 2.5rem;
  height: 2.5rem;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 3px;
  background: transparent;
  color: var(--ab-text);
  cursor: pointer;
}

.abook__icon-btn:hover {
  border-color: var(--ab-line);
  background: var(--ab-panel);
}

.abook__icon-btn:focus-visible {
  outline: 2px solid var(--ab-accent);
  outline-offset: 2px;
}

.abook__stage {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1.25rem;
  padding: 1.5rem;
}

.abook__error {
  margin: 0;
  color: #c45c4a;
  font-size: 0.875rem;
}

.abook__cover {
  width: 12rem;
  height: 12rem;
  object-fit: cover;
  border-radius: 3px;
  box-shadow: 0 18px 48px #0b100d66;
}

.abook__cover--empty {
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--ab-panel);
  color: var(--ab-muted);
  border: 1px solid var(--ab-line);
  box-shadow: none;
}

.abook__chapter {
  display: inline-flex;
  max-width: 28rem;
  align-items: center;
  gap: 0.5rem;
  margin: 0;
  padding: 0.4rem 0.75rem;
  border: 1px solid var(--ab-line);
  border-radius: 3px;
  background: var(--ab-panel);
  color: var(--ab-text);
  font: inherit;
  font-size: 0.95rem;
  font-weight: 500;
  cursor: pointer;
}

.abook__chapter--static {
  border: none;
  background: transparent;
  cursor: default;
}

.abook__chapter-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.abook__scrub {
  display: flex;
  width: 100%;
  max-width: 28rem;
  flex-direction: column;
  gap: 0.35rem;
}

.abook__times {
  display: flex;
  justify-content: space-between;
  font-family: 'IBM Plex Mono', PlexMono, ui-monospace, monospace;
  font-size: 0.75rem;
  font-variant-numeric: tabular-nums;
  color: var(--ab-muted);
}

.abook__range {
  -webkit-appearance: none;
  appearance: none;
  width: 100%;
  height: 2.5rem;
  margin: 0;
  background: transparent;
  cursor: pointer;
}

.abook__range:focus-visible {
  outline: none;
}

.abook__range::-webkit-slider-runnable-track {
  height: 3px;
  border-radius: 1px;
  background: var(--ab-line);
}

.abook__range::-webkit-slider-thumb {
  -webkit-appearance: none;
  appearance: none;
  width: 14px;
  height: 14px;
  margin-top: -5.5px;
  border: 2px solid var(--ab-bg);
  border-radius: 50%;
  background: var(--ab-accent);
  box-shadow: 0 0 0 1px var(--ab-accent);
}

.abook__range::-moz-range-track {
  height: 3px;
  border: none;
  border-radius: 1px;
  background: var(--ab-line);
}

.abook__range::-moz-range-thumb {
  width: 14px;
  height: 14px;
  border: 2px solid var(--ab-bg);
  border-radius: 50%;
  background: var(--ab-accent);
  box-shadow: 0 0 0 1px var(--ab-accent);
}

.abook__book-left {
  margin: 0;
  text-align: center;
  font-size: 0.75rem;
  color: var(--ab-muted);
}

.abook__transport {
  display: flex;
  align-items: center;
  gap: 0.35rem;
}

.abook__play {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 3.5rem;
  height: 3.5rem;
  margin: 0 0.25rem;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: var(--ab-play);
  color: var(--ab-play-fg);
  cursor: pointer;
}

.abook__play:focus-visible {
  outline: 2px solid var(--ab-accent);
  outline-offset: 3px;
}

.abook__rates {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 0.35rem;
}

.abook__rate {
  min-width: 2.75rem;
  padding: 0.35rem 0.55rem;
  border: 1px solid var(--ab-line);
  border-radius: 3px;
  background: transparent;
  color: var(--ab-text);
  font: inherit;
  font-size: 0.8rem;
  cursor: pointer;
}

.abook__rate--on {
  background: var(--ab-text);
  border-color: var(--ab-text);
  color: var(--ab-bg);
}

.abook__rate:focus-visible {
  outline: 2px solid var(--ab-accent);
  outline-offset: 2px;
}

.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  white-space: nowrap;
  border: 0;
}

.abook-toc {
  --ab-bg: #171916;
  --ab-panel: #21241f;
  --ab-text: #e4dcc8;
  --ab-muted: #b4b6a5;
  --ab-line: #484b3d;
  --ab-accent: #b5a16b;
  position: fixed;
  inset: 0;
  z-index: 80;
  display: flex;
  justify-content: flex-end;
}

.abook-toc.abook--light {
  --ab-bg: #f3efe4;
  --ab-panel: #fffdf7;
  --ab-text: #1c1f1a;
  --ab-muted: #5c5f54;
  --ab-line: #c9c4b4;
  --ab-accent: #8a7340;
}

.abook-toc__scrim {
  flex: 1;
  border: none;
  background: #0b100d88;
  cursor: pointer;
}

.abook-toc__panel {
  display: flex;
  width: min(22rem, 92vw);
  flex-direction: column;
  background: var(--ab-panel);
  color: var(--ab-text);
  border-left: 1px solid var(--ab-line);
  box-shadow: -12px 0 40px #0b100d55;
  font-family: 'IBM Plex Sans', Plex, 'Trebuchet MS', sans-serif;
}

.abook-toc__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0.85rem 0.75rem 0.85rem 1.1rem;
  border-bottom: 1px solid var(--ab-line);
}

.abook-toc__head h2 {
  margin: 0;
  font-family: Instrument, Georgia, serif;
  font-size: 1.35rem;
  font-weight: 400;
}

.abook-toc__list {
  margin: 0;
  padding: 0.5rem 0 1.5rem;
  list-style: none;
  overflow: auto;
}

.abook-toc__item {
  display: grid;
  grid-template-columns: 2rem 1fr auto;
  gap: 0.65rem;
  align-items: baseline;
  width: 100%;
  padding: 0.7rem 1.1rem;
  border: none;
  border-left: 2px solid transparent;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.abook-toc__item:hover {
  background: color-mix(in srgb, var(--ab-accent) 12%, transparent);
}

.abook-toc__item--current {
  border-left-color: var(--ab-accent);
  background: color-mix(in srgb, var(--ab-accent) 16%, transparent);
}

.abook-toc__num {
  font-family: 'IBM Plex Mono', PlexMono, ui-monospace, monospace;
  font-size: 0.75rem;
  color: var(--ab-muted);
}

.abook-toc__title {
  font-size: 0.92rem;
}

.abook-toc__time {
  font-family: 'IBM Plex Mono', PlexMono, ui-monospace, monospace;
  font-size: 0.72rem;
  color: var(--ab-muted);
}
</style>
