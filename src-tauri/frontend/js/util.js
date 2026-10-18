// Michael Burgus — https://github.com/NeuralDrifter
// Small DOM and formatting helpers

const { convertFileSrc } = window.__TAURI__.core

/** Escape text for use inside HTML (content and quoted attributes). */
export function esc(value) {
  return String(value ?? '')
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;')
}

export const $ = (sel, root = document) => root.querySelector(sel)
export const $$ = (sel, root = document) => [...root.querySelectorAll(sel)]

export function icon(name, cls = '') {
  return `<span class="material-symbols-outlined ${cls}">${name}</span>`
}

const nf = new Intl.NumberFormat()
export const num = n => nf.format(n ?? 0)

/** 76340 -> "76k" */
export function kWords(n) {
  if (!n) return ''
  return n >= 1000 ? `${Math.round(n / 1000)}k words` : `${n} words`
}

export function bytes(n) {
  if (!n) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.min(units.length - 1, Math.floor(Math.log(n) / Math.log(1024)))
  const v = n / 1024 ** i
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`
}

/** Unix seconds -> "14m ago" */
export function ago(unixSecs) {
  if (!unixSecs) return 'never'
  const s = Math.max(0, Date.now() / 1000 - unixSecs)
  if (s < 60) return 'just now'
  if (s < 3600) return `${Math.floor(s / 60)}m ago`
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`
  return `${Math.floor(s / 86400)}d ago`
}

export function dateTime(unixSecs) {
  return unixSecs ? new Date(unixSecs * 1000).toLocaleString() : ''
}

export function coverUrl(path, bust = '') {
  return path ? convertFileSrc(path) + (bust ? `?t=${bust}` : '') : ''
}

/** Strip HTML from a description into plain paragraphs. */
export function plainParagraphs(html) {
  if (!html) return []
  const doc = new DOMParser().parseFromString(html, 'text/html')
  const paras = [...doc.querySelectorAll('p')].map(p => p.textContent.trim()).filter(Boolean)
  return paras.length ? paras : [doc.body.textContent.trim()].filter(Boolean)
}

export function debounce(fn, ms) {
  let t
  return (...args) => {
    clearTimeout(t)
    t = setTimeout(() => fn(...args), ms)
  }
}

/** Close an open popover when clicking elsewhere or pressing Escape. */
export function dismissable(el, onClose) {
  const away = e => { if (!el.contains(e.target)) close() }
  const key = e => { if (e.key === 'Escape') close() }
  function close() {
    document.removeEventListener('pointerdown', away, true)
    document.removeEventListener('keydown', key, true)
    onClose()
  }
  setTimeout(() => {
    document.addEventListener('pointerdown', away, true)
    document.addEventListener('keydown', key, true)
  })
  return close
}
