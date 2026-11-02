import { isPastelAccent } from '@/lib/theme-accent-meta'

export interface BookCoverPalette {
  gradient: string
  from: string
  to: string
  color: string
  accent: string
  textMuted: string
}

const DEFAULT_THEME_HUE = 263
const THEME_HUE_OFFSETS = [-36, -24, -12, 0, 12, 24, 36] as const

type CoverTone = 'vivid' | 'pastel'

export interface BookCoverPaletteOptions {
  themeHue?: number
  accent?: string | null
  isDark?: boolean
}

function seedHash(seed: string): number {
  let h = 5381
  for (let i = 0; i < seed.length; i++) {
    h = ((h << 5) + h + seed.charCodeAt(i)) >>> 0
  }
  return h
}

function normalizeHue(hue: number): number {
  const value = hue % 360
  return value < 0 ? value + 360 : value
}

function resolveThemeHue(fallbackHue = DEFAULT_THEME_HUE): number {
  if (typeof document === 'undefined') return fallbackHue
  const raw = getComputedStyle(document.documentElement).getPropertyValue('--tint-h').trim()
  const hue = Number.parseFloat(raw)
  if (!Number.isFinite(hue)) return fallbackHue
  return normalizeHue(hue)
}

function resolveAccentFromDocument(): string | null {
  if (typeof document === 'undefined') return null
  for (const className of document.documentElement.classList) {
    if (className.startsWith('accent-')) return className.slice('accent-'.length)
  }
  return null
}

function resolveIsDarkFromDocument(): boolean {
  return typeof document !== 'undefined' && document.documentElement.classList.contains('dark')
}

function resolveTone(accent: string | null): CoverTone {
  return isPastelAccent(accent) ? 'pastel' : 'vivid'
}

function resolvePaletteOptions(themeHueOrOptions?: number | BookCoverPaletteOptions): {
  themeHue?: number
  accent: string | null
  isDark: boolean
} {
  if (typeof themeHueOrOptions === 'number') {
    return { themeHue: themeHueOrOptions, accent: resolveAccentFromDocument(), isDark: resolveIsDarkFromDocument() }
  }
  return {
    themeHue: themeHueOrOptions?.themeHue,
    accent: themeHueOrOptions?.accent ?? resolveAccentFromDocument(),
    isDark: themeHueOrOptions?.isDark ?? resolveIsDarkFromDocument(),
  }
}

export function bookCoverPalette(seed: string, themeHueOrOptions?: number | BookCoverPaletteOptions): BookCoverPalette {
  const options = resolvePaletteOptions(themeHueOrOptions)
  const n = seedHash(seed)
  const baseHue = normalizeHue(options.themeHue ?? resolveThemeHue())
  const hueOffset = THEME_HUE_OFFSETS[n % THEME_HUE_OFFSETS.length] ?? 0
  const hue = normalizeHue(baseHue + hueOffset)
  const tone = resolveTone(options.accent)
  const n2 = (n >>> 8) & 0xff
  const n3 = (n >>> 16) & 0xff

  const baseL = tone === 'pastel' ? (options.isDark ? 0.56 : 0.66) + (n2 % 5) * 0.03 : (options.isDark ? 0.46 : 0.45) + (n2 % 6) * 0.05

  const baseC = tone === 'pastel' ? (options.isDark ? 0.05 : 0.04) + (n3 % 4) * 0.012 : (options.isDark ? 0.11 : 0.12) + (n3 % 5) * 0.02

  const from = `oklch(${baseL.toFixed(3)} ${baseC.toFixed(3)} ${Math.round(hue)})`

  // Gradient: 20% lightness drop
  const toL = baseL - (tone === 'pastel' ? 0.16 : 0.2)
  const to = `oklch(${toL.toFixed(3)} ${baseC.toFixed(3)} ${Math.round(hue)})`

  // Text: Purest tint
  const color = `oklch(0.99 0.01 ${Math.round(hue)})`

  // Accent: Brightest highlight
  const accentL = Math.min(0.95, baseL + (tone === 'pastel' ? 0.2 : 0.35))
  const accent = `oklch(${accentL.toFixed(3)} ${Math.max(0.02, baseC - (tone === 'pastel' ? 0.02 : 0.08)).toFixed(3)} ${Math.round(hue)})`

  const textMuted = `oklch(0.9 ${tone === 'pastel' ? '0.012' : '0.020'} ${Math.round(hue)})`

  return {
    gradient: `linear-gradient(135deg, ${from} 0%, ${to} 100%)`,
    from,
    to,
    color,
    accent,
    textMuted,
  }
}

export function bookCoverStyle(seed: string): { background: string; color: string } {
  const p = bookCoverPalette(seed)
  return { background: p.gradient, color: p.color }
}

export function titleFontSizeClass(title: string): string {
  const len = title.length
  if (len <= 6) return 'text-[30cqi]'
  if (len <= 12) return 'text-[24cqi]'
  if (len <= 22) return 'text-[18cqi]'
  if (len <= 35) return 'text-[13cqi]'
  return 'text-[10cqi]'
}
