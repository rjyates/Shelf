import type { TranslationResult } from './translation.types'

const AZURE_AUTH_URL = 'https://edge.microsoft.com/translate/auth'
const AZURE_TRANSLATE_URL = 'https://api-edge.cognitive.microsofttranslator.com/translate'
const TOKEN_TTL_MS = 7 * 60 * 1000

export class AzureTranslateError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'AzureTranslateError'
  }
}

let cachedToken: string | null = null
let tokenExpiresAt: number = 0

export function resetTokenCache(): void {
  cachedToken = null
  tokenExpiresAt = 0
}

async function getAuthToken(): Promise<string> {
  if (cachedToken && Date.now() < tokenExpiresAt) {
    return cachedToken
  }

  let response: Response
  try {
    response = await fetch(AZURE_AUTH_URL)
  } catch (err) {
    throw new AzureTranslateError(`Auth network error: ${err instanceof Error ? err.message : String(err)}`)
  }

  if (!response.ok) {
    throw new AzureTranslateError(`Auth HTTP ${response.status}`)
  }

  const token = await response.text()
  if (!token || !token.trim()) {
    throw new AzureTranslateError('Empty auth token')
  }

  cachedToken = token.trim()
  tokenExpiresAt = Date.now() + TOKEN_TTL_MS
  return cachedToken
}

export async function translateWithAzure(text: string, targetLang: string): Promise<TranslationResult> {
  const token = await getAuthToken()

  const url = new URL(AZURE_TRANSLATE_URL)
  url.searchParams.set('to', targetLang)
  url.searchParams.set('api-version', '3.0')

  let response: Response
  try {
    response = await fetch(url.toString(), {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify([{ Text: text }]),
    })
  } catch (err) {
    throw new AzureTranslateError(`Network error: ${err instanceof Error ? err.message : String(err)}`)
  }

  if (response.status === 401) {
    resetTokenCache()
    throw new AzureTranslateError('Auth token rejected (401)')
  }

  if (!response.ok) {
    throw new AzureTranslateError(`HTTP ${response.status}`)
  }

  let data: unknown
  try {
    data = await response.json()
  } catch {
    throw new AzureTranslateError('Failed to parse response')
  }

  if (!Array.isArray(data) || data.length === 0) {
    throw new AzureTranslateError('Unexpected response shape')
  }

  const first = data[0] as Record<string, unknown>
  const translations = first['translations']
  if (!Array.isArray(translations) || translations.length === 0) {
    throw new AzureTranslateError('No translations in response')
  }

  const translation = translations[0] as Record<string, unknown>
  const translatedText = translation['text']
  if (typeof translatedText !== 'string' || !translatedText) {
    throw new AzureTranslateError('Empty translated text in response')
  }

  let detectedSourceLang = 'auto'
  const detectedLanguage = first['detectedLanguage']
  if (detectedLanguage && typeof detectedLanguage === 'object') {
    const lang = (detectedLanguage as Record<string, unknown>)['language']
    if (typeof lang === 'string') {
      detectedSourceLang = lang
    }
  }

  return { translatedText, detectedSourceLang, provider: 'Azure' }
}
