// Michael Burgus — https://github.com/NeuralDrifter
// In-app EPUB reader (foliate-js), shown in place of the library inside the app shell

import { makeBook } from '../../vendor/foliate-js/view.js'
import { attachSanitizer } from '../../sanitize.js'
import { api } from '../api.js'
import { $, esc, icon } from '../util.js'
import { R, rq, currentSection, fractionInSection, cleanLabel } from './ctx.js'
import { applyBookSettings, applyThemeVars, saveSettings, changeFontSize, initTypeset, typesetHtml, handleTypesetClick, handleTypesetInput } from './typeset.js'
import { loadContents, contentsShell, renderContents, markContents, handleContentsClick, handleContentsInput, chapterMarks, shortLabel } from './contents.js'
import { resetSearch, searchHtml, handleSearchSubmit, handleSearchClick, focusSearch } from './search.js'
import { bookmarkHere, toggleBookmark, bookmarksHtml, handleBookmarksClick } from './bookmarks.js'
import { startPace, trackPace, stopPace } from './pace.js'
import { createFormats } from '../formats.js'
import { define, resetLexicon, lexiconHtml, handleLexiconClick } from './lexicon.js'
import { initAnnotations, attachAnnotations, onPageSelection, popoverJustOpened, popoverOpen, hidePopover, suppressPopover, annotationsHtml, handleAnnotationsClick, handleAnnotationsSubmit, resetAnnotations } from './annotations.js'

const { convertFileSrc } = window.__TAURI__.core
const win = window.__TAURI__.window.getCurrentWindow()

const SAVE_DELAY_MS = 1000
const WHEEL_LOCK_MS = 350

let saveTimer = null
let wheelLock = 0
let fullscreen = false

const formats = createFormats({
  container: () => R.el && rq('[data-formats]'),
  repo: () => R.repo,
  onError: text => {
    const box = rq('[data-fmt-error]')
    box.textContent = text
    box.hidden = false
  },
})

// --- Shell ---

const tool = (act, iconName, label, title) => `
  <button data-act="${act}" title="${title}" class="r-tool flex items-center gap-1 h-8 px-2 rounded-lg text-label-md text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface transition-colors">
    ${icon(iconName, 'text-[18px]')}${label ? `<span>${label}</span>` : ''}
  </button>`

