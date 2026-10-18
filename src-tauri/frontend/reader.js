// Michael Burgus — https://github.com/NeuralDrifter
// In-app EPUB reader overlay built on foliate-js

import { makeBook } from './vendor/foliate-js/view.js'
import { attachSanitizer } from './sanitize.js'

const { invoke, convertFileSrc } = window.__TAURI__.core

const WORDS_PER_MINUTE = 250
const SAVE_DELAY_MS = 1000
const CHROME_HIDE_MS = 2500

const THEMES = {
  light: { bg: '#ffffff', fg: '#1b1b1b', link: '#1a5fb4' },
  sepia: { bg: '#f4ecd8', fg: '#3b2f22', link: '#7a4b16' },
  dark: { bg: '#1c1c1e', fg: '#dcdcdc', link: '#8ab4f8' },
}
const LINE_HEIGHTS = { compact: 1.3, normal: 1.5, relaxed: 1.75 }
const FONTS = {
  serif: 'Georgia, "Iowan Old Style", "Palatino Linotype", serif',
  sans: '"Segoe UI", system-ui, -apple-system, sans-serif',
}

let root = null        // overlay element
let view = null        // <foliate-view>
let current = null     // { repo, title, author, wordCount }
let position = null    // last foliate relocate detail
let settings = null
let saveTimer = null
let chromeTimer = null
let backCfi = null
let wheelLock = 0

const $ = sel => root.querySelector(sel)

// --- CSS injected into every book page ---

function bookCSS(s) {
  const t = THEMES[s.theme] ?? THEMES.light
  const font = FONTS[s.font_family]
  return `
    @namespace epub "http://www.idpf.org/2007/ops";
    html {
      color-scheme: ${s.theme === 'dark' ? 'dark' : 'light'};
      font-size: ${s.font_size}% !important;
    }
    html, body { background: ${t.bg} !important; color: ${t.fg} !important; }
    a:link, a:visited { color: ${t.link} !important; }
    p, li, blockquote, dd, td { line-height: ${LINE_HEIGHTS[s.line_height] ?? 1.5} !important; }
    ${font ? `body, p, li, blockquote, dd, td, div, span { font-family: ${font} !important; }` : ''}
    ${s.theme === 'dark'
      ? 'img[epub|type~="se:image.color-depth.black-on-transparent"] { filter: invert(1); }'
      : ''}
  `
}

// --- Overlay DOM ---

function build() {
  root = document.createElement('div')
  root.className = 'reader'
  root.hidden = true
  root.innerHTML = `
    <header class="r-toolbar">
      <button class="r-btn" data-act="toc" title="Contents (T)">&#9776;</button>
      <div class="r-title"><span class="t"></span><span class="a"></span></div>
      <button class="r-btn" data-act="settings" title="Reading settings">Aa</button>
      <button class="r-btn" data-act="close" title="Close (Esc)">&#10005;</button>
    </header>
    <div class="r-body">
      <aside class="r-side" hidden><nav class="r-toc"></nav></aside>
      <main class="r-main">
        <div class="r-view"></div>
        <button class="r-back" hidden>&larr; Back</button>
        <div class="r-message" hidden></div>
      </main>
    </div>
    <footer class="r-footer">
      <span class="r-chapter"></span>
      <input type="range" class="r-slider" min="0" max="1" step="0.0001" value="0">
      <span class="r-percent"></span>
      <span class="r-left"></span>
    </footer>
    <div class="r-popover" hidden>
      <div class="row"><div class="label">Layout</div><div class="r-seg" data-setting="layout">
        <button data-value="paginated">Paginated</button><button data-value="spread">Two-page</button><button data-value="scroll">Scroll</button>
      </div></div>
      <div class="row"><div class="label">Theme</div><div class="r-seg" data-setting="theme">
        <button data-value="light">Light</button><button data-value="sepia">Sepia</button><button data-value="dark">Dark</button>
      </div></div>
      <div class="row"><div class="label">Font</div><div class="r-seg" data-setting="font_family">
        <button data-value="publisher">Publisher</button><button data-value="serif">Serif</button><button data-value="sans">Sans</button>
      </div></div>
      <div class="row"><div class="label">Size</div><div class="r-size">
        <button class="r-btn" data-act="smaller" title="Smaller (-)">A&minus;</button><span class="r-size-val"></span><button class="r-btn" data-act="larger" title="Larger (+)">A+</button>
      </div></div>
      <div class="row"><div class="label">Line spacing</div><div class="r-seg" data-setting="line_height">
        <button data-value="compact">Compact</button><button data-value="normal">Normal</button><button data-value="relaxed">Relaxed</button>
      </div></div>
    </div>
  `
  document.body.append(root)

  root.addEventListener('click', e => {
    const act = e.target.closest('[data-act]')?.dataset.act
    if (act === 'toc') toggleSide()
    else if (act === 'settings') togglePopover()
    else if (act === 'close') close()
    else if (act === 'smaller') changeFontSize(-10)
    else if (act === 'larger') changeFontSize(10)

    const seg = e.target.closest('.r-seg button')
    if (seg) updateSetting(seg.parentElement.dataset.setting, seg.dataset.value)

    if (!e.target.closest('.r-popover, [data-act="settings"]')) $('.r-popover').hidden = true
  })

  $('.r-slider').addEventListener('input', e => view?.goToFraction(parseFloat(e.target.value)))
  $('.r-back').addEventListener('click', goBack)
  root.addEventListener('mousemove', e => onPointer(e.clientY))
  window.addEventListener('blur', flushSave)
}

