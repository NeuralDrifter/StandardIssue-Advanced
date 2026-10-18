// Michael Burgus — https://github.com/NeuralDrifter
// Sidebar: catalog shelves, continue reading, operations nav, local library meter

import { state, on } from './state.js'
import { setQuery, setRoute, openReader } from './actions.js'
import { $, esc, num, bytes, icon, coverUrl } from './util.js'

const SHELVES = [
  { status: 'all', label: 'All Ebooks', icon: 'library_books', count: o => o.total, tone: 'text-secondary' },
  { status: 'built', label: 'Downloaded', icon: 'cloud_done', count: o => o.built, tone: 'text-tertiary' },
  { status: 'building', label: 'Building', icon: 'precision_manufacturing', count: o => o.queue.active, tone: 'text-primary', pulse: true },
  { status: 'queued', label: 'Queue', icon: 'pending_actions', count: o => o.queue.queued, tone: 'text-on-surface-variant' },
  { status: 'reading', label: 'Currently Reading', icon: 'auto_stories', count: o => o.reading, tone: 'text-on-surface-variant' },
]

const PAGES = [
  { route: 'sync', label: 'GitHub Sync Status', icon: 'sync_alt' },
  { route: 'enrich', label: 'Metadata Enrichment', icon: 'auto_fix_high' },
  { route: 'logs', label: 'Archival Build Logs', icon: 'receipt_long' },
  { route: 'settings', label: 'Settings', icon: 'settings' },
]

const ACTIVE = 'bg-primary-container text-on-primary-container font-semibold'
const IDLE = 'text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface'

export function initSidebar() {
  const el = $('#sidebar')
  el.innerHTML = `
    <div class="flex flex-col gap-space-md px-space-md">
      <div class="flex items-center gap-space-sm pb-space-xs">
        <div class="w-7 h-7 rounded-lg bg-primary-container flex items-center justify-center text-on-primary-container">
          ${icon('menu_book', 'text-[18px]')}
        </div>
        <div>
          <div class="font-display text-headline-sm font-semibold text-on-surface leading-tight">StandardIssue</div>
          <div class="text-label-sm text-on-surface-variant">Advanced Edition</div>
        </div>
      </div>
      <div class="flex flex-col gap-1">
        <div class="eyebrow px-space-xs pt-space-xs">Catalog Shelves</div>
        <nav id="shelves" class="flex flex-col gap-0.5"></nav>
      </div>
      <div id="continue" hidden></div>
      <div class="flex flex-col gap-1">
        <div class="eyebrow px-space-xs pt-space-xs">Operations &amp; Pipeline</div>
        <nav id="pages-nav" class="flex flex-col gap-0.5"></nav>
      </div>
    </div>
    <div class="px-space-md pt-space-sm">
      <div id="disk" class="p-space-sm rounded-xl bg-surface-container flex flex-col gap-1.5"></div>
    </div>`

  el.addEventListener('click', e => {
    const shelf = e.target.closest('[data-shelf]')
    if (shelf) {
      setRoute('library')
      setQuery({ status: shelf.dataset.shelf })
      return
    }
    // [data-route] links are handled globally in main.js
    const cont = e.target.closest('[data-continue]')
    if (cont) openReader(cont.dataset.continue)
  })

  on(['overview', 'query', 'route', 'queue'], renderNav)
  on('reading', renderContinue)
  on(['disk', 'overview'], renderDisk)
  renderNav()
  renderDisk()
}

function renderNav() {
  const o = state.overview
  const onLibrary = state.route === 'library'
  $('#shelves').innerHTML = SHELVES.map(s => {
    const active = onLibrary && state.query.status === s.status
    const count = o ? num(s.count({ ...o, queue: state.queue })) : '–'
    const pulse = s.pulse && state.queue.active ? 'animate-pulse' : ''
    return `
      <a href="#" data-shelf="${s.status}" class="flex items-center justify-between px-space-sm py-1.5 rounded-lg transition-colors ${active ? ACTIVE : IDLE}">
        <span class="flex items-center gap-space-sm">${icon(s.icon, 'text-[18px]')}<span class="text-body-sm">${s.label}</span></span>
        <span class="text-label-sm bg-surface-container px-1.5 py-0.5 rounded-lg ${s.tone} ${pulse}">${count}</span>
      </a>`
  }).join('')

  $('#pages-nav').innerHTML = PAGES.map(p => `
    <a href="#" data-route="${p.route}" class="flex items-center gap-space-sm px-space-sm py-1.5 rounded-lg transition-colors ${state.route === p.route ? ACTIVE : IDLE}">
      ${icon(p.icon, 'text-[18px]')}<span class="text-body-sm">${p.label}</span>
    </a>`).join('')
}

function renderContinue() {
  const recent = Object.entries(state.reading)
    .filter(([, p]) => p.fraction > 0 && p.fraction < 0.995)
    .sort((a, b) => b[1].updated_at - a[1].updated_at)[0]
  const el = $('#continue')
  if (!recent) {
    el.hidden = true
    return
  }
  const [repo, p] = recent
  const pct = Math.round(p.fraction * 100)
  el.hidden = false
  el.innerHTML = `
    <div class="eyebrow px-space-xs pb-1">Continue Reading</div>
    <button data-continue="${esc(repo)}" class="w-full flex items-center gap-space-sm p-1.5 rounded-xl bg-surface-container hover:bg-surface-container-high text-left transition-colors">
      <div class="w-8 aspect-[2/3] rounded-lg overflow-hidden cover-fallback shrink-0">
        ${p.cover_path ? `<img class="w-full h-full object-cover" src="${esc(coverUrl(p.cover_path))}" alt="">` : ''}
      </div>
      <div class="min-w-0 flex-1">
        <div class="text-body-sm text-on-surface truncate">${esc(p.title || repo)}</div>
        <div class="mt-1 h-1 rounded-full bg-surface-container-highest overflow-hidden"><div class="h-full bg-primary" style="width:${pct}%"></div></div>
        <div class="text-[10px] text-outline mt-0.5">${pct}% read</div>
      </div>
    </button>`
}

function renderDisk() {
  const d = state.disk
  const o = state.overview
  const used = d && d.total_bytes ? Math.round(((d.total_bytes - d.free_bytes) / d.total_bytes) * 100) : 0
  $('#disk').innerHTML = `
    <div class="flex items-center justify-between">
      <span class="text-label-sm text-on-surface-variant">Local Library</span>
      <span class="text-label-sm text-primary">${d ? bytes(d.library_bytes) : '…'}</span>
    </div>
    <div class="w-full h-1.5 bg-surface-container-highest rounded-full overflow-hidden" title="${used}% of the drive in use">
      <div class="bg-primary h-full" style="width:${used}%"></div>
    </div>
    <div class="flex justify-between items-center text-[10px] text-outline">
      <span>${d ? `${bytes(d.free_bytes)} free on ${esc(d.drive)}` : ''}</span>
      <span>${o ? `${num(o.not_built)} not built` : ''}</span>
    </div>`
}
