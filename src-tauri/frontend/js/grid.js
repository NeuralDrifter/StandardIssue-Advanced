// Michael Burgus — https://github.com/NeuralDrifter
// The shelf: status tabs, sort, grid / compact / list views, cards, pagination

import { state, on, emit } from './state.js'
import { setQuery, inspect, openReader, build, toggleSelected } from './actions.js'
import { setView } from './toolbar.js'
import { $, $$, esc, num, kWords, bytes, icon, coverUrl } from './util.js'

const TABS = [
  { status: 'all', label: 'All', dot: '' },
  { status: 'built', label: 'Built', dot: 'bg-tertiary' },
  { status: 'pipeline', label: 'In Pipeline', dot: 'bg-primary animate-pulse' },
  { status: 'not_built', label: 'Not Built', dot: 'bg-secondary' },
  { status: 'failed', label: 'Failed', dot: 'bg-error', hideEmpty: true },
  { status: 'reading', label: 'Reading', dot: 'bg-primary-fixed', hideEmpty: true },
]

const SORTS = [
  ['title', 'Title (A–Z)'],
  ['author', 'Author (A–Z)'],
  ['year', 'Publication year'],
  ['words', 'Word count (longest)'],
  ['recent_build', 'Recently built'],
  ['newest', 'Newest Standard Ebooks releases'],
]

const VIEWS = [['grid', 'grid_view', 'Cover grid'], ['compact', 'view_comfy', 'Compact covers'], ['list', 'table_rows', 'Archival table']]

let lastClicked = null

export function initGrid() {
  $('#tabs').addEventListener('click', e => {
    const tab = e.target.closest('[data-tab]')
    if (tab) setQuery({ status: tab.dataset.tab })
    const v = e.target.closest('[data-view]')
    if (v) setView(v.dataset.view)
  })
  $('#tabs').addEventListener('change', e => {
    if (e.target.id === 'sort') setQuery({ sort: e.target.value })
  })

  const shelf = $('#shelf')
  shelf.addEventListener('click', onShelfClick)
  shelf.addEventListener('dblclick', e => {
    const card = e.target.closest('[data-repo]')
    if (card && !e.target.closest('button, input') && rowOf(card.dataset.repo)?.downloaded) openReader(card.dataset.repo)
  })

  on(['result', 'query', 'settings'], renderTabs)
  on(['result', 'settings', 'reading'], renderShelf)
  on(['inspected', 'selection'], renderMarks)
  on('inspector-visibility', layout)
  renderTabs()
  renderShelf()
  layout()
}

export function rowOf(repo) {
  return state.result?.books.find(b => b.repo_name === repo)
}

function layout() {
  const shelf = $('#shelf')
  const open = state.inspectorOpen
  shelf.className = `col-span-12 ${open ? 'xl:col-span-8 2xl:col-span-9' : ''} flex flex-col gap-space-lg`
  $('#inspector').hidden = !open
}

function onShelfClick(e) {
  const pageBtn = e.target.closest('[data-page]')
  if (pageBtn) {
    setQuery({ page: Number(pageBtn.dataset.page) }, { resetPage: false })
    $('#main').scrollTo({ top: 0 })
    return
  }
  const card = e.target.closest('[data-repo]')
  if (!card) return
  const repo = card.dataset.repo
  const action = e.target.closest('[data-action]')?.dataset.action

  if (action === 'select' || e.target.closest('[data-select]')) {
    e.stopPropagation()
    const on = !state.selection.has(repo)
    if (e.shiftKey && lastClicked) {
      const repos = state.result.books.map(b => b.repo_name)
      const [a, b] = [repos.indexOf(lastClicked), repos.indexOf(repo)].sort((x, y) => x - y)
      for (const r of repos.slice(a, b + 1)) toggleSelected(r, on)
    } else {
      toggleSelected(repo, on)
    }
    lastClicked = repo
    return
  }
  if (action === 'read') return openReader(repo)
  if (action === 'build') return build([repo])
  inspect(repo)
}

