// Michael Burgus — https://github.com/NeuralDrifter
// Typesetting: reading atmospheres, fonts, size, measure; the CSS injected into book pages

import { api } from '../api.js'
import { R, rq } from './ctx.js'
import { esc, icon } from '../util.js'

export const THEMES = {
  obsidian: { label: 'Obsidian', note: 'Atelier Midnight', bg: '#181c22', fg: '#dfe2eb', dim: '#99907c', link: '#f2ca50', accent: '#f2ca50', dark: true },
  parchment: { label: 'Parchment', note: 'Warm Sepia', bg: '#f4ecd8', fg: '#3b2f22', dim: '#8a7558', link: '#7a4b16', accent: '#9a6a12', dark: false },
  cream: { label: 'Cream', note: 'Editorial Day', bg: '#fbf8f1', fg: '#1f1b16', dim: '#857b6a', link: '#6b5000', accent: '#8a6a00', dark: false },
  oled: { label: 'Pure OLED', note: 'Zero Light', bg: '#000000', fg: '#c9ccd3', dim: '#77705f', link: '#e9c349', accent: '#e9c349', dark: true },
}

const FONTS = {
  publisher: { label: 'Original', stack: null, sample: 'Georgia, serif' },
  serif: { label: 'Serif', stack: '"Playfair Display", Georgia, serif' },
  sans: { label: 'Sans', stack: 'Manrope, "Segoe UI", system-ui, sans-serif' },
  mono: { label: 'Mono', stack: '"Cascadia Mono", Consolas, "Courier New", monospace' },
}

const MEASURES = { compact: 520, optimal: 660, wide: 860 }
const LINE_HEIGHTS = { compact: 1.35, normal: 1.6, relaxed: 1.85 }
const SIZE_MIN = 12
const SIZE_MAX = 32

const fontUrl = file => new URL(`vendor/fonts/${file}`, document.baseURI).href

// Book pages are separate documents, so the bundled faces are declared again inside them
const FONT_FACES = `
  @font-face { font-family: "Playfair Display"; font-weight: 400 900; font-style: normal;
    src: url("${fontUrl('playfair-display-latin-wght-normal.woff2')}") format("woff2-variations"); }
  @font-face { font-family: "Playfair Display"; font-weight: 400 900; font-style: italic;
    src: url("${fontUrl('playfair-display-latin-wght-italic.woff2')}") format("woff2-variations"); }
  @font-face { font-family: "Manrope"; font-weight: 200 800;
    src: url("${fontUrl('manrope-latin-wght-normal.woff2')}") format("woff2-variations"); }
`

function bookCSS(s) {
  const t = THEMES[s.theme] ?? THEMES.obsidian
  const font = FONTS[s.font_family]?.stack
  return `
    @namespace epub "http://www.idpf.org/2007/ops";
    ${FONT_FACES}
    html {
      color-scheme: ${t.dark ? 'dark' : 'light'};
      font-size: ${s.font_size}px !important;
    }
    html, body { background: ${t.bg} !important; color: ${t.fg} !important; }
    a:link, a:visited { color: ${t.link} !important; }
    p, li, blockquote, dd, td { line-height: ${LINE_HEIGHTS[s.line_height] ?? 1.6} !important; }
    ${font ? `body, p, li, blockquote, dd, td, div, span, h1, h2, h3, h4, h5, h6 { font-family: ${font} !important; }` : ''}
    ${s.justify ? 'p { text-align: justify; hyphens: auto; }' : ''}
    :is(h1, h2, h3)[epub|type~="ordinal"], :is(h2, h3) [epub|type~="z3998:roman"] { color: ${t.accent}; }
    ${s.drop_cap ? `
    body[epub|type~="bodymatter"] :is(h2, h3, h4, hgroup, header) + p::first-letter {
      float: left;
      font-family: "Playfair Display", Georgia, serif;
      font-size: 3.4em;
      font-weight: 600;
      line-height: .82;
      margin: .06em .09em 0 0;
      color: ${t.accent};
    }` : ''}
    ${t.dark ? 'img[epub|type~="se:image.color-depth.black-on-transparent"] { filter: invert(1); }' : ''}
  `
}