function build() {
  R.el = $('#reader')
  R.el.innerHTML = `
    <header class="shrink-0 flex items-start justify-between gap-space-md px-margin py-space-sm bg-surface-container-low/80 shadow-[0_1px_8px_rgba(0,0,0,0.3)]">
      <div class="flex items-center gap-space-md min-w-0">
        <button data-act="close" class="btn-glass shrink-0" title="Back to the library (Esc)">${icon('arrow_back', 'text-[18px]')}Shelves</button>
        <div class="min-w-0">
          <div class="flex items-center gap-2 min-w-0">
            <h2 data-title class="font-display text-headline-sm text-on-surface truncate"></h2>
            <span class="shrink-0 text-[10px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded-lg bg-primary/15 text-primary">Standard Ebooks Edition</span>
          </div>
          <div data-byline class="text-label-sm text-on-surface-variant truncate"></div>
        </div>
      </div>
      <div class="flex flex-col items-end gap-1 shrink-0">
        <div data-where class="flex items-center gap-1.5 text-label-md text-on-surface-variant max-w-[36rem] min-h-6"></div>
        <div class="flex items-center gap-0.5">
          ${tool('contents', 'format_list_bulleted', 'Contents', 'Contents (T)')}
          ${tool('typeset', 'text_fields', 'Typesetting', 'Typesetting')}
          ${tool('search', 'search', '', 'Search the text (Ctrl+F)')}
          ${tool('bookmark', 'bookmark_add', '', 'Bookmark this page (B)')}
          <button data-act="bookmarks" title="Bookmarks" class="r-tool flex items-center gap-1 h-8 px-2 rounded-lg text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface">
            ${icon('bookmarks', 'text-[18px]')}<span data-mark-count class="text-[10px] font-semibold min-w-4 px-1 rounded-md bg-secondary-container text-on-secondary-container"></span>
          </button>
          <button data-act="annotations" title="Highlights and notes (N)" class="r-tool flex items-center gap-1 h-8 px-2 rounded-lg text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface">
            ${icon('ink_highlighter', 'text-[18px]')}<span data-ann-count class="text-[10px] font-semibold min-w-4 px-1 rounded-md bg-secondary-container text-on-secondary-container"></span>
          </button>
          ${tool('lexicon', 'dictionary', '', 'Dictionary (double-click a word)')}
          ${tool('fullscreen', 'fullscreen', '', 'Fullscreen (F)')}
        </div>
      </div>
    </header>

    <div class="flex-1 min-h-0 flex gap-gutter px-margin py-space-md">
      <aside data-left class="w-64 shrink-0 flex flex-col gap-space-sm min-h-0"></aside>

      <div class="flex-1 min-w-0 flex flex-col gap-space-sm">
        <div class="relative flex-1 min-h-0">
          <div class="r-page absolute inset-0 rounded-2xl shadow-2xl overflow-hidden flex flex-col">
            <div class="r-view relative flex-1 min-h-0"></div>
            <div class="shrink-0 h-10 flex items-center justify-between gap-space-md px-space-md text-label-sm r-page-dim">
              <span data-page-note class="flex items-center gap-1.5 min-w-0 truncate"></span>
              <div class="flex items-center gap-space-md shrink-0">
                <button data-act="prev-section" class="hover:underline">Previous Section</button>
                <button data-act="next-section" class="flex items-center gap-1 hover:underline">Next Section${icon('arrow_forward', 'text-[16px]')}</button>
              </div>
            </div>
            <div data-message class="absolute inset-0 z-10 flex flex-col items-center justify-center gap-space-sm p-space-lg text-center text-body-md r-page-solid" hidden></div>
          </div>
          <button data-act="left" title="Previous page" class="absolute left-0 top-1/2 -translate-x-1/2 -translate-y-1/2 w-9 h-9 grid place-items-center rounded-xl bg-surface-container-high text-on-surface-variant shadow-lg hover:text-primary z-10">${icon('chevron_left')}</button>
          <button data-act="right" title="Next page" class="absolute right-0 top-1/2 translate-x-1/2 -translate-y-1/2 w-9 h-9 grid place-items-center rounded-xl bg-surface-container-high text-on-surface-variant shadow-lg hover:text-primary z-10">${icon('chevron_right')}</button>
        </div>

        <div class="shrink-0 px-space-md py-space-sm rounded-xl bg-surface-container-low shadow-lg">
          <div class="flex items-center justify-between gap-space-md text-label-sm">
            <span data-sc-chapter class="truncate text-on-surface-variant"></span>
            <span data-sc-progress class="shrink-0 text-primary"></span>
          </div>
          <div class="relative h-5 mt-1">
            <div data-sc-ticks class="absolute inset-x-0 top-1/2 h-2 -translate-y-1/2 pointer-events-none"></div>
            <input data-scrub type="range" min="0" max="1" step="0.0001" value="0" class="r-range r-scrub absolute inset-0 w-full">
          </div>
          <div data-sc-labels class="relative h-4 mt-0.5 text-[10px] text-outline"></div>
        </div>
      </div>

      <aside data-right class="w-72 shrink-0 flex flex-col gap-space-sm min-h-0 overflow-y-auto" hidden>
        <section data-panel class="flex flex-col gap-space-sm p-space-md rounded-xl bg-surface-container-low shadow-lg"></section>
        <section class="flex flex-col gap-space-sm p-space-md rounded-xl bg-surface-container-low shadow-lg">
          <span class="eyebrow">Archival Formats</span>
          <div data-formats class="flex flex-col gap-1.5"></div>
          <div data-fmt-error class="text-body-sm text-error" hidden></div>
          <button data-act="se-page" class="self-start text-label-sm text-secondary hover:underline flex items-center gap-1">${icon('language', 'text-[15px]')}View on standardebooks.org</button>
        </section>
      </aside>
    </div>`

  R.el.addEventListener('click', onClick)
  R.el.addEventListener('input', onInput)
  R.el.addEventListener('change', e => {
    if (e.target.matches('[data-range="font_size"]')) renderPanel()
  })
  R.el.addEventListener('submit', async e => {
    // Both handlers call preventDefault before their first await
    if (handleSearchSubmit(e)) return
    if (await handleAnnotationsSubmit(e)) renderPanel()
  })
  // The popover stops its own pointerdowns, so any that reach here are outside it
  R.el.addEventListener('pointerdown', () => popoverOpen() && hidePopover())
  window.addEventListener('blur', flushSave)
  initTypeset(onSettingChanged)
  initAnnotations({
    onChange: () => {
      renderMarks()
      if (R.panel === 'annotations') renderPanel()
    },
    onDefine: lookUp,
  })
}

