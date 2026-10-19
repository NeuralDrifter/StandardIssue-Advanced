// Michael Burgus — https://github.com/NeuralDrifter
// Archival formats card: the epub, the Kobo KEPUB and Kindle AZW3 made from it, and Add to Calibre

import { api } from './api.js'
import { esc, icon, bytes } from './util.js'

const FORMATS = [
  ['kepub', 'KEPUB', 'Kobo', 'Kobo e-readers: sentence-level reading stats and position sync'],
  ['azw3', 'AZW3', 'Kindle', 'Kindle e-readers, converted with Calibre'],
]

/**
 * One card per place it is shown. `container()` returns the element to paint into (or null),
 * `repo()` the book it is for, `onError(text)` reports a failed action.
 */
export function createFormats({ container, repo, onError }) {
  let status = null      // formats_status for `loadedFor`
  let loadedFor = null
  const busy = new Set() // "repo:format" being made

  async function load() {
    const r = repo()
    if (!r) return
    if (r !== loadedFor) status = null
    paint()
    try {
      const s = await api.formatsStatus(r)
      if (repo() !== r) return
      status = s
      loadedFor = r
    } catch (err) {
      console.error('formats status', err)
    }
    paint()
  }

  function paint() {
    const el = container()
    if (el) el.innerHTML = html()
  }

  function html() {
    const r = repo()
    if (!status || loadedFor !== r) return `<div class="text-label-sm text-outline">Checking formats…</div>`
    const fileRow = (name, sub, info, actions) => `
      <div class="flex items-center gap-2 py-1.5 border-b border-outline-variant/30 last:border-0">
        <div class="flex-1 min-w-0">
          <div class="text-label-md text-on-surface">${name} <span class="text-outline font-normal">· ${sub}</span></div>
          <div class="text-[10px] ${info?.stale ? 'text-primary' : 'text-outline'}">${info ? `${bytes(info.size)}${info.stale ? ' · older than the epub' : ''}` : 'Not made yet'}</div>
        </div>
        <div class="flex items-center gap-1 shrink-0">${actions}</div>
      </div>`
    const btn = (attr, iconName, label, title, disabled = false) => `
      <button ${attr} title="${esc(title)}" ${disabled ? 'disabled' : ''} class="flex items-center gap-1 h-7 px-2 rounded-lg bg-surface-container hover:bg-surface-container-high text-label-sm text-on-surface-variant hover:text-on-surface disabled:opacity-50 disabled:pointer-events-none">${icon(iconName, 'text-[15px]')}${label}</button>`

    const rows = [fileRow('EPUB 3', 'Standard Ebooks', status.epub,
      btn('data-fmt-open', 'open_in_new', '', 'Open in your default e-reader')
      + btn('data-fmt-reveal', 'folder_open', '', 'Show the files in Explorer'))]
    for (const [key, name, device, help] of FORMATS) {
      const info = status[key]
      const making = busy.has(`${r}:${key}`)
      const needsCalibre = key === 'azw3' && !status.calibre
      const label = making ? 'Making…' : info ? 'Remake' : 'Make'
      rows.push(fileRow(name, device, info, needsCalibre
        ? `<span class="text-[10px] text-outline text-right leading-tight" title="Install Calibre (calibre-ebook.com) to convert to AZW3">Needs Calibre</span>`
        : btn(`data-fmt-make="${key}"`, making ? 'hourglass_top' : info ? 'cached' : 'add', label, help, making)))
    }
    return `
      <div class="flex flex-col">${rows.join('')}</div>
      ${status.calibre ? `<button data-fmt-calibre class="btn-glass self-start" title="Open the epub in Calibre, which adds it to your Calibre library">${icon('library_add', 'text-[16px]')}Add to Calibre</button>` : ''}`
  }

  /** Returns true when the click was on the card. */
  function handleClick(e) {
    const r = repo()
    if (!r || !e.target.closest) return false
    if (e.target.closest('[data-fmt-open]')) {
      api.openEpub(r).catch(err => onError(`Could not open: ${err}`))
      return true
    }
    if (e.target.closest('[data-fmt-reveal]')) {
      api.revealEpub(r).catch(err => onError(`Could not show the file: ${err}`))
      return true
    }
    if (e.target.closest('[data-fmt-calibre]')) {
      api.addToCalibre(r).catch(err => onError(`Could not add to Calibre: ${err}`))
      return true
    }
    const key = e.target.closest('[data-fmt-make]')?.dataset.fmtMake
    if (key) {
      make(r, key)
      return true
    }
    return false
  }

  async function make(r, key) {
    const id = `${r}:${key}`
    if (busy.has(id)) return
    busy.add(id)
    paint()
    try {
      const s = await api.makeFormat(r, key)
      if (repo() === r) {
        status = s
        loadedFor = r
      }
    } catch (err) {
      onError(`Could not make the ${key.toUpperCase()}: ${err}`)
    } finally {
      busy.delete(id)
      paint()
    }
  }

  return { load, paint, handleClick }
}