/** Colour the page card (outside the book's own documents) to match the atmosphere. */
export function applyThemeVars() {
  const t = THEMES[R.settings.theme] ?? THEMES.obsidian
  R.el.dataset.theme = R.settings.theme
  R.el.style.setProperty('--page-bg', t.bg)
  R.el.style.setProperty('--page-fg', t.fg)
  R.el.style.setProperty('--page-dim', t.dim)
}

/** Push the current settings into the open book. */
export function applyBookSettings() {
  const r = R.view?.renderer
  if (!r) return
  const s = R.settings
  r.setAttribute('flow', s.layout === 'scroll' ? 'scrolled' : 'paginated')
  r.setAttribute('max-column-count', s.layout === 'spread' ? '2' : '1')
  r.setAttribute('max-inline-size', `${MEASURES[s.measure] ?? MEASURES.optimal}px`)
  r.setAttribute('margin', '56px')
  r.setAttribute('gap', '6%')
  r.setStyles?.(bookCSS(s))
}

// --- Settings changes ---

let saveTimer = null
let onChange = () => {}

export function initTypeset(changed) {
  onChange = changed
}

/** `quiet` skips re-rendering the panel (used while a slider is being dragged). */
export function updateSetting(key, value, { quiet = false } = {}) {
  R.settings = { ...R.settings, [key]: value }
  applyThemeVars()
  applyBookSettings()
  if (!quiet) onChange(key)
  clearTimeout(saveTimer)
  saveTimer = setTimeout(saveSettings, 300)
}

/** Persist the current settings. The reply is not applied: a newer change may already be on screen. */
export function saveSettings() {
  api.readerSaveSettings(R.settings).catch(err => console.error('could not save reader settings', err))
}

export function changeFontSize(delta) {
  const size = Math.min(SIZE_MAX, Math.max(SIZE_MIN, R.settings.font_size + delta))
  if (size !== R.settings.font_size) updateSetting('font_size', size)
}

export async function resetTypesetting() {
  const keep = { toc_open: R.settings.toc_open, typeset_open: R.settings.typeset_open }
  const defaults = await api.readerSaveSettings({ ...keep })
  R.settings = defaults
  applyThemeVars()
  applyBookSettings()
  onChange('reset')
}

// --- Panel ---

/** Approximate characters per line for the current font, size and measure. */
function charsPerLine() {
  const s = R.settings
  const ctx = (charsPerLine.canvas ??= document.createElement('canvas')).getContext('2d')
  const family = FONTS[s.font_family]?.stack ?? FONTS.publisher.sample
  ctx.font = `${s.font_size}px ${family}`
  const sample = 'the quick brown fox jumps over the lazy dog, said Professor Challenger. '
  const avg = ctx.measureText(sample).width / sample.length
  const view = rq('.r-page')?.clientWidth ?? Infinity
  const column = Math.min(MEASURES[s.measure] ?? MEASURES.optimal, view * 0.88)
  return avg > 0 ? Math.round(column / avg) : 0
}

const COLS = { 2: 'grid-cols-2', 3: 'grid-cols-3', 4: 'grid-cols-4' }

