import {
  COVER_SEARCH_DEFAULT_PROVIDERS,
  DEFAULT_COVER_SEARCH_PROVIDER,
  type CoverSearchDefaultProvider,
  type CoverSearchPreferences,
} from '@bookorbit/types'
import { ref } from 'vue'
import { api } from '@/lib/api'

const ENDPOINT = '/api/v1/user-preferences/cover-search'

const defaultProvider = ref<CoverSearchDefaultProvider>(DEFAULT_COVER_SEARCH_PROVIDER)
const isLoading = ref(false)
const isSaving = ref(false)

let hasLoaded = false
let inFlightLoad: Promise<boolean> | null = null

function normalizeProvider(value: unknown): CoverSearchDefaultProvider {
  return typeof value === 'string' && COVER_SEARCH_DEFAULT_PROVIDERS.includes(value as CoverSearchDefaultProvider)
    ? (value as CoverSearchDefaultProvider)
    : DEFAULT_COVER_SEARCH_PROVIDER
}

async function fetchPreferences(): Promise<boolean> {
  isLoading.value = true
  try {
    const response = await api(ENDPOINT)
    if (!response.ok) return false

    const body = (await response.json()) as { settings?: Partial<CoverSearchPreferences> }
    defaultProvider.value = normalizeProvider(body.settings?.defaultProvider)
    hasLoaded = true
    return true
  } catch {
    return false
  } finally {
    isLoading.value = false
  }
}

export function useCoverSearchPreferences() {
  async function load(): Promise<boolean> {
    if (hasLoaded) return true
    inFlightLoad ??= fetchPreferences().finally(() => {
      inFlightLoad = null
    })
    return inFlightLoad
  }

  async function update(provider: CoverSearchDefaultProvider): Promise<boolean> {
    const previous = defaultProvider.value
    defaultProvider.value = provider
    isSaving.value = true

    try {
      const response = await api(ENDPOINT, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ settings: { defaultProvider: provider } } satisfies { settings: CoverSearchPreferences }),
      })
      if (!response.ok) {
        defaultProvider.value = previous
        return false
      }
      hasLoaded = true
      return true
    } catch {
      defaultProvider.value = previous
      return false
    } finally {
      isSaving.value = false
    }
  }

  return { defaultProvider, isLoading, isSaving, load, update }
}
