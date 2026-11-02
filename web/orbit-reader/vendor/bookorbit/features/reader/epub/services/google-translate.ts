import type { TranslationResult } from './translation.types'

const GOOGLE_TRANSLATE_URL = 'https://translate.googleapis.com/translate_a/single'

export class GoogleTranslateError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'GoogleTranslateError'
  }
}

export async function translateWithGoogle(text: string, targetLang: string): Promise<TranslationResult> {
  const url = new URL(GOOGLE_TRANSLATE_URL)
  url.searchParams.set('client', 'gtx')
  url.searchParams.set('dt', 't')
  url.searchParams.set('sl', 'auto')
  url.searchParams.set('tl', targetLang)
  url.searchParams.set('q', text)

  let response: Response
  try {
    response = await fetch(url.toString())
  } catch (err) {
    throw new GoogleTranslateError(`Network error: ${err instanceof Error ? err.message : String(err)}`)
  }

  if (!response.ok) {
    throw new GoogleTranslateError(`HTTP ${response.status}`)
  }

  let data: unknown
  try {
    data = await response.json()
  } catch {
    throw new GoogleTranslateError('Failed to parse response')
  }

  if (!Array.isArray(data) || data.length === 0) {
    throw new GoogleTranslateError('Unexpected response shape')
  }

  const sentences = data[0]
  if (!Array.isArray(sentences)) {
    throw new GoogleTranslateError('Unexpected sentences shape')
  }

  let translatedText = ''
  for (const sentence of sentences) {
    if (Array.isArray(sentence) && typeof sentence[0] === 'string') {
      translatedText += sentence[0]
    }
  }

  if (!translatedText) {
    throw new GoogleTranslateError('Empty translation in response')
  }

  const detectedSourceLang = typeof data[2] === 'string' ? data[2] : 'auto'

  return { translatedText, detectedSourceLang, provider: 'Google' }
}