const seg = (key, options, current) => `
  <div class="grid ${COLS[options.length]} gap-1 p-1 rounded-lg bg-surface-container">
    ${options.map(([value, label, extra = '']) => `
      <button data-set="${key}" data-value="${value}" class="py-1.5 rounded-lg text-label-md transition-colors ${extra}
        ${value === current ? 'bg-surface-container-highest text-primary font-semibold' : 'text-on-surface-variant hover:text-on-surface'}">${label}</button>`).join('')}
  </div>`

const sectionTitle = (text, right = '') =>
  `<div class="flex items-center justify-between"><span class="eyebrow">${text}</span>${right}</div>`

export function typesetHtml() {
  const s = R.settings
  const sizePct = ((s.font_size - SIZE_MIN) / (SIZE_MAX - SIZE_MIN)) * 100
  const chars = charsPerLine()
  return `
    <div class="flex items-center justify-between">
      <h3 class="flex items-center gap-1.5 font-display text-headline-sm text-on-surface">${icon('tune', 'text-[18px] text-primary')}Atelier Typesetting</h3>
      <button data-typeset="reset" class="text-label-sm text-outline hover:text-primary">Reset</button>
    </div>

    ${sectionTitle('Reading Atmosphere')}
    <div class="grid grid-cols-2 gap-1.5">
      ${Object.entries(THEMES).map(([key, t]) => `
        <button data-set="theme" data-value="${key}" class="flex items-center gap-2 p-2 rounded-lg text-left transition-colors
          ${key === s.theme ? 'bg-surface-container-highest ring-1 ring-primary/50' : 'bg-surface-container hover:bg-surface-container-high'}">
          <span class="w-3.5 h-3.5 rounded-full shrink-0 ring-1 ring-outline/40" style="background:${t.bg}"></span>
          <span class="min-w-0"><span class="block text-label-md text-on-surface">${t.label}</span>
          <span class="block text-[10px] text-outline truncate">${t.note}</span></span>
        </button>`).join('')}
    </div>

    ${sectionTitle('Typography Family')}
    ${seg('font_family', [
      ['publisher', 'Original'],
      ['serif', 'Serif', 'font-display'],
      ['sans', 'Sans'],
      ['mono', 'Mono', 'font-mono'],
    ], s.font_family)}

    ${sectionTitle('Font Scale', `<span data-size-label class="text-label-md text-primary font-semibold">${s.font_size} px</span>`)}
    <div class="flex items-center gap-2">
      <button data-typeset="smaller" class="text-label-md text-on-surface-variant hover:text-primary w-6" title="Smaller (-)">A−</button>
      <input type="range" class="r-range flex-1" data-range="font_size" min="${SIZE_MIN}" max="${SIZE_MAX}" step="0.5" value="${s.font_size}" style="--fill:${sizePct}%">
      <button data-typeset="larger" class="text-label-md text-on-surface-variant hover:text-primary w-6" title="Larger (+)">A+</button>
    </div>

    ${sectionTitle('Column Measure', chars ? `<span class="text-label-md text-on-surface-variant">≈ ${chars} chars</span>` : '')}
    ${seg('measure', [['compact', 'Compact'], ['optimal', 'Optimal'], ['wide', 'Wide']], s.measure)}

    ${sectionTitle('Line Spacing')}
    ${seg('line_height', [['compact', 'Tight'], ['normal', 'Normal'], ['relaxed', 'Airy']], s.line_height)}

    ${sectionTitle('Page Layout')}
    ${seg('layout', [['paginated', 'Single'], ['spread', 'Two-page'], ['scroll', 'Scroll']], s.layout)}

    <div class="flex flex-col gap-2 pt-1">
      ${switchRow('justify', 'Justified alignment', s.justify)}
      ${switchRow('drop_cap', 'Drop cap at chapter start', s.drop_cap)}
    </div>`
}

const switchRow = (key, label, on) => `
  <label class="flex items-center justify-between cursor-pointer">
    <span class="eyebrow">${esc(label)}</span>
    <button data-set="${key}" data-value="${!on}" role="switch" aria-checked="${on}" class="toggle"></button>
  </label>`

/** Click inside the typesetting panel; returns true when handled. */
export function handleTypesetClick(e) {
  const set = e.target.closest('[data-set]')
  if (set) {
    let value = set.dataset.value
    if (value === 'true' || value === 'false') value = value === 'true'
    updateSetting(set.dataset.set, value)
    return true
  }
  const act = e.target.closest('[data-typeset]')?.dataset.typeset
  if (act === 'smaller') changeFontSize(-1)
  else if (act === 'larger') changeFontSize(1)
  else if (act === 'reset') resetTypesetting()
  return !!act
}

export function handleTypesetInput(e) {
  const range = e.target.closest('[data-range="font_size"]')
  if (!range) return false
  const px = parseFloat(range.value)
  range.style.setProperty('--fill', `${((px - SIZE_MIN) / (SIZE_MAX - SIZE_MIN)) * 100}%`)
  rq('[data-size-label]').textContent = `${px} px`
  updateSetting('font_size', px, { quiet: true })
  return true
}