// --- Effective status: live pipeline state wins over the last query ---

export function statusOf(row) {
  const live = state.live[row.repo_name]
  if (live?.state === 'building') return { key: 'building', pct: live.pct, phase: live.phase }
  if (live?.state === 'queued') return { key: 'queued', pos: live.queue_pos }
  if (live?.state === 'built') return { key: 'built' }
  if (live?.state === 'failed') return { key: 'failed', error: live.error }
  return { key: row.status }
}

const BADGES = {
  built: ['border-tertiary/40', 'bg-tertiary', 'text-tertiary', () => 'Built'],
  building: ['border-primary/50', 'bg-primary animate-ping', 'text-primary', s => `Building ${s.pct ?? 0}%`],
  queued: ['border-outline/30', 'bg-on-surface-variant', 'text-on-surface-variant', s => (s.pos ? `Queue #${s.pos}` : 'Queued')],
  failed: ['border-error/50', 'bg-error', 'text-error', () => 'Failed'],
  not_built: ['border-outline/20', 'bg-outline', 'text-outline', () => 'Not built'],
}

export function badgeHtml(s, extra = '') {
  const [border, dot, text, label] = BADGES[s.key] ?? BADGES.not_built
  return `<div class="badge ${border} ${extra}"><span class="dot ${dot}"></span><span class="${text}">${esc(label(s))}</span></div>`
}

function coverHtml(b, compact) {
  const url = coverUrl(b.cover_path, state.coverBust)
  return url
    ? `<img class="w-full h-full object-cover" src="${esc(url)}" alt="" loading="lazy" draggable="false">`
    : `<div class="w-full h-full cover-fallback flex flex-col items-center justify-center p-3 text-center gap-1">
         <span class="font-display ${compact ? 'text-body-sm' : 'text-headline-sm'} text-on-surface-variant leading-tight line-clamp-4">${esc(b.title)}</span>
         ${compact ? '' : `<span class="text-[10px] uppercase tracking-wider text-outline">${esc(b.author)}</span>`}
       </div>`
}

function readBar(repo) {
  const p = state.reading[repo]
  if (!p || p.fraction <= 0) return ''
  return `<div class="absolute top-0 inset-x-0 h-1 bg-black/50" title="${Math.round(p.fraction * 100)}% read"><div class="h-full bg-primary-fixed" style="width:${(p.fraction * 100).toFixed(1)}%"></div></div>`
}