async function onClick(e) {
  if (formats.handleClick(e)) return
  const act = e.target.closest('[data-act]')?.dataset.act
  switch (act) {
    case 'close': return close()
    case 'contents': return toggleContents()
    case 'typeset': return togglePanel('typeset')
    case 'search': return togglePanel('search')
    case 'bookmarks': return togglePanel('bookmarks')
    case 'annotations': return togglePanel('annotations')
    case 'lexicon': return togglePanel('lexicon')
    case 'bookmark':
      await toggleBookmark()
      renderMarks()
      if (R.panel === 'bookmarks') renderPanel()
      return
    case 'fullscreen': return setFullscreen(!fullscreen)
    case 'left': return R.view?.goLeft()
    case 'right': return R.view?.goRight()
    case 'prev-section': return goSection(-1)
    case 'next-section': return goSection(1)
    case 'back': return goBack()
    case 'se-page': return R.info?.url && api.openUrl(R.info.url).catch(err => console.error(err))
  }
  if (handleContentsClick(e)) return
  if (R.panel === 'typeset' && handleTypesetClick(e)) return
  if (R.panel === 'search') {
    if (handleSearchClick(e) === 'rerender') renderPanel()
    return
  }
  if (R.panel === 'bookmarks' && (await handleBookmarksClick(e)) === 'rerender') {
    renderPanel()
    renderMarks()
  }
  if (R.panel === 'lexicon' && handleLexiconClick(e, renderPanel)) return
  if (R.panel === 'annotations' && (await handleAnnotationsClick(e)) === 'rerender') {
    renderPanel()
    renderMarks()
  }
}

function onInput(e) {
  if (e.target.matches('[data-scrub]')) {
    R.view?.goToFraction(parseFloat(e.target.value))
    return
  }
  if (handleContentsInput(e)) return
  handleTypesetInput(e)
}

function onSettingChanged(key) {
  if (key === 'toc_open') renderLayout()
  if (R.panel === 'typeset') renderPanel()
}

// --- Open / close ---

async function open(repo) {
  if (!R.el) build()
  if (R.repo) await close({ keepShell: true })

  R.repo = repo
  $('#main').hidden = true
  R.el.hidden = false
  document.body.classList.add('reader-open')
  showMessage('Opening…')
  R.settings ??= await api.readerGetSettings()
  applyThemeVars()
  R.panel = R.settings.typeset_open ? 'typeset' : null

  let info
  try {
    info = await api.readerOpen(repo)
  } catch (err) {
    if (String(err) === 'not-found') showMissing(repo)
    else showMessage(`Could not open this book: ${err}`)
    return
  }
  if (R.repo !== repo) return // another book was opened meanwhile
  R.info = info
  R.bookmarks = info.bookmarks ?? []
  R.annotations = info.annotations ?? []
  R.pace = info.pace
  rq('[data-fmt-error]').hidden = true
  formats.load()
  rq('[data-title]').textContent = info.title || repo
  rq('[data-byline]').textContent = [info.author, info.published?.label].filter(Boolean).join(' • ')

  try {
    const res = await fetch(convertFileSrc(info.path))
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
    const file = new File([await res.blob()], `${repo}.epub`, { type: 'application/epub+zip' })

    // Sanitize before any page of the book is rendered
    const book = await makeBook(file)
    attachSanitizer(book)
    if (R.repo !== repo) return

    R.book = book
    rq('[data-left]').innerHTML = contentsShell()
    R.view = document.createElement('foliate-view')
    rq('.r-view').append(R.view)
    R.view.addEventListener('relocate', onRelocate)
    R.view.addEventListener('load', onSectionLoad)
    R.view.addEventListener('external-link', onExternalLink)
    R.view.addEventListener('link', onInternalLink)
    attachAnnotations(R.view)
    await R.view.open(book)
    R.fractions = R.view.getSectionFractions()
    applyBookSettings()

    loadContents()
    renderContents()
    renderTicks()
    renderLayout()
    renderMarks()
    startPace()
    document.fonts?.load('16px "Playfair Display"').then(() => R.panel === 'typeset' && renderPanel())

    await goToSaved(info.progress)
    hideMessage()
  } catch (err) {
    console.error(err)
    showOpenFailed(repo, err)
  }
}

