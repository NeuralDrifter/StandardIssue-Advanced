// Michael Burgus — https://github.com/NeuralDrifter
// Boot (instance-lock gate), component wiring and backend event handling

import { api, events } from './api.js'
import { state, emit, notify } from './state.js'
import { refreshQuery, refreshOverview, refreshDisk, refreshReading, refreshSoon, setRoute } from './actions.js'
import { initTitlebar } from './titlebar.js'
import { initSidebar } from './sidebar.js'
import { initToolbar } from './toolbar.js'
import { initHero } from './hero.js'
import { initGrid, patchBook, rowOf } from './grid.js'
import { initInspector } from './inspector.js'
import { initPages } from './pages.js'
import { $, debounce } from './util.js'

initTitlebar()
boot()

// Nothing loads until this copy holds the instance lock (only one edition may run at a time)
async function boot() {
  const lock = $('#lock-screen')
  let status
  try {
    status = await api.instanceStatus()
  } catch (err) {
    $('#lock-title').textContent = 'StandardIssue cannot start'
    $('#lock-detail').textContent = String(err)
    lock.hidden = false
    return
  }
  if (!status.ready) {
    const other = status.held_by ? `StandardIssue ${status.held_by.edition}` : 'Another copy of StandardIssue'
    $('#lock-title').textContent = `${other} is already running`
    $('#lock-detail').textContent = 'Close it to continue. This window will carry on automatically.'
    lock.hidden = false
    setTimeout(boot, 2000)
    return
  }
  lock.hidden = true

  state.settings = await api.getSettings()
  initSidebar()
  initToolbar()
  initHero()
  initGrid()
  initInspector()
  initPages()
  wireEvents()
  emit('settings')

  await Promise.all([refreshOverview(), refreshQuery(), refreshReading()])
  refreshDisk()
  showStartupReport(status)
}

function showStartupReport(status) {
  const notes = []
  const m = status.migration
  if (m && (m.moved || m.duplicates || m.conflicts.length || m.failures.length)) {
    notes.push(`Library moved to Documents\\StandardIssue Library: ${m.moved} moved, ${m.duplicates} duplicates removed` +
      (m.conflicts.length ? `, ${m.conflicts.length} conflicts left in place` : '') +
      (m.failures.length ? `, ${m.failures.length} failed` : ''))
    if (m.conflicts.length || m.failures.length) console.warn('library migration', m)
  }
  if (status.refiled?.moved) notes.push(`Re-filed ${status.refiled.moved} epubs into genre folders`)
  notes.push(...status.warnings)
  if (notes.length) notify(notes.join(' — '), status.warnings.length ? 'error' : 'info')
}

function wireEvents() {
  // [data-route] links anywhere (sidebar, title bar) switch pages
  document.addEventListener('click', e => {
    const link = e.target.closest('[data-route]')
    if (!link) return
    e.preventDefault()
    setRoute(link.dataset.route)
  })

  events.buildUpdate(onBuildUpdate)
  events.buildSummary(summary => {
    state.queue = summary
    emit('queue')
    refreshQueuePositions()
  })

  events.opProgress(p => {
    if (!state.op) return
    if (p.message) state.op.message = p.message
    if (p.total) Object.assign(state.op, { current: p.current, total: p.total, message: `${p.title ?? ''} — ${p.status ?? ''}` })
    emit('op')
  })
  events.opDone(({ operation, summary }) => {
    const parts = Object.entries(summary ?? {})
      .filter(([k, v]) => v != null && k !== 'rate_remaining')
      .map(([k, v]) => `${v} ${k}`)
    notify(`${operation} finished: ${parts.join(', ')}`)
  })
  events.opError(({ operation, error }) => notify(`${operation} failed: ${error}`, 'error'))

  // Reader overlay hooks
  const hookReader = () => {
    if (!window.Reader) return false
    window.Reader.onClose = refreshReading
    window.Reader.onLibraryChanged = refreshSoon
    return true
  }
  if (!hookReader()) window.addEventListener('load', hookReader, { once: true })

  setInterval(refreshDisk, 5 * 60_000)
}

async function onBuildUpdate(u) {
  const finished = u.state === 'built' || u.state === 'failed'
  state.live[u.repo] = { state: u.state, pct: u.pct, phase: u.phase, queue_pos: 0, error: u.error }

  // Keep the pipeline strip's percentages moving between summaries
  const active = state.queue.active_list.find(a => a.repo === u.repo)
  if (active && !finished) {
    active.pct = u.pct
    active.phase = u.phase
    emit('queue-progress')
  }

  patchBook(u.repo)
  if (u.repo === state.inspected) emit('live-inspected')

  if (finished) {
    if (u.state === 'failed') notify(`Build failed: ${rowOf(u.repo)?.title ?? u.repo} — ${u.error}`, 'error')
    try {
      const row = await api.getBook(u.repo)
      const books = state.result?.books ?? []
      const i = books.findIndex(b => b.repo_name === u.repo)
      if (row && i >= 0) books[i] = row
      delete state.live[u.repo]
      patchBook(u.repo)
      if (u.repo === state.inspected) emit('inspected')
    } catch (err) {
      console.error('refresh built book', err)
    }
    refreshOverviewSoon()
    refreshDiskSoon()
  }
}

const refreshOverviewSoon = debounce(refreshOverview, 500)
const refreshDiskSoon = debounce(refreshDisk, 3000)

/** Queue positions shift as books finish; re-read them for the visible cards. */
const refreshQueuePositions = debounce(async () => {
  const repos = state.result?.books.map(b => b.repo_name) ?? []
  if (!repos.length) return
  try {
    const live = await api.liveStates(repos)
    for (const [repo, l] of Object.entries(live)) {
      const before = JSON.stringify(state.live[repo] ?? null)
      if (l) state.live[repo] = l
      else if (state.live[repo]?.state === 'queued' || state.live[repo]?.state === 'building') delete state.live[repo]
      if (JSON.stringify(state.live[repo] ?? null) !== before) {
        patchBook(repo)
        if (repo === state.inspected) emit('live-inspected')
      }
    }
  } catch (err) {
    console.error('live states', err)
  }
}, 300)