function cardHtml(b, compact) {
  const s = statusOf(b)
  const building = s.key === 'building'
  const selected = state.selection.has(b.repo_name)
  const inspected = state.inspected === b.repo_name
  const meta = (() => {
    if (building) return `<span class="text-primary truncate">${esc(s.phase || 'Building')}…</span>`
    if (s.key === 'queued') return `<span class="text-on-surface-variant">Waiting to build</span>`
    if (s.key === 'failed') return `<span class="text-error">Build failed</span>`
    if (b.downloaded) return `<span>EPUB · ${bytes(b.epub_size)}</span>`
    return `<span class="text-outline">Not built</span>`
  })()
  const primary = b.downloaded
    ? `<button data-action="read" class="w-full py-1.5 rounded-lg bg-primary text-on-primary text-label-sm font-bold flex items-center justify-center gap-1 shadow-md hover:bg-primary-fixed-dim">${icon('menu_book', 'text-[16px]')} Open Reader</button>`
    : s.key === 'building' || s.key === 'queued'
      ? ''
      : `<button data-action="build" class="w-full py-1.5 rounded-lg bg-primary text-on-primary text-label-sm font-bold flex items-center justify-center gap-1 shadow-md hover:bg-primary-fixed-dim">${icon('download', 'text-[16px]')} Build EPUB</button>`

  return `
  <article data-repo="${esc(b.repo_name)}" class="group relative flex flex-col rounded-xl ${inspected ? 'bg-surface-container-high ring-1 ring-primary/40 shadow-xl' : 'bg-surface-container-low hover:bg-surface-container shadow-md hover:shadow-2xl'} transition-all duration-300 ${compact ? 'p-1.5' : 'p-2.5'} cursor-pointer">
    <div class="relative w-full aspect-[2/3] rounded-xl overflow-hidden bg-surface-container-highest shelf-shadow group-hover:scale-[1.02] transition-transform duration-300">
      ${coverHtml(b, compact)}
      <div class="cover-spine"></div>
      ${readBar(b.repo_name)}
      ${badgeHtml(s, compact ? 'scale-90 origin-top-right' : '')}
      ${compact ? '' : `<div class="absolute inset-0 bg-surface-container-lowest/80 backdrop-blur-sm opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex flex-col justify-end p-2.5 gap-1.5">
        ${primary}
        <button data-action="inspect" class="w-full py-1.5 rounded-lg bg-surface-container-high text-on-surface text-label-sm flex items-center justify-center gap-1 hover:bg-surface-bright">${icon('info', 'text-[16px]')} Inspect</button>
      </div>`}
      <button data-select title="Select for Build Selected (Shift-click for a range)"
        class="absolute top-2 left-2 w-5 h-5 rounded-lg border ${selected ? 'bg-primary border-primary text-on-primary opacity-100' : 'bg-surface-dim/80 border-outline/50 text-transparent opacity-0 group-hover:opacity-100'} backdrop-blur-md grid place-items-center transition-opacity z-10">
        ${icon('check', 'text-[14px]')}
      </button>
      ${building ? `<div class="absolute bottom-0 inset-x-0 h-1.5 bg-surface-container-lowest"><div class="h-full bg-primary transition-all duration-300" style="width:${s.pct ?? 0}%"></div></div>` : ''}
    </div>
    ${compact
      ? `<div class="mt-1.5 text-body-sm font-display font-semibold truncate ${building ? 'text-primary' : 'text-on-surface'}">${esc(b.title)}</div>`
      : `<div class="mt-2.5 flex flex-col min-w-0">
          <div class="flex items-center justify-between gap-1">
            <h3 class="font-display text-headline-sm font-semibold truncate ${building ? 'text-primary' : 'text-on-surface group-hover:text-primary'} transition-colors" title="${esc(b.title)}">${esc(b.title)}</h3>
            ${building ? icon('sync', 'text-primary text-[16px] animate-spin shrink-0') : ''}
          </div>
          <span class="text-body-sm text-outline truncate">${esc(b.author)}</span>
          <div class="mt-1 flex items-center justify-between gap-2 text-[11px] ${building ? 'text-primary' : 'text-secondary'}">
            ${meta}<span class="text-outline shrink-0">${kWords(b.word_count)}</span>
          </div>
        </div>`}
  </article>`
}