/** Resume at the saved CFI; if it no longer resolves, use the saved fraction; else the text start. */
async function goToSaved(progress) {
  if (progress?.cfi) {
    try {
      if (R.view.resolveNavigation(progress.cfi)) {
        await R.view.init({ lastLocation: progress.cfi })
        return
      }
    } catch (e) {
      console.warn('saved position no longer resolves', e)
    }
    if (progress.fraction > 0) {
      try {
        await R.view.goToFraction(progress.fraction)
        return
      } catch (e) {
        console.warn('could not go to saved fraction', e)
      }
    }
  }
  await R.view.init({ showTextStart: true })
}

async function close({ keepShell = false } = {}) {
  if (!R.el || R.el.hidden) return
  await flushSave()
  await stopPace()
  resetSearch()
  resetAnnotations()
  resetLexicon()
  R.view?.close()
  R.view?.remove()
  Object.assign(R, { view: null, book: null, info: null, repo: null, position: null, fractions: [], bookmarks: [], annotations: [], backCfi: null })
  rq('[data-left]').innerHTML = ''
  rq('[data-where]').innerHTML = ''
  rq('[data-page-note]').innerHTML = ''
  if (keepShell) return
  if (fullscreen) await setFullscreen(false)
  R.el.hidden = true
  $('#main').hidden = false
  document.body.classList.remove('reader-open')
  Reader.onClose?.()
}

// --- Position ---

function onRelocate({ detail }) {
  if (!R.info) return
  R.position = detail
  hidePopover()
  const f = detail.fraction ?? 0
  const i = currentSection()
  const chapter = cleanLabel(detail.tocItem?.label)
  const loc = detail.location
  const locText = loc ? `Location ${loc.current + 1} of ${loc.total}` : ''
  const pct = Math.floor(f * 100)

  rq('[data-where]').innerHTML = `
    ${icon('menu_book', 'text-[16px] text-primary')}
    <span class="truncate text-on-surface">${esc(chapter)}</span>
    ${locText ? `<span class="text-outline">•</span><span class="shrink-0 text-primary font-semibold">${locText}</span>` : ''}
    <span class="shrink-0 text-outline">(${pct}% overall)</span>`

  const inChapter = i >= 0 ? Math.floor(fractionInSection(i, f) * 100) : 0
  rq('[data-sc-chapter]').textContent = chapter
  rq('[data-sc-progress]').textContent = `Progress: ${inChapter}% of ${shortLabel(chapter) || 'section'} • ${pct}% book`
  const scrub = rq('[data-scrub]')
  scrub.value = f
  scrub.style.setProperty('--fill', `${f * 100}%`)

  renderPageNote()
  renderHeads()
  markContents()
  renderMarks()
  trackPace(f)

  clearTimeout(saveTimer)
  saveTimer = setTimeout(flushSave, SAVE_DELAY_MS)
}

/** Running head above the text: title / author / location. */
function renderHeads() {
  const heads = R.view?.renderer?.heads
  if (!heads?.length) return
  const cell = text => `<span style="overflow:hidden;text-overflow:ellipsis">${esc(text)}</span>`
  const row = cells => `<div style="display:flex;justify-content:space-between;gap:1.5em">${cells.join('')}</div>`
  const title = R.info?.title ?? ''
  const author = R.info?.author ?? ''
  const loc = R.position?.location ? String(R.position.location.current + 1) : ''
  if (heads.length === 1) heads[0].innerHTML = row([cell(title), cell(author), cell(loc)])
  else {
    heads[0].innerHTML = row([cell(title), cell('')])
    heads[heads.length - 1].innerHTML = row([cell(author), cell(loc)])
  }
  for (const foot of R.view.renderer.feet ?? []) foot.textContent = ''
}

