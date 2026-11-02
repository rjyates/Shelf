import { describe, it, expect } from 'vitest'
import { bookCoverStyle, bookCoverPalette, titleFontSizeClass } from './book-cover'

function extractHue(color: string): number {
  const match = color.match(/oklch\([^)]+\s(-?\d+(?:\.\d+)?)\)/)
  if (!match) throw new Error(`Could not extract hue from color: ${color}`)
  return Number.parseFloat(match[1]!)
}

function extractChroma(color: string): number {
  const match = color.match(/oklch\(\s*-?\d+(?:\.\d+)?\s+(-?\d+(?:\.\d+)?)\s+-?\d+(?:\.\d+)?\s*\)/)
  if (!match) throw new Error(`Could not extract chroma from color: ${color}`)
  return Number.parseFloat(match[1]!)
}

function hueDistance(a: number, b: number): number {
  const delta = Math.abs(a - b) % 360
  return delta > 180 ? 360 - delta : delta
}

describe('bookCoverStyle', () => {
  it('returns background and color properties', () => {
    const style = bookCoverStyle('Dune')
    expect(style).toHaveProperty('background')
    expect(style).toHaveProperty('color')
  })

  it('produces a CSS gradient for background', () => {
    const style = bookCoverStyle('Test')
    expect(style.background).toMatch(/^linear-gradient\(/)
  })

  it('produces oklch text color', () => {
    const style = bookCoverStyle('Test')
    expect(style.color).toMatch(/^oklch\(/)
  })

  it('produces consistent results for the same seed', () => {
    const a = bookCoverStyle('Foundation')
    const b = bookCoverStyle('Foundation')
    expect(a).toEqual(b)
  })

  it('produces different results for different seeds', () => {
    const results = new Set<string>()
    for (let i = 0; i < 50; i++) {
      results.add(bookCoverStyle(`seed-${i}`).background)
    }
    expect(results.size).toBeGreaterThan(1)
  })

  it('handles empty string seed without throwing', () => {
    expect(() => bookCoverStyle('')).not.toThrow()
  })

  it('handles numeric fallback seed (e.g. book id as string)', () => {
    const style = bookCoverStyle('42')
    expect(style.background).toMatch(/^linear-gradient\(/)
  })

  it('background matches the gradient from bookCoverPalette for the same seed', () => {
    const style = bookCoverStyle('Dune')
    const palette = bookCoverPalette('Dune')
    expect(style.background).toBe(palette.gradient)
    expect(style.color).toBe(palette.color)
  })
})

describe('bookCoverPalette', () => {
  it('returns all required palette fields', () => {
    const palette = bookCoverPalette('Dune')
    expect(palette).toHaveProperty('gradient')
    expect(palette).toHaveProperty('from')
    expect(palette).toHaveProperty('to')
    expect(palette).toHaveProperty('color')
    expect(palette).toHaveProperty('accent')
    expect(palette).toHaveProperty('textMuted')
  })

  it('produces consistent results for the same seed', () => {
    const a = bookCoverPalette('Foundation')
    const b = bookCoverPalette('Foundation')
    expect(a).toEqual(b)
  })

  it('all core oklch color fields are oklch strings', () => {
    const palette = bookCoverPalette('Test')
    expect(palette.color).toMatch(/^oklch\(/)
    expect(palette.accent).toMatch(/^oklch\(/)
    expect(palette.textMuted).toMatch(/^oklch\(/)
    expect(palette.from).toMatch(/^oklch\(/)
    expect(palette.to).toMatch(/^oklch\(/)
  })

  it('produces different colors for different seeds', () => {
    const hues = new Set<string>()
    for (let i = 0; i < 50; i++) {
      hues.add(bookCoverPalette(`seed-${i}`).accent)
    }
    expect(hues.size).toBeGreaterThanOrEqual(10)
  })

  it('anchors placeholder hue near the supplied theme hue', () => {
    const themeHue = 142
    for (let i = 0; i < 50; i++) {
      const palette = bookCoverPalette(`seed-${i}`, { themeHue })
      const hue = extractHue(palette.from)
      expect([0, 12, 24, 36]).toContain(hueDistance(hue, themeHue))
    }
  })

  it('uses --tint-h from CSS variables when no theme hue is provided', () => {
    const root = document.documentElement
    const prev = root.style.getPropertyValue('--tint-h')
    root.style.setProperty('--tint-h', '118')
    try {
      const palette = bookCoverPalette('seed-css-hue')
      const hue = extractHue(palette.from)
      expect([0, 12, 24, 36]).toContain(hueDistance(hue, 118))
    } finally {
      if (prev) root.style.setProperty('--tint-h', prev)
      else root.style.removeProperty('--tint-h')
    }
  })

  it('falls back to default hue if --tint-h is not a number', () => {
    const root = document.documentElement
    const prev = root.style.getPropertyValue('--tint-h')
    root.style.setProperty('--tint-h', 'invalid')
    try {
      expect(bookCoverPalette('seed-fallback')).toEqual(bookCoverPalette('seed-fallback', { themeHue: 263 }))
    } finally {
      if (prev) root.style.setProperty('--tint-h', prev)
      else root.style.removeProperty('--tint-h')
    }
  })

  it('uses lower chroma for pastel accents than vivid accents', () => {
    const vivid = bookCoverPalette('seed-tone-compare', { themeHue: 142, accent: 'blue', isDark: false })
    const pastel = bookCoverPalette('seed-tone-compare', { themeHue: 142, accent: 'mint', isDark: false })
    expect(extractChroma(pastel.from)).toBeLessThan(extractChroma(vivid.from))
  })

  it('always produces vibrant backgrounds', () => {
    for (let i = 0; i < 50; i++) {
      const p = bookCoverPalette(`seed-${i}`)
      expect(p.from).toMatch(/^oklch\(0\.[4-7]/)
    }
  })

  it('handles empty string seed without throwing', () => {
    expect(() => bookCoverPalette('')).not.toThrow()
  })
})

describe('titleFontSizeClass', () => {
  it('returns largest class for very short titles (up to 6 chars)', () => {
    expect(titleFontSizeClass('Dune')).toBe('text-[30cqi]')
    expect(titleFontSizeClass('123456')).toBe('text-[30cqi]')
  })

  it('returns large class for titles 7-12 chars', () => {
    expect(titleFontSizeClass('1234567')).toBe('text-[24cqi]')
    expect(titleFontSizeClass('Foundation')).toBe('text-[24cqi]')
  })

  it('returns medium class for titles 13-22 chars', () => {
    expect(titleFontSizeClass('1234567890123')).toBe('text-[18cqi]')
    expect(titleFontSizeClass('The Lord of the Rings')).toBe('text-[18cqi]')
  })

  it('returns small class for titles 23-35 chars', () => {
    expect(titleFontSizeClass('A Game of Thrones Series!')).toBe('text-[13cqi]')
  })

  it('returns smallest class for titles over 35 chars', () => {
    expect(titleFontSizeClass("The Hitchhiker's Guide to the Galaxy")).toBe('text-[10cqi]')
  })

  it('handles empty string (treated as very short)', () => {
    expect(titleFontSizeClass('')).toBe('text-[30cqi]')
  })
})
