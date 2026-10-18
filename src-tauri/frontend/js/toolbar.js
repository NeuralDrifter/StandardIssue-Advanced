// Michael Burgus — https://github.com/NeuralDrifter
// Command toolbar: search, facet filters, catalog count, operations, view toggle

import { state, on, emit } from './state.js'
import { setQuery, setRoute, runOp, buildAll, setBuildToggle } from './actions.js'
import { $, $$, esc, num, icon, debounce, dismissable } from './util.js'
import { api } from './api.js'

const STATUS_LABELS = {
  all: 'All', built: 'Built', not_built: 'Not built', pipeline: 'In pipeline',
  queued: 'Queued', building: 'Building', failed: 'Failed', reading: 'Currently reading',
}

const FILTERS = [
  { key: 'author', label: 'Author', facet: 'authors', searchable: true },
  { key: 'genre', label: 'Genre', facet: 'genres', searchable: true },
  { key: 'era', label: 'Era', facet: 'eras' },
  { key: 'status', label: 'Build Status', facet: 'statuses' },
]

export function initToolbar() {
  $('#toolbar').innerHTML = `
    <div class="h-16 w-full px-margin flex items-center justify-between gap-gutter">
      <div class="flex items-center gap-space-md flex-1 max-w-3xl min-w-0">
        <div class="relative w-full min-w-48 max-w-md">
          ${icon('search', 'absolute left-3 top-1/2 -translate-y-1/2 text-[18px] text-outline')}
          <input id="search" type="text" spellcheck="false" autocomplete="off"
                 class="field pl-9 pr-16" placeholder="Search title, author, genre or subject…">
          <span class="absolute right-2.5 top-1/2 -translate-y-1/2 text-label-sm text-outline bg-surface-container-high px-1.5 py-0.5 rounded-lg pointer-events-none">Ctrl K</span>
        </div>
        <div id="filter-buttons" class="hidden xl:flex items-center gap-1.5"></div>
      </div>
      <div class="flex items-center gap-space-md shrink-0">
        <div id="ready-chip" class="hidden lg:flex items-center gap-space-xs text-label-sm px-space-sm py-1 rounded-lg bg-surface-container"></div>
        <div class="flex items-center gap-space-xs">
          <button class="btn-glass" data-op="sync">${icon('sync', 'text-[16px] text-primary')}<span>Sync Catalog</span></button>
          <button class="btn-glass" data-op="enrich">${icon('auto_fix_normal', 'text-[16px] text-secondary')}<span>Enrich Metadata</span></button>
          <button class="btn-gold" id="build-all">${icon('bolt', 'text-[16px]')}<span>Build All</span></button>
        </div>
        <div id="view-toggle" class="flex items-center bg-surface-container p-0.5 rounded-lg"></div>
      </div>
    </div>
    <div class="w-full h-0.5 bg-surface-container-highest relative overflow-hidden">
      <div id="progress-line" class="h-full w-0 bg-gradient-to-r from-primary to-tertiary transition-[width] duration-300"></div>
    </div>`

  const search = $('#search')
  const apply = debounce(() => {
    setRoute('library')
    setQuery({ search: search.value.trim() })
  }, 250)
  search.addEventListener('input', apply)
  search.addEventListener('keydown', e => {
    if (e.key === 'Escape' && search.value) {
      search.value = ''
      apply()
    }
  })
  document.addEventListener('keydown', e => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault()
      search.focus()
      search.select()
    }
  })

  $('#toolbar').addEventListener('click', e => {
    const op = e.target.closest('[data-op]')
    if (op) runOp(op.dataset.op)
    if (e.target.closest('#build-all')) buildAll()
    const f = e.target.closest('[data-filter]')
    if (f) openFilterMenu(f, FILTERS.find(x => x.key === f.dataset.filter))
    const v = e.target.closest('[data-view]')
    if (v) setView(v.dataset.view)
  })

  $('#active-filters').addEventListener('click', e => {
    const chip = e.target.closest('[data-clear]')
    if (!chip) return
    if (chip.dataset.clear === 'search') search.value = ''
    setQuery({ [chip.dataset.clear]: chip.dataset.clear === 'status' ? 'all' : '' })
  })

  on(['result', 'query'], renderFilters)
  on(['overview', 'queue'], renderReadyChip)
  on(['op', 'queue', 'queue-progress'], renderProgressLine)
  on(['op', 'queue'], renderOpButtons)
  on('settings', renderViewToggle)
  renderFilters()
  renderReadyChip()
  renderViewToggle()
}

export async function setView(view) {
  state.settings = await api.saveSettings({ ...state.settings, view })
  setRoute('library')
  emit('settings', 'result')
}

function renderViewToggle() {
  const current = state.settings?.view ?? 'grid'
  $('#view-toggle').innerHTML = [['grid', 'grid_view', 'Cover grid'], ['list', 'view_list', 'Archival list']]
    .map(([v, i, t]) => {
      const on = current === v || (v === 'grid' && current === 'compact')
      return `<button data-view="${v}" title="${t}" class="p-1 rounded-lg flex items-center justify-center ${on ? 'bg-surface-container-high text-primary' : 'text-outline hover:text-on-surface'}">${icon(i, 'text-[18px]')}</button>`
    }).join('')
}