// --- Open / close ---

async function open(repo) {
  if (!root) build()
  if (current) await close()

  root.hidden = false
  document.body.classList.add('reader-open')
  showMessage('Opening…')
  settings ??= await invoke('reader_get_settings')
  applyChromeSettings()

  let info
  try {
    info = await invoke('reader_open', { repo })
  } catch (err) {
    if (String(err) === 'not-found') showMissing(repo)
    else showMessage(`Could not open this book: ${err}`)
    return
  }
  current = { repo, title: info.title, author: info.author, wordCount: info.word_count }
  $('.r-title .t').textContent = info.title || repo
  $('.r-title .a').textContent = info.author ? `— ${info.author}` : ''

  try {
    const res = await fetch(convertFileSrc(info.path))
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
    const file = new File([await res.blob()], `${repo}.epub`, { type: 'application/epub+zip' })

    // Sanitize before any page of the book is rendered
    const book = await makeBook(file)
    attachSanitizer(book)

    view = document.createElement('foliate-view')
    $('.r-view').append(view)
    view.addEventListener('relocate', onRelocate)
    view.addEventListener('load', onSectionLoad)
    view.addEventListener('external-link', onExternalLink)
    view.addEventListener('link', onInternalLink)
    await view.open(book)
    applyBookSettings()
    renderTOC(book.toc ?? [])
    await goToSaved(info.progress)
    hideMessage()
    scheduleChromeHide()
  } catch (err) {
    console.error(err)
    showOpenFailed(repo, err)
  }
}

/** Resume at the saved CFI; if it no longer resolves, use the saved fraction; else the text start. */
async function goToSaved(progress) {
  if (progress?.cfi) {
    try {
      if (view.resolveNavigation(progress.cfi)) {
        await view.init({ lastLocation: progress.cfi })
        return
      }
    } catch (e) {
      console.warn('saved position no longer resolves', e)
    }
    if (progress.fraction > 0) {
      try {
        await view.goToFraction(progress.fraction)
        return
      } catch (e) {
        console.warn('could not go to saved fraction', e)
      }
    }
  }
  await view.init({ showTextStart: true })
}

async function close() {
  if (!root || root.hidden) return
  await flushSave()
  view?.close()
  view?.remove()
  view = null
  current = null
  position = null
  backCfi = null
  $('.r-back').hidden = true
  $('.r-popover').hidden = true
  $('.r-toc').innerHTML = ''
  root.hidden = true
  document.body.classList.remove('reader-open')
  Reader.onClose?.()
}

// --- Position tracking ---

function onRelocate({ detail }) {
  position = detail
  const { fraction = 0, tocItem } = detail
  $('.r-slider').value = fraction
  $('.r-percent').textContent = `${Math.round(fraction * 100)}%`
  $('.r-chapter').textContent = tocItem?.label?.trim() ?? ''
  $('.r-left').textContent = timeLeft(fraction)
  markCurrentTOC(tocItem?.href)

  clearTimeout(saveTimer)
  saveTimer = setTimeout(flushSave, SAVE_DELAY_MS)
}

function timeLeft(fraction) {
  if (!current?.wordCount) return ''
  const minutes = Math.round(current.wordCount * (1 - fraction) / WORDS_PER_MINUTE)
  if (minutes < 1) return ''
  const h = Math.floor(minutes / 60)
  const m = minutes % 60
  return `~${h ? `${h}h ` : ''}${m}m left`
}

