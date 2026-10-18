// Michael Burgus — https://github.com/NeuralDrifter
// Curator Inspector: the selected book's details, build output and actions

import { api } from './api.js'
import { state, on, emit } from './state.js'
import { build, openReader } from './actions.js'
import { rowOf, statusOf } from './grid.js'
import { toggleHtml, handleToggleClick } from './toolbar.js'
import { $, esc, num, bytes, icon, coverUrl, plainParagraphs, dateTime } from './util.js'

let book = null     // row currently shown
let renderedKey = '' // status key at last full render
let logTimer = null

const STATUS_CHIP = {
  built: ['bg-tertiary/10 text-tertiary', 'Built edition'],
  building: ['bg-primary/10 text-primary', 'Active pipeline unit'],
  queued: ['bg-secondary/10 text-secondary', 'Queued for build'],
  failed: ['bg-error/10 text-error', 'Build failed'],
  not_built: ['bg-surface-container text-outline', 'Not built yet'],
}

export function initInspector() {
  const el = $('#inspector')
  el.addEventListener('click', onClick)
  on('inspected', load)
  on('live-inspected', () => {
    book = rowOf(state.inspected) ?? book
    // Progress ticks only patch numbers; a state change re-renders
    if (book && statusOf(book).key === renderedKey && renderedKey === 'building') patchProgress()
    else render()
  })
  on(['result', 'settings', 'reading'], () => {
    if (!state.inspected) return
    const fresh = rowOf(state.inspected)
    if (fresh) book = fresh
    render()
  })
  load()
}

async function load() {
  const repo = state.inspected
  if (!repo) {
    book = null
    render()
    return
  }
  book = rowOf(repo) ?? book
  render()
  if (!rowOf(repo)) {
    // Selected from outside the current page (pipeline strip, logs)
    try {
      book = await api.getBook(repo)
      render()
    } catch (err) {
      console.error('get_book', err)
    }
  }
}

function onClick(e) {
  if (handleToggleClick(e)) return
  const a = e.target.closest('[data-act]')
  if (!a || !book) return
  const repo = book.repo_name
  switch (a.dataset.act) {
    case 'close':
      state.inspectorOpen = false
      emit('inspector-visibility')
      break
    case 'read': openReader(repo); break
    case 'open': api.openEpub(repo).catch(err => alertInline(`Could not open: ${err}`)); break
    case 'reveal': api.revealEpub(repo).catch(err => alertInline(`Could not show the file: ${err}`)); break
    case 'url': api.openUrl(a.dataset.url); break
    case 'build': build([repo]); break
    case 'rebuild': build([repo], { force: true }); break
  }
}

function alertInline(text) {
  const box = $('#inspector [data-alert]')
  if (box) {
    box.textContent = text
    box.hidden = false
  }
}

function hostOf(src) {
  try {
    return new URL(src).hostname.replace(/^www\./, '')
  } catch {
    return src
  }
}

function link(url, label, cls = 'text-secondary') {
  return url
    ? `<button data-act="url" data-url="${esc(url)}" class="${cls} hover:underline inline-flex items-center gap-1 text-right">${esc(label)}${icon('open_in_new', 'text-[13px]')}</button>`
    : esc(label)
}