function renderPageNote() {
  rq('[data-page-note]').innerHTML = R.backCfi
    ? `<button data-act="back" class="flex items-center gap-1 hover:underline">${icon('undo', 'text-[16px]')}Back to where you were</button>`
    : `<span class="w-1.5 h-1.5 rounded-full shrink-0" style="background:currentColor"></span><span class="truncate">Standard Ebooks · ${esc(R.info?.published?.label ? `first published ${R.info.published.label}` : 'public domain edition')}</span>`
}

async function flushSave() {
  clearTimeout(saveTimer)
  saveTimer = null
  if (!R.repo || !R.position?.cfi) return
  try {
    await api.readerSaveProgress(R.repo, R.position.cfi, R.position.fraction ?? 0)
  } catch (err) {
    console.error('could not save reading position', err)
  }
}

/** Jump to the start of the previous / next section that has text. */
function goSection(dir) {
  const sizes = R.fractions.slice(1).map((x, i) => x - R.fractions[i])
  let i = currentSection() + dir
  while (i >= 0 && i < sizes.length && !(sizes[i] > 1e-9)) i += dir
  if (i >= 0 && i < sizes.length) R.view.goTo(i).catch(err => console.error(err))
}

// --- Layout and panels ---

function renderLayout() {
  rq('[data-left]').hidden = !R.settings.toc_open
  rq('[data-right]').hidden = !R.panel
  for (const [act, on] of [['contents', R.settings.toc_open], ['typeset', R.panel === 'typeset'],
    ['search', R.panel === 'search'], ['bookmarks', R.panel === 'bookmarks'], ['annotations', R.panel === 'annotations'], ['lexicon', R.panel === 'lexicon']]) {
    const b = rq(`[data-act="${act}"]`)
    b.classList.toggle('text-primary', on)
    b.classList.toggle('bg-surface-container', on)
  }
  renderPanel()
}

function renderPanel() {
  const el = rq('[data-panel]')
  if (!R.panel || !R.info) return
  el.innerHTML = { typeset: typesetHtml, search: searchHtml, bookmarks: bookmarksHtml, annotations: annotationsHtml, lexicon: lexiconHtml }[R.panel]()
}

/** Show the dictionary card for a word. */
function lookUp(word) {
  if (R.panel !== 'lexicon') {
    R.panel = 'lexicon'
    if (R.settings.typeset_open) {
      R.settings = { ...R.settings, typeset_open: false }
      saveSettings()
    }
    renderLayout()
  }
  define(word, renderPanel)
}

function toggleContents() {
  R.settings = { ...R.settings, toc_open: !R.settings.toc_open }
  saveSettings()
  renderLayout()
}

function togglePanel(panel) {
  R.panel = R.panel === panel ? null : panel
  const typesetOpen = R.panel === 'typeset'
  if (typesetOpen !== R.settings.typeset_open) {
    R.settings = { ...R.settings, typeset_open: typesetOpen }
    saveSettings()
  }
  if (panel === 'search' && R.panel !== 'search') resetSearch()
  renderLayout()
  if (R.panel === 'search') focusSearch()
}

function renderMarks() {
  const here = !!bookmarkHere()
  const b = rq('[data-act="bookmark"]')
  const glyph = b.querySelector('.material-symbols-outlined')
  glyph.textContent = here ? 'bookmark' : 'bookmark_add'
  glyph.classList.toggle('icon-fill', here)
  b.classList.toggle('text-primary', here)
  b.title = here ? 'Remove the bookmark on this page (B)' : 'Bookmark this page (B)'
  const count = rq('[data-mark-count]')
  count.textContent = R.bookmarks.length || ''
  count.hidden = !R.bookmarks.length
  const notes = rq('[data-ann-count]')
  notes.textContent = R.annotations.length || ''
  notes.hidden = !R.annotations.length
}