function listRowHtml(b) {
  const s = statusOf(b)
  const inspected = state.inspected === b.repo_name
  const selected = state.selection.has(b.repo_name)
  return `
  <tr data-repo="${esc(b.repo_name)}" class="cursor-pointer border-b border-surface-container-highest/40 ${inspected ? 'bg-surface-container-high' : 'hover:bg-surface-container'}">
    <td class="py-1.5 pl-2 w-8"><button data-select class="w-4 h-4 rounded-lg border ${selected ? 'bg-primary border-primary text-on-primary' : 'border-outline/50 text-transparent'} grid place-items-center">${icon('check', 'text-[12px]')}</button></td>
    <td class="py-1.5 w-10"><div class="w-7 aspect-[2/3] rounded-lg overflow-hidden bg-surface-container-highest">${b.cover_path ? `<img class="w-full h-full object-cover" src="${esc(coverUrl(b.cover_path, state.coverBust))}" alt="" loading="lazy">` : ''}</div></td>
    <td class="py-1.5 pr-3"><div class="font-display text-body-md font-semibold text-on-surface truncate max-w-80">${esc(b.title)}</div><div class="text-body-sm text-outline truncate max-w-80">${esc(b.author)}</div></td>
    <td class="py-1.5 pr-3 text-body-sm text-on-surface-variant whitespace-nowrap">${esc(b.published?.label ?? '—')}</td>
    <td class="py-1.5 pr-3 text-body-sm text-on-surface-variant truncate max-w-40">${esc(b.categories?.join(', ') || '—')}</td>
    <td class="py-1.5 pr-3 text-body-sm text-outline whitespace-nowrap text-right">${b.word_count ? num(b.word_count) : '—'}</td>
    <td class="py-1.5 pr-3 whitespace-nowrap"><div class="relative h-6 w-32">${badgeHtml(s, '!static !inline-flex')}</div></td>
    <td class="py-1.5 pr-2 text-body-sm text-outline whitespace-nowrap text-right">${b.downloaded ? bytes(b.epub_size) : ''}</td>
  </tr>`
}

function renderTabs() {
  const counts = Object.fromEntries((state.result?.facets?.statuses ?? []).map(f => [f.value, f.count]))
  const view = state.settings?.view ?? 'grid'
  $('#tabs').innerHTML = `
    <div class="flex flex-col md:flex-row md:items-center justify-between gap-space-sm bg-surface-container-low px-space-md py-space-sm rounded-xl">
      <div class="flex items-center gap-1.5 overflow-x-auto pb-1 md:pb-0">
        ${TABS.filter(t => !t.hideEmpty || counts[t.status] || state.query.status === t.status).map(t => {
          const active = state.query.status === t.status
          return `<button data-tab="${t.status}" class="px-space-md py-1 rounded-full text-label-sm whitespace-nowrap transition-colors flex items-center gap-1.5 ${active ? 'bg-primary-container text-on-primary-container font-bold shadow-sm' : 'bg-surface-container text-on-surface-variant hover:text-on-surface hover:bg-surface-container-high'}">
            ${t.dot && !active ? `<span class="dot ${t.dot}"></span>` : ''}${t.label} (${num(counts[t.status] ?? 0)})</button>`
        }).join('')}
      </div>
      <div class="flex items-center gap-space-sm self-end md:self-auto">
        <label class="flex items-center gap-1 bg-surface-container px-2 py-1 rounded-lg">
          <span class="text-label-sm text-outline">Sort:</span>
          <select id="sort" class="bg-transparent text-label-sm text-on-surface font-semibold focus:outline-none cursor-pointer">
            ${SORTS.map(([v, l]) => `<option class="bg-surface-container" value="${v}" ${state.query.sort === v ? 'selected' : ''}>${l}</option>`).join('')}
          </select>
        </label>
        <div class="flex items-center bg-surface-container p-0.5 rounded-lg">
          ${VIEWS.map(([v, i, t]) => `<button data-view="${v}" title="${t}" class="p-1 rounded-lg flex items-center ${view === v ? 'bg-surface-container-high text-primary' : 'text-outline hover:text-on-surface'}">${icon(i, 'text-[18px]')}</button>`).join('')}
        </div>
      </div>
    </div>`
}

