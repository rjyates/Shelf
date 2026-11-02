<script setup lang="ts">
import { onMounted } from 'vue'
import { Sheet, SheetContent } from '@/components/ui/sheet'
import TranslationPanel from './TranslationPanel.vue'
import { useTranslation } from '../composables/useTranslation'
import { copyToClipboard } from '@/lib/clipboard'

const props = defineProps<{
  text: string
}>()

const emit = defineEmits<{
  close: []
}>()

const { supportedLanguages, targetLang, loading, error, result, setTargetLang, translate, reset } = useTranslation()

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

function handleOpenChange(open: boolean) {
  if (!open) emit('close')
}

onMounted(runTranslation)
</script>

<template>
  <Sheet :open="true" @update:open="handleOpenChange">
    <SheetContent side="bottom" class="p-0 rounded-t-xl max-h-[60vh] overflow-y-auto">
      <!-- Drag handle -->
      <div class="flex justify-center pt-3 pb-1">
        <div class="w-9 h-1 rounded-full bg-muted-foreground/30" />
      </div>

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
    </SheetContent>
  </Sheet>
</template>