async function flushSave() {
  clearTimeout(saveTimer)
  saveTimer = null
  if (!current || !position?.cfi) return
  try {
    await invoke('reader_save_progress', {
      repo: current.repo, cfi: position.cfi, fraction: position.fraction ?? 0,
    })
  } catch (err) {
    console.error('could not save reading position', err)
  }
}

// --- Settings ---

function applyChromeSettings() {
  root.dataset.theme = settings.theme
  $('.r-side').hidden = !settings.toc_open
  $('[data-act="toc"]').classList.toggle('active', settings.toc_open)
  for (const seg of root.querySelectorAll('.r-seg')) {
    const value = settings[seg.dataset.setting]
    for (const b of seg.children) b.classList.toggle('active', b.dataset.value === value)
  }
  $('.r-size-val').textContent = `${settings.font_size}%`
}

function applyBookSettings() {
  const r = view?.renderer
  if (!r) return
  r.setAttribute('flow', settings.layout === 'scroll' ? 'scrolled' : 'paginated')
  r.setAttribute('max-column-count', settings.layout === 'spread' ? '2' : '1')
  r.setStyles?.(bookCSS(settings))
}

let settingsSaveTimer = null
function updateSetting(key, value) {
  settings = { ...settings, [key]: value }
  applyChromeSettings()
  applyBookSettings()
  clearTimeout(settingsSaveTimer)
  settingsSaveTimer = setTimeout(async () => {
    try {
      settings = await invoke('reader_save_settings', { settings })
    } catch (err) {
      console.error('could not save reader settings', err)
    }
  }, 300)
}

function changeFontSize(delta) {
  const size = Math.min(200, Math.max(70, settings.font_size + delta))
  if (size !== settings.font_size) updateSetting('font_size', size)
}

function toggleSide() {
  updateSetting('toc_open', !settings.toc_open)
}

function togglePopover() {
  $('.r-popover').hidden = !$('.r-popover').hidden
}

// --- Table of contents ---

function renderTOC(toc) {
  const nav = $('.r-toc')
  nav.innerHTML = ''
  nav.append(tocList(toc))
}

function tocList(items) {
  const ul = document.createElement('ul')
  for (const item of items) {
    const li = document.createElement('li')
    const a = document.createElement('a')
    a.href = '#'
    a.textContent = item.label?.trim() || '—'
    a.dataset.href = item.href ?? ''
    a.addEventListener('click', e => {
      e.preventDefault()
      if (item.href) view?.goTo(item.href)
    })
    if (item.subitems?.length) {
      const twisty = document.createElement('span')
      twisty.className = 'twisty'
      twisty.textContent = '▾'
      twisty.addEventListener('click', e => {
        e.stopPropagation()
        li.classList.toggle('collapsed')
        twisty.textContent = li.classList.contains('collapsed') ? '▸' : '▾'
      })
      a.prepend(twisty)
      li.append(a, tocList(item.subitems))
    } else {
      li.append(a)
    }
    ul.append(li)
  }
  return ul
}

function markCurrentTOC(href) {
  for (const a of root.querySelectorAll('.r-toc a.current')) a.classList.remove('current')
  if (!href) return
  const a = [...root.querySelectorAll('.r-toc a')].find(x => x.dataset.href === href)
  if (!a) return
  a.classList.add('current')
  a.scrollIntoView({ block: 'nearest' })
}

// --- Links ---

function onExternalLink(e) {
  e.preventDefault()
  const url = e.detail.href
  if (/^(https?|mailto):/i.test(url))
    invoke('plugin:opener|open_url', { url }).catch(err => console.error(err))
}

function onInternalLink() {
  // Remember where we were so endnote jumps can come back
  if (position?.cfi) {
    backCfi = position.cfi
    $('.r-back').hidden = false
  }
}

async function goBack() {
  $('.r-back').hidden = true
  if (backCfi) await view?.goTo(backCfi)
  backCfi = null
}

// --- Input inside book pages ---

function onSectionLoad({ detail: { doc } }) {
  doc.addEventListener('keydown', onKey)
  doc.addEventListener('mousemove', e => {
    const frame = doc.defaultView.frameElement?.getBoundingClientRect()
    onPointer((frame?.top ?? 0) + e.clientY)
  })
  doc.addEventListener('click', e => onPageClick(e, doc))
  doc.addEventListener('wheel', onWheel, { passive: true })
}