function renderShelf() {
  const r = state.result
  const shelf = $('#shelf')
  if (!r) {
    shelf.innerHTML = `<div class="py-24 text-center text-outline">Loading the catalog…</div>`
    return
  }
  if (!r.books.length) {
    const empty = state.overview?.total === 0
    shelf.innerHTML = `
      <div class="py-24 flex flex-col items-center gap-3 text-center">
        ${icon(empty ? 'cloud_sync' : 'search_off', 'text-[40px] text-outline')}
        <div class="font-display text-headline-md">${empty ? 'The catalog is empty' : 'No books match'}</div>
        <div class="text-body-sm text-outline">${empty ? 'Run <b>Sync GitHub</b> to fetch the Standard Ebooks catalog.' : 'Try clearing a filter or changing the search.'}</div>
      </div>`
    return
  }
  const view = state.settings?.view ?? 'grid'
  const body = view === 'list'
    ? `<div class="bg-surface-container-low rounded-xl overflow-hidden"><table class="w-full">
         <thead><tr class="text-left eyebrow border-b border-surface-container-highest">
           <th class="py-2 pl-2"></th><th></th><th class="py-2">Title</th><th>Published</th><th>Genre</th><th class="text-right pr-3">Words</th><th>Status</th><th class="text-right pr-2">Size</th>
         </tr></thead>
         <tbody>${r.books.map(listRowHtml).join('')}</tbody></table></div>`
    : `<div class="grid ${view === 'compact' ? 'grid-cols-[repeat(auto-fill,minmax(112px,1fr))] gap-gutter-compact' : 'grid-cols-[repeat(auto-fill,minmax(158px,1fr))] gap-gutter'}">
         ${r.books.map(b => cardHtml(b, view === 'compact')).join('')}</div>`
  shelf.innerHTML = body + paginationHtml(r)
}

/** Re-render just one card/row (live build updates). */
export function patchBook(repo) {
  const row = rowOf(repo)
  const el = $(`#shelf [data-repo="${CSS.escape(repo)}"]`)
  if (!row || !el) return
  const view = state.settings?.view ?? 'grid'
  const tpl = document.createElement(view === 'list' ? 'tbody' : 'div')
  tpl.innerHTML = view === 'list' ? listRowHtml(row) : cardHtml(row, view === 'compact')
  el.replaceWith(tpl.firstElementChild)
}

function renderMarks() {
  // Inspected highlight and checkboxes change without new data: re-render affected items only
  for (const el of $$('#shelf [data-repo]')) patchBook(el.dataset.repo)
}

function paginationHtml(r) {
  const from = (r.page - 1) * r.per_page + 1
  const to = Math.min(r.total, r.page * r.per_page)
  const pages = pageList(r.page, r.pages)
  const btn = (p, label = p) => `<button data-page="${p}" class="px-2.5 py-1 rounded-lg bg-surface-container text-on-surface-variant hover:text-on-surface transition-colors">${label}</button>`
  return `
    <div class="flex items-center justify-between py-space-sm px-space-md bg-surface-container-low rounded-xl">
      <span class="text-body-sm text-outline">Showing <span class="text-on-surface font-semibold">${num(from)} – ${num(to)}</span> of ${num(r.total)} Standard Ebooks editions</span>
      <div class="flex items-center gap-1 text-label-sm">
        ${r.page > 1 ? btn(r.page - 1, 'Prev') : ''}
        ${pages.map(p => p === '…' ? '<span class="px-1 text-outline">…</span>'
          : p === r.page ? `<span class="px-3 py-1 rounded-lg bg-primary-container text-on-primary-container font-bold">${p}</span>` : btn(p)).join('')}
        ${r.page < r.pages ? btn(r.page + 1, 'Next') : ''}
      </div>
    </div>`
}

/** 1 … 4 5 [6] 7 8 … 64 */
function pageList(page, pages) {
  const set = new Set([1, pages, page - 1, page, page + 1, page < 4 ? 2 : 0, page < 4 ? 3 : 0, page > pages - 3 ? pages - 1 : 0, page > pages - 3 ? pages - 2 : 0])
  const list = [...set].filter(p => p >= 1 && p <= pages).sort((a, b) => a - b)
  const out = []
  list.forEach((p, i) => {
    if (i && p - list[i - 1] > 1) out.push('…')
    out.push(p)
  })
  return out
}

export function onInspectorVisibility() {
  emit('inspector-visibility')
}
