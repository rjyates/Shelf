import { ref } from 'vue'
import { translateWithGoogle } from '../services/google-translate'
import { translateWithAzure } from '../services/azure-translate'
import { SUPPORTED_LANGUAGES, TRANSLATION_CHAR_LIMIT } from '../services/translation.types'
import type { TranslationResult } from '../services/translation.types'

const STORAGE_KEY = 'translation_target_lang'
const DEFAULT_LANG = 'en'

function loadPersistedLang(): string {
  try {
    const stored = localStorage.getItem(STORAGE_KEY)
    if (stored && SUPPORTED_LANGUAGES.some((l) => l.code === stored)) {
      return stored
    }
  } catch {
    // localStorage unavailable (e.g. SSR or private mode)
  }
  return DEFAULT_LANG
}

function persistLang(code: string): void {
  try {
    localStorage.setItem(STORAGE_KEY, code)
  } catch {
    // ignore
  }
}

export function useTranslation() {
  const targetLang = ref<string>(loadPersistedLang())
  const loading = ref(false)
  const error = ref<string | null>(null)
  const result = ref<TranslationResult | null>(null)

  function setTargetLang(code: string): void {
    targetLang.value = code
    persistLang(code)
  }

  async function translate(text: string, lang: string): Promise<void> {
    const trimmed = text.trim()

    if (trimmed.length > TRANSLATION_CHAR_LIMIT) {
      error.value = `Selection is too long. Please select ${TRANSLATION_CHAR_LIMIT} characters or fewer.`
      result.value = null
      return
    }

    if (!trimmed) {
      error.value = 'Nothing to translate.'
      result.value = null
      return
    }

    loading.value = true
    error.value = null
    result.value = null

    try {
      const res = await translateWithGoogle(trimmed, lang)
      result.value = res
    } catch {
      try {
        const res = await translateWithAzure(trimmed, lang)
        result.value = res
      } catch {
        error.value = 'Translation failed. Check your connection and try again.'
      }
    } finally {
      loading.value = false
    }
  }

  function reset(): void {
    loading.value = false
    error.value = null
    result.value = null
  }

  return {
    supportedLanguages: SUPPORTED_LANGUAGES,
    targetLang,
    loading,
    error,
    result,
    setTargetLang,
    translate,
    reset,
  }
}
