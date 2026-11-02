<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import TranslationPanel from './TranslationPanel.vue'
import { useTranslation } from '../composables/useTranslation'
import { copyToClipboard } from '@/lib/clipboard'

const props = defineProps<{
  text: string
  position: { x: number; y: number; showBelow: boolean }
}>()

const emit = defineEmits<{
  close: []
}>()

const { supportedLanguages, targetLang, loading, error, result, setTargetLang, translate, reset } = useTranslation()

const popoverRef = ref<HTMLElement | null>(null)
const popoverStyle = ref({ left: '0px', top: '0px' })
let resizeObserver: ResizeObserver | null = null

const VIEWPORT_MARGIN = 8
const FALLBACK_WIDTH = 320
const FALLBACK_HEIGHT = 240

function clamp(value: number, min: number, max: number) {
  if (max < min) return min
  return Math.min(Math.max(value, min), max)
}

function updatePosition() {
  if (typeof window === 'undefined') return

  const popoverWidth = popoverRef.value?.offsetWidth || FALLBACK_WIDTH
  const popoverHeight = popoverRef.value?.offsetHeight || FALLBACK_HEIGHT

  const desiredLeft = props.position.x - popoverWidth / 2
  const desiredTop = props.position.showBelow ? props.position.y : props.position.y - popoverHeight

  popoverStyle.value = {
    left: `${clamp(desiredLeft, VIEWPORT_MARGIN, window.innerWidth - popoverWidth - VIEWPORT_MARGIN)}px`,
    top: `${clamp(desiredTop, VIEWPORT_MARGIN, window.innerHeight - popoverHeight - VIEWPORT_MARGIN)}px`,
  }
}

async function runTranslation() {
  reset()
  await translate(props.text, targetLang.value)
}

function handleChangeLanguage(code: string) {
  setTargetLang(code)
  runTranslation()
}

function handleCopy() {
  if (result.value) {
    void copyToClipboard(result.value.translatedText)
  }
}

onMounted(async () => {
  await nextTick()
  updatePosition()
  window.addEventListener('resize', updatePosition)
  if (typeof ResizeObserver !== 'undefined' && popoverRef.value) {
    resizeObserver = new ResizeObserver(() => updatePosition())
    resizeObserver.observe(popoverRef.value)
  }
  await runTranslation()
})

onBeforeUnmount(() => {
  window.removeEventListener('resize', updatePosition)
  resizeObserver?.disconnect()
  resizeObserver = null
})

watch(
  () => [props.position.x, props.position.y, props.position.showBelow],
  () => updatePosition(),
  { immediate: true },
)

watch([loading, result, error], async () => {
  await nextTick()
  updatePosition()
})
</script>

<template>
  <Teleport to="body">
    <div class="fixed inset-0 z-[61]" @click="emit('close')" />
    <div ref="popoverRef" class="fixed z-[62] select-none" :style="popoverStyle" @mousedown.stop @click.stop>
      <div
        class="bg-card text-card-foreground rounded-lg shadow-xl border border-border w-80 min-w-64 min-h-28 max-w-[calc(100vw-16px)] max-h-[calc(100vh-16px)] overflow-auto resize"
      >
        <TranslationPanel
          :originalText="text"
          :loading="loading"
          :error="error"
          :result="result"
          :targetLang="targetLang"
          :languages="supportedLanguages"
          @changeLanguage="handleChangeLanguage"
          @retry="runTranslation"
          @copy="handleCopy"
        />
      </div>
    </div>
  </Teleport>
</template>
