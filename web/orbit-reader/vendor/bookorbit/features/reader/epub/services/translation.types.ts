export const TRANSLATION_CHAR_LIMIT = 500

export interface TranslationResult {
  translatedText: string
  detectedSourceLang: string
  provider: 'Google' | 'Azure'
}

export interface SupportedLanguage {
  code: string
  name: string
}

export const SUPPORTED_LANGUAGES: SupportedLanguage[] = [
  { code: 'en', name: 'English' },
  { code: 'es', name: 'Spanish' },
  { code: 'fr', name: 'French' },
  { code: 'de', name: 'German' },
  { code: 'zh', name: 'Chinese' },
  { code: 'ja', name: 'Japanese' },
  { code: 'ko', name: 'Korean' },
  { code: 'ar', name: 'Arabic' },
  { code: 'hi', name: 'Hindi' },
  { code: 'pt', name: 'Portuguese' },
]