/** Chapter ticks along the scrubber, with a few labels underneath. */
function renderTicks() {
  const marks = chapterMarks()
  rq('[data-sc-ticks]').innerHTML = marks
    .filter(m => m.fraction > 0.002 && m.fraction < 0.998)
    .map(m => `<span class="absolute top-0 bottom-0 w-px bg-outline/50" style="left:${m.fraction * 100}%"></span>`).join('')

  const picks = []
  if (marks.length) {
    picks.push({ ...marks[0], fraction: 0, align: 'left', text: `0% ${shortLabel(marks[0].label)}` })
    for (const target of [0.25, 0.5, 0.75]) {
      const m = marks.reduce((best, x) => Math.abs(x.fraction - target) < Math.abs(best.fraction - target) ? x : best)
      if (Math.abs(m.fraction - target) < 0.12 && !picks.some(p => p.index === m.index))
        picks.push({ ...m, align: 'center', text: shortLabel(m.label) })
    }
    const last = marks[marks.length - 1]
    if (marks.length > 1) picks.push({ ...last, fraction: 1, align: 'right', text: `100% ${shortLabel(last.label)}` })
  }
  rq('[data-sc-labels]').innerHTML = picks.map(p => {
    const pos = p.align === 'left' ? 'left:0' : p.align === 'right' ? 'right:0' : `left:${p.fraction * 100}%;transform:translateX(-50%)`
    return `<span class="absolute top-0 whitespace-nowrap" style="${pos}">${esc(p.text)}</span>`
  }).join('')
}

async function setFullscreen(on) {
  fullscreen = on
  document.body.classList.toggle('reader-full', on)
  try {
    await win.setFullscreen(on)
  } catch (err) {
    console.error('fullscreen', err)
  }
  const b = rq('[data-act="fullscreen"] .material-symbols-outlined')
  if (b) b.textContent = on ? 'fullscreen_exit' : 'fullscreen'
}

// --- Links ---

function onExternalLink(e) {
  e.preventDefault()
  const url = e.detail.href_ ?? e.detail.href
  if (/^(https?|mailto):/i.test(url)) api.openUrl(url).catch(err => console.error(err))
}

function onInternalLink() {
  // Remember where we were so endnote jumps can come back
  if (R.position?.cfi) {
    R.backCfi = R.position.cfi
    renderPageNote()
  }
}

async function goBack() {
  const cfi = R.backCfi
  R.backCfi = null
  renderPageNote()
  if (cfi) await R.view?.goTo(cfi)
}

// --- Input inside book pages ---

function onSectionLoad({ detail: { doc, index } }) {
  doc.addEventListener('keydown', onKey)
  doc.addEventListener('click', e => onPageClick(e, doc))
  doc.addEventListener('wheel', onWheel, { passive: true })
  doc.addEventListener('pointerdown', () => hidePopover())
  doc.addEventListener('pointerup', () => onPageSelection(doc, index))
  doc.addEventListener('keyup', e => e.shiftKey && onPageSelection(doc, index))
  doc.addEventListener('dblclick', e => {
    if (e.target.closest?.('a')) return
    const text = doc.getSelection()?.toString().trim()
    if (!text || /\s/.test(text)) return
    suppressPopover()
    lookUp(text)
  })
}

function onPageClick(e, doc) {
  if (R.settings.layout === 'scroll' || e.target.closest?.('a')) return
  if (!doc.getSelection()?.isCollapsed) return
  const frame = doc.defaultView.frameElement?.getBoundingClientRect()
  const area = rq('.r-view').getBoundingClientRect()
  const x = (frame?.left ?? 0) + e.clientX - area.left
  // A click on a highlight opens its popover (foliate handles it after us), not a page turn
  setTimeout(() => {
    if (popoverJustOpened()) return
    if (x < area.width / 4) R.view?.goLeft()
    else if (x > area.width * 3 / 4) R.view?.goRight()
  })
}