function onPageClick(e, doc) {
  if (settings.layout === 'scroll' || e.target.closest?.('a')) return
  if (!doc.getSelection()?.isCollapsed) return
  $('.r-popover').hidden = true
  const frame = doc.defaultView.frameElement?.getBoundingClientRect()
  const area = $('.r-view').getBoundingClientRect()
  const x = (frame?.left ?? 0) + e.clientX - area.left
  if (x < area.width / 3) view.goLeft()
  else if (x > area.width * 2 / 3) view.goRight()
}

function onWheel(e) {
  if (settings.layout === 'scroll') return
  const now = Date.now()
  if (now < wheelLock || Math.abs(e.deltaY) < 4) return
  wheelLock = now + 350
  if (e.deltaY > 0) view.next()
  else view.prev()
}

// --- Keyboard ---

function onKey(e) {
  if (root.hidden) return
  // Esc works even while an error message is shown or the slider has focus
  if (e.key === 'Escape') {
    e.preventDefault()
    onEscape()
    return
  }
  if (!view || e.target.closest?.('input, textarea, select')) return
  switch (e.key) {
    case 'ArrowLeft': view.goLeft(); break
    case 'ArrowRight': view.goRight(); break
    case 'PageUp': view.prev(); break
    case 'PageDown': view.next(); break
    case ' ': e.shiftKey ? view.prev() : view.next(); break
    case 'Home': goToSectionEdge(0); break
    case 'End': goToSectionEdge(1); break
    case 't': case 'T': toggleSide(); break
    case '+': case '=': changeFontSize(10); break
    case '-': case '_': changeFontSize(-10); break
    default: return
  }
  e.preventDefault()
}

/** anchor 0 = first page of the current chapter file, 1 = last page */
function goToSectionEdge(anchor) {
  const index = view.renderer.getContents()[0]?.index
  if (index != null) view.renderer.goTo({ index, anchor })
}

function onEscape() {
  if (!$('.r-popover').hidden) $('.r-popover').hidden = true
  else if (settings.toc_open) toggleSide()
  else close()
}

// --- Auto-hiding toolbar ---

function onPointer(y) {
  if (y < 70) {
    root.classList.remove('chrome-hidden')
    scheduleChromeHide()
  }
}

function scheduleChromeHide() {
  clearTimeout(chromeTimer)
  chromeTimer = setTimeout(() => {
    if ($('.r-popover').hidden) root.classList.add('chrome-hidden')
  }, CHROME_HIDE_MS)
}

// --- Messages ---

function showMessage(text, ...buttons) {
  const box = $('.r-message')
  box.innerHTML = ''
  const p = document.createElement('p')
  p.textContent = text
  box.append(p, ...buttons)
  box.hidden = false
  root.classList.remove('chrome-hidden')
}

function hideMessage() {
  $('.r-message').hidden = true
}

function button(label, onClick) {
  const b = document.createElement('button')
  b.textContent = label
  b.addEventListener('click', onClick)
  return b
}

function showMissing(repo) {
  showMessage('The epub file for this book was not found.',
    button('Build again', async () => {
      showMessage('Building epub…')
      try {
        await invoke('build_and_wait', { repo })
        Reader.onLibraryChanged?.()
        await open(repo)
      } catch (err) {
        showMessage(`Build failed: ${err}`)
      }
    }))
}

function showOpenFailed(repo, err) {
  showMessage(`This book could not be displayed: ${err?.message ?? err}`,
    button('Open in system reader', () =>
      invoke('open_epub', { repo }).catch(e => showMessage(`Could not open: ${e}`))))
}

// --- Context for future panels (notes, AI) ---

function getReaderContext() {
  if (!current) return null
  let selectionText = ''
  let selectionCfi = null
  for (const { doc, index } of view?.renderer?.getContents?.() ?? []) {
    const sel = doc.getSelection()
    if (sel && !sel.isCollapsed) {
      selectionText = sel.toString()
      selectionCfi = view.getCFI(index, sel.getRangeAt(0))
      break
    }
  }
  return {
    repo: current.repo,
    title: current.title,
    author: current.author,
    cfi: position?.cfi ?? null,
    fraction: position?.fraction ?? 0,
    chapterTitle: position?.tocItem?.label?.trim() ?? '',
    selectionText,
    selectionCfi,
  }
}

document.addEventListener('keydown', e => {
  if (root && !root.hidden) {
    onKey(e)
    e.stopImmediatePropagation()
  }
}, true)

const Reader = {
  open,
  close,
  isOpen: () => !!root && !root.hidden,
  getReaderContext,
  onClose: null,
  onLibraryChanged: null,
}
window.Reader = Reader
export default Reader
