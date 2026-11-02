/**
 * English-only i18n host for the Book Orbit reader inside Shelf.
 * Catalog and ICU helper come from Book Orbit (AGPL-3.0-only):
 * https://github.com/bookorbit/bookorbit
 */
import { createI18n } from 'vue-i18n'
import { DEFAULT_LOCALE } from '@bookorbit/types'
import en from '@/locales/en.json'
import { compileIcuCatalog } from './icu'

export type MessageSchema = typeof en

export const i18n = createI18n({
  legacy: false,
  locale: DEFAULT_LOCALE,
  fallbackLocale: DEFAULT_LOCALE,
  missingWarn: false,
  fallbackWarn: false,
  messages: { en: compileIcuCatalog(en, DEFAULT_LOCALE) },
})

export async function setI18nLocale(): Promise<void> {
  // Shelf ships English for the vendored reader in this phase.
}