function onWheel(e) {
  if (R.settings.layout === 'scroll') return
  const now = Date.now()
  if (now < wheelLock || Math.abs(e.deltaY) < 4) return
  wheelLock = now + WHEEL_LOCK_MS
  if (e.deltaY > 0) R.view.next()
  else R.view.prev()
}

// --- Keyboard ---

/** Returns true when the key was handled by the reader. */
function onKey(e) {
  if (!R.el || R.el.hidden) return false
  // Typing in the library toolbar (outside the reader) is not reader input
  if (e.target instanceof Node && e.target.isConnected && !R.el.contains(e.target)
    && e.target.closest?.('input, textarea, select')) return false
  if (e.key === 'Escape') {
    e.preventDefault()
    onEscape()
    return true
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
    e.preventDefault()
    if (R.panel !== 'search') togglePanel('search')
    else focusSearch()
    return true
  }
  if (e.ctrlKey || e.metaKey || e.altKey) return false
  if (!R.view || e.target.closest?.('input, textarea, select')) return false
  const v = R.view
  switch (e.key) {
    case 'ArrowLeft': v.goLeft(); break
    case 'ArrowRight': v.goRight(); break
    case 'PageUp': v.prev(); break
    case 'PageDown': v.next(); break
    case ' ': e.shiftKey ? v.prev() : v.next(); break
    case 'Home': goToSectionEdge(0); break
    case 'End': goToSectionEdge(1); break
    case 't': case 'T': toggleContents(); break
    case 'b': case 'B': onClick({ target: rq('[data-act="bookmark"]') }); break
    case 'n': case 'N': togglePanel('annotations'); break
    case 'f': case 'F': setFullscreen(!fullscreen); break
    case '+': case '=': changeFontSize(1); break
    case '-': case '_': changeFontSize(-1); break
    default: return false
  }
  e.preventDefault()
  return true
}

/** anchor 0 = first page of the current chapter file, 1 = last page */
function goToSectionEdge(anchor) {
  const index = R.view.renderer.getContents()[0]?.index
  if (index != null) R.view.renderer.goTo({ index, anchor })
}

function onEscape() {
  if (popoverOpen()) hidePopover()
  else if (fullscreen) setFullscreen(false)
  else if (R.panel) togglePanel(R.panel)
  else close()
}

document.addEventListener('keydown', e => {
  if (R.el && !R.el.hidden && onKey(e)) e.stopImmediatePropagation()
}, true)

// --- Messages ---

function showMessage(text, ...buttons) {
  const box = rq('[data-message]')
  box.innerHTML = ''
  const p = document.createElement('p')
  p.textContent = text
  box.append(p, ...buttons)
  box.hidden = false
}

function hideMessage() {
  rq('[data-message]').hidden = true
}

function button(label, onClick) {
  const b = document.createElement('button')
  b.className = 'btn-gold'
  b.textContent = label
  b.addEventListener('click', onClick)
  return b
}

function showMissing(repo) {
  showMessage('The epub file for this book was not found.',
    button('Build again', async () => {
      showMessage('Building epub…')
      try {
        await api.buildAndWait(repo)
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
      api.openEpub(repo).catch(e => showMessage(`Could not open: ${e}`))))
}

// --- Context for future panels (AI) ---

function getReaderContext() {
  if (!R.repo) return null
  let selectionText = ''
  let selectionCfi = null
  for (const { doc, index } of R.view?.renderer?.getContents?.() ?? []) {
    const sel = doc.getSelection()
    if (sel && !sel.isCollapsed) {
      selectionText = sel.toString()
      selectionCfi = R.view.getCFI(index, sel.getRangeAt(0))
      break
    }
  }
  return {
    repo: R.repo,
    title: R.info?.title ?? '',
    author: R.info?.author ?? '',
    cfi: R.position?.cfi ?? null,
    fraction: R.position?.fraction ?? 0,
    chapterTitle: cleanLabel(R.position?.tocItem?.label),
    selectionText,
    selectionCfi,
  }
}

const Reader = {
  open,
  close: () => close(),
  isOpen: () => !!R.el && !R.el.hidden,
  getReaderContext,
  onClose: null,
  onLibraryChanged: null,
}
window.Reader = Reader
export default Reader