function render() {
  clearTimeout(logTimer)
  const el = $('#inspector')
  if (!book) {
    el.innerHTML = `
      <div class="flex items-center gap-space-xs pb-space-xs border-b border-surface-container-highest/50">
        ${icon('menu_book', 'text-primary text-[20px]')}<h2 class="font-display text-headline-sm font-bold">Curator Inspector</h2>
      </div>
      <p class="text-body-sm text-outline py-6 text-center">Select a book to see its details, build output and actions.</p>`
    return
  }

  const b = book
  const s = statusOf(b)
  renderedKey = s.key
  const [chipCls, chipLabel] = STATUS_CHIP[s.key] ?? STATUS_CHIP.not_built
  const chipText = s.key === 'building' ? `${chipLabel} · ${s.pct ?? 0}%` : s.key === 'queued' && s.pos ? `${chipLabel} · #${s.pos}` : chipLabel
  const desc = plainParagraphs(b.long_description || b.description)
  const short = b.abstract || ''
  const art = b.cover_art
  const artText = art?.title ? `${art.title}${art.year ? ` (${art.year})` : ''}${art.artist ? `, ${art.artist}` : ''}` : art?.artist || ''
  const last = b.last_build
  const rows = [
    ['Git repository', link(b.repo, `standardebooks/${b.repo_name}`, 'text-secondary font-mono text-[11px] break-all')],
    last?.sha && ['Commit', `<span class="text-primary font-mono">${esc(last.sha.slice(0, 7))}</span>`],
    b.categories?.length && ['Genres', esc(b.categories.join(' / '))],
    b.series && ['Series', esc(`${b.series}${b.series_position ? ` #${b.series_position}` : ''}`)],
    b.subjects?.length && ['Subjects', `<span title="${esc(b.subjects.join('\n'))}">${esc(b.subjects.slice(0, 3).map(x => x.replace(/--.*$/, '')).join(' / '))}${b.subjects.length > 3 ? ' …' : ''}</span>`],
    artText && ['Cover artwork', art.artist_url ? link(art.artist_url, artText, 'text-on-surface-variant') : esc(artText)],
    b.language && ['Language', esc(b.language)],
    b.se_published && ['SE release', esc(b.se_published.slice(0, 10))],
    b.wikipedia && ['Wikipedia', link(b.wikipedia, 'Article')],
    b.sources?.length && ['Sources', b.sources.map(src => link(/^https?:/.test(src) ? src : '', hostOf(src))).join('<br>')],
  ].filter(Boolean)

  const busy = s.key === 'building' || s.key === 'queued'
  const logStatus = last
    ? last.status === 'built'
      ? `<span class="text-[10px] text-tertiary font-mono bg-tertiary-container/20 px-1.5 py-0.5 rounded-lg">ok · ${(last.duration_ms / 1000).toFixed(1)}s · ${esc(dateTime(last.built_at))}</span>`
      : `<span class="text-[10px] text-error font-mono bg-error-container/30 px-1.5 py-0.5 rounded-lg">failed · ${esc(dateTime(last.built_at))}</span>`
    : ''

  el.innerHTML = `
    <div class="flex items-center justify-between gap-2 pb-space-xs border-b border-surface-container-highest/50">
      <div class="flex items-center gap-space-xs">${icon('menu_book', 'text-primary text-[20px]')}<h2 class="font-display text-headline-sm font-bold">Curator Inspector</h2></div>
      <div class="flex items-center gap-1 min-w-0">
        <span class="text-label-sm px-2 py-0.5 rounded-lg bg-surface-container text-secondary font-mono truncate max-w-40" title="${esc(b.repo_name)}">${esc(b.repo_name)}</span>
        <button data-act="close" class="p-0.5 text-outline hover:text-on-surface" title="Hide inspector">${icon('close', 'text-[18px]')}</button>
      </div>
    </div>

    <div class="flex flex-col gap-space-sm">
      <div class="flex gap-space-md items-start">
        <div class="relative w-24 shrink-0 aspect-[2/3] rounded-xl overflow-hidden shadow-xl bg-surface-container-highest">
          ${b.cover_path ? `<img class="w-full h-full object-cover" src="${esc(coverUrl(b.cover_path, state.coverBust))}" alt="">` : `<div class="w-full h-full cover-fallback"></div>`}
          <div class="absolute inset-y-0 left-0 w-2 bg-gradient-to-r from-black/50 to-transparent"></div>
          ${s.key === 'building' ? `<div class="absolute bottom-0 inset-x-0 h-1 bg-surface-container-lowest"><div data-pct-bar class="h-full bg-primary" style="width:${s.pct ?? 0}%"></div></div>` : ''}
        </div>
        <div class="flex flex-col min-w-0">
          <div data-chip class="inline-flex items-center gap-1 px-2 py-0.5 rounded-lg ${chipCls} text-[10px] uppercase font-bold w-fit mb-1">${esc(chipText)}</div>
          <h3 class="font-display text-headline-md font-bold text-on-surface leading-tight selectable">${esc(b.title)}</h3>
          <span class="text-body-md text-primary font-medium">${esc(b.author)}</span>
          <div class="mt-2 flex flex-wrap gap-1 text-label-sm text-outline">
            ${b.published ? `<span class="chip" title="Original publication">${esc(b.published.label)}</span>` : ''}
            ${b.word_count ? `<span class="chip">${num(b.word_count)} words</span>` : ''}
            ${b.reading_ease ? `<span class="chip text-tertiary" title="Flesch reading ease">Reading ease ${b.reading_ease.toFixed(1)}</span>` : ''}
            ${b.reading_time ? `<span class="chip">${esc(b.reading_time)}</span>` : ''}
          </div>
        </div>
      </div>
      ${short ? `<p class="text-body-sm text-on-surface italic selectable">${esc(short)}</p>` : ''}
      ${desc.length ? `<div class="text-body-sm text-on-surface-variant leading-relaxed flex flex-col gap-2 max-h-48 overflow-y-auto pr-1 selectable">${desc.map(p => `<p>${esc(p)}</p>`).join('')}</div>`
        : `<p class="text-body-sm text-outline">No description yet — run Enrich Metadata, or build this book with Metadata on.</p>`}
    </div>

    <div class="bg-surface-container p-space-sm rounded-xl flex flex-col text-[12px]">
      ${rows.map(([k, v]) => `<div class="kv-row"><span class="kv-key">${k}</span><span class="kv-val">${v}</span></div>`).join('')}
    </div>

    <div class="flex flex-col gap-1.5">
      <div class="flex items-center justify-between gap-2">
        <span class="eyebrow flex items-center gap-1.5">${icon('terminal', 'text-[14px] text-tertiary')}Build output</span>
        ${logStatus}
      </div>
      <pre id="build-log" class="w-full bg-surface-container-lowest p-space-sm rounded-xl font-mono text-[11px] text-secondary/90 leading-relaxed shadow-inner overflow-auto h-32 whitespace-pre-wrap selectable">${last || busy ? 'Loading…' : 'This book has not been built by this app yet.'}</pre>
    </div>

    <div data-alert class="text-body-sm text-error" hidden></div>

    <div class="flex flex-col gap-2 pt-space-xs">
      ${b.downloaded ? `
        <div class="grid grid-cols-2 gap-2">
          <button data-act="read" class="py-2 px-space-sm rounded-lg bg-surface-container hover:bg-surface-container-high text-label-sm font-semibold flex items-center justify-center gap-1.5">${icon('menu_book', 'text-[16px] text-primary')}Read</button>
          <button data-act="open" class="py-2 px-space-sm rounded-lg bg-surface-container hover:bg-surface-container-high text-label-sm font-semibold flex items-center justify-center gap-1.5">${icon('open_in_new', 'text-[16px] text-secondary')}System reader</button>
        </div>
        <div class="grid grid-cols-2 gap-2">
          <button data-act="reveal" class="py-2 px-space-sm rounded-lg bg-surface-container hover:bg-surface-container-high text-label-sm font-semibold flex items-center justify-center gap-1.5">${icon('folder_open', 'text-[16px] text-tertiary')}Show in folder</button>
          <button data-act="url" data-url="${esc(b.url)}" class="py-2 px-space-sm rounded-lg bg-surface-container hover:bg-surface-container-high text-label-sm font-semibold flex items-center justify-center gap-1.5">${icon('language', 'text-[16px] text-outline')}SE page</button>
        </div>
        <div class="text-[11px] text-outline text-center">EPUB 3 · ${bytes(b.epub_size)}</div>` : `
        <button data-act="url" data-url="${esc(b.url)}" class="py-2 px-space-sm rounded-lg bg-surface-container hover:bg-surface-container-high text-label-sm font-semibold flex items-center justify-center gap-1.5">${icon('language', 'text-[16px] text-outline')}Standard Ebooks page</button>`}
      <div class="flex items-center justify-center gap-space-md">${toggleHtml('build_metadata', 'Metadata')}${toggleHtml('build_covers', 'Cover art')}</div>
      <button data-act="${b.downloaded ? 'rebuild' : 'build'}" ${busy ? 'disabled' : ''}
        class="w-full py-2.5 rounded-lg bg-primary hover:bg-primary-fixed-dim text-on-primary text-label-sm font-bold flex items-center justify-center gap-2 shadow-lg shadow-primary/10 transition-transform active:scale-95">
        ${icon(busy ? 'hourglass_top' : b.downloaded ? 'cached' : 'download', 'text-[18px]')}
        <span data-build-label>${busy ? (s.key === 'building' ? `Building… ${s.pct ?? 0}%` : 'Queued') : b.downloaded ? 'Rebuild Clean Edition' : 'Build EPUB'}</span>
      </button>
    </div>`

  if (last || busy) loadLog(b.repo_name, busy)
}

function patchProgress() {
  const s = statusOf(book)
  const pct = s.pct ?? 0
  const bar = $('#inspector [data-pct-bar]')
  if (bar) bar.style.width = `${pct}%`
  const chip = $('#inspector [data-chip]')
  if (chip) chip.textContent = `${STATUS_CHIP.building[1]} · ${pct}%`
  const label = $('#inspector [data-build-label]')
  if (label) label.textContent = `Building… ${pct}%`
}

async function loadLog(repo, tail) {
  try {
    const text = await api.buildLog(repo)
    if (book?.repo_name !== repo) return
    const pre = $('#build-log')
    if (!pre) return
    const atBottom = pre.scrollTop + pre.clientHeight >= pre.scrollHeight - 4
    pre.innerHTML = text ? colorLog(text) : '<span class="text-outline">No log yet.</span>'
    if (atBottom || tail) pre.scrollTop = pre.scrollHeight
  } catch (err) {
    console.error('build log', err)
  }
  if (tail) logTimer = setTimeout(() => loadLog(repo, statusOf(book ?? {}).key === 'building'), 1000)
}

function colorLog(text) {
  return text.split('\n').map(line => {
    const cls = /failed|error|fatal/i.test(line) ? 'text-error'
      : /succeeded|Packaged/.test(line) ? 'text-tertiary'
        : /^\[[^\]]*\] \$/.test(line) ? 'text-outline'
          : /HEAD|Fetching/.test(line) ? 'text-primary'
            : 'text-on-surface-variant'
    return `<span class="${cls}">${esc(line)}</span>`
  }).join('\n')
}