function renderFilters() {
  $('#filter-buttons').innerHTML = FILTERS.map(f => {
    const value = state.query[f.key]
    const active = f.key === 'status' ? value !== 'all' : !!value
    const label = active ? (f.key === 'status' ? STATUS_LABELS[value] : value) : f.label
    return `<button data-filter="${f.key}" class="px-2 py-1 rounded-lg ${active ? 'bg-primary/15 text-primary' : 'bg-surface-container text-on-surface-variant hover:text-on-surface hover:bg-surface-container-high'} text-label-sm flex items-center gap-1 transition-colors max-w-44">
      <span class="truncate">${esc(label)}</span>${icon('expand_more', 'text-[14px]')}</button>`
  }).join('')

  const q = state.query
  const chips = [
    q.search && ['search', `“${q.search}”`],
    q.author && ['author', q.author],
    q.genre && ['genre', q.genre],
    q.era && ['era', q.era],
    q.status !== 'all' && ['status', STATUS_LABELS[q.status]],
  ].filter(Boolean)
  const bar = $('#active-filters')
  bar.hidden = !chips.length
  bar.innerHTML = chips.map(([key, label]) => `
    <button data-clear="${key}" class="flex items-center gap-1 pl-2.5 pr-1.5 py-0.5 rounded-full bg-surface-container-high text-label-sm text-on-surface-variant hover:text-on-surface">
      ${esc(label)}${icon('close', 'text-[14px]')}
    </button>`).join('') +
    (chips.length > 1 ? `<button data-clear-all class="px-2 py-0.5 text-label-sm text-outline hover:text-primary">Clear all</button>` : '')
  bar.querySelector('[data-clear-all]')?.addEventListener('click', () => {
    $('#search').value = ''
    setQuery({ search: '', author: '', genre: '', era: '', status: 'all' })
  })
}

function openFilterMenu(button, filter) {
  document.querySelector('.menu')?.remove()
  const facets = state.result?.facets?.[filter.facet] ?? []
  const current = state.query[filter.key]
  const menu = document.createElement('div')
  menu.className = 'menu'
  const rect = button.getBoundingClientRect()
  Object.assign(menu.style, { position: 'fixed', left: `${rect.left}px`, top: `${rect.bottom}px` })

  const allValue = filter.key === 'status' ? 'all' : ''
  const items = filter.key === 'status'
    ? facets.map(f => ({ value: f.value, label: STATUS_LABELS[f.value], count: f.count }))
    : [{ value: '', label: `All ${filter.label.toLowerCase()}s`, count: null }, ...facets.map(f => ({ value: f.value, label: f.value, count: f.count }))]

  menu.innerHTML = `
    ${filter.searchable ? `<input class="field mb-1" placeholder="Filter ${filter.label.toLowerCase()}s…">` : ''}
    <div data-list></div>`
  const list = menu.querySelector('[data-list]')
  const renderList = (needle = '') => {
    const n = needle.toLowerCase()
    const shown = items.filter(i => !n || i.value === allValue || i.label.toLowerCase().includes(n)).slice(0, 300)
    list.innerHTML = shown.map(i => `
      <button class="menu-item" data-value="${esc(i.value)}" aria-selected="${i.value === current}">
        <span class="truncate">${esc(i.label)}</span>
        ${i.count == null ? '' : `<span class="text-label-sm text-outline">${num(i.count)}</span>`}
      </button>`).join('') || '<div class="px-2 py-1.5 text-body-sm text-outline">No matches</div>'
  }
  renderList()
  document.body.append(menu)
  const close = dismissable(menu, () => menu.remove())
  const input = menu.querySelector('input')
  input?.addEventListener('input', () => renderList(input.value))
  input?.focus()
  list.addEventListener('click', e => {
    const item = e.target.closest('[data-value]')
    if (!item) return
    close()
    setRoute('library')
    setQuery({ [filter.key]: item.dataset.value })
  })
}

function renderReadyChip() {
  const o = state.overview
  $('#ready-chip').innerHTML = o
    ? `<span class="text-tertiary font-semibold">${num(o.built)}</span><span class="text-outline">/</span><span class="text-on-surface-variant">${num(o.total)} Ebooks Ready</span>`
    : ''
}

function renderProgressLine() {
  const line = $('#progress-line')
  let pct = 0
  if (state.op?.total) {
    pct = (state.op.current / state.op.total) * 100
  } else if (state.op) {
    pct = 100 // indeterminate (sync): full shimmering line
  } else if (state.queue.running) {
    const q = state.queue
    const total = q.done + q.failed + q.active + q.queued
    const partial = q.active_list.reduce((s, a) => s + a.pct / 100, 0)
    pct = total ? ((q.done + q.failed + partial) / total) * 100 : 0
  }
  line.style.width = `${pct}%`
  line.classList.toggle('opacity-60', !!state.op && !state.op.total)
}

function renderOpButtons() {
  for (const b of $$('#toolbar [data-op]')) b.disabled = !!state.op
}

// Build toggles live in the hero and inspector; exported for reuse
export function toggleHtml(key, label) {
  const on = !!state.settings?.[key]
  return `<button type="button" role="switch" aria-checked="${on}" data-toggle="${key}" class="flex items-center gap-1.5 text-label-sm text-on-surface-variant hover:text-on-surface">
    <span class="toggle" aria-checked="${on}"></span>${label}</button>`
}

export function handleToggleClick(e) {
  const t = e.target.closest('[data-toggle]')
  if (!t) return false
  setBuildToggle(t.dataset.toggle, !state.settings?.[t.dataset.toggle])
  return true
}
