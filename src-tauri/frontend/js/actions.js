// Michael Burgus — https://github.com/NeuralDrifter
// Actions shared by components: data refreshes, builds, operations, navigation

import { api } from './api.js'
import { state, emit, notify } from './state.js'
import { debounce } from './util.js'
import Reader from './reader/index.js'

// --- Data refreshes ---

let querySeq = 0

export async function refreshQuery() {
  const seq = ++querySeq
  try {
    const result = await api.queryBooks({ ...state.query, per_page: state.settings?.page_size ?? 24 })
    if (seq !== querySeq) return // a newer query superseded this one
    state.result = result
    state.query.page = result.page
    // Fresh rows are authoritative: seed live pipeline state from them, drop stale entries
    for (const b of result.books) {
      if (b.live) state.live[b.repo_name] = b.live
      else delete state.live[b.repo_name]
    }
    if (!state.inspected && result.books.length) state.inspected = result.books[0].repo_name
    emit('result', 'inspected')
  } catch (err) {
    notify(`Could not load the library: ${err}`, 'error')
  }
}

export async function refreshOverview() {
  try {
    state.overview = await api.overview()
    state.queue = state.overview.queue
    if (state.overview.operation && !state.op) state.op = { name: state.overview.operation, message: 'Running…' }
    emit('overview', 'queue', 'op')
  } catch (err) {
    console.error('overview', err)
  }
}

export async function refreshDisk() {
  try {
    state.disk = await api.diskUsage()
    emit('disk')
  } catch (err) {
    console.error('disk usage', err)
  }
}

export async function refreshReading() {
  try {
    state.reading = (await api.readingProgress()) ?? {}
    emit('reading')
  } catch (err) {
    console.error('reading progress', err)
  }
}

export const refreshSoon = debounce(() => {
  refreshOverview()
  refreshQuery()
}, 400)

// --- Navigation and filters ---

export function setQuery(patch, { resetPage = true } = {}) {
  if (Reader.isOpen()) Reader.close()
  Object.assign(state.query, patch)
  if (resetPage) state.query.page = 1
  emit('query')
  refreshQuery()
}

export function setRoute(route) {
  if (Reader.isOpen()) Reader.close()
  state.route = route
  emit('route')
}

export function inspect(repo) {
  state.inspected = repo
  state.inspectorOpen = true
  emit('inspected', 'inspector-visibility')
}

export function toggleSelected(repo, on = !state.selection.has(repo)) {
  if (on) state.selection.add(repo)
  else state.selection.delete(repo)
  emit('selection')
}

export function clearSelection() {
  state.selection.clear()
  emit('selection')
}

export function openReader(repo) {
  if (repo) Reader.open(repo)
}

// --- Builds ---

export function buildToggles() {
  return {
    metadata: state.settings?.build_metadata ?? true,
    covers: state.settings?.build_covers ?? true,
  }
}

export async function setBuildToggle(key, value) {
  state.settings = await api.saveSettings({ ...state.settings, [key]: value })
  emit('settings')
}

export async function build(repos, { force = false } = {}) {
  if (!repos.length) return
  try {
    const { added } = await api.buildBooks(repos, { ...buildToggles(), force })
    if (!added) notify(repos.length === 1 ? 'That book is already in the pipeline.' : 'Those books are already in the pipeline.')
  } catch (err) {
    notify(`Could not start the build: ${err}`, 'error')
  }
}

export async function buildSelected() {
  const repos = [...state.selection]
  await build(repos)
  clearSelection()
}

export async function buildAll() {
  try {
    const { added } = await api.buildAll()
    notify(added ? `Queued ${added} books to build.` : 'Every book is already built.')
  } catch (err) {
    notify(`Build All failed to start: ${err}`, 'error')
  }
}

export async function stopAll() {
  if (state.queue.running) await api.cancelBuilds()
  if (state.op) await api.cancelOperation()
  notify('Stopping…')
}

// --- Operations (sync / enrich / covers) ---

const OPS = {
  sync: ['Catalog sync', api.syncCatalog],
  enrich: ['Metadata enrichment', api.enrichAll],
  covers: ['Cover fetch', api.fetchCovers],
}

export async function runOp(key) {
  if (state.op) {
    notify(`${state.op.name} is already running.`)
    return
  }
  const [name, call] = OPS[key]
  state.op = { name, message: 'Starting…' }
  emit('op')
  try {
    await call()
  } catch (err) {
    // op-error already reported it; this catches refusals before start
    if (!state.notice || state.notice.kind !== 'error') notify(`${name}: ${err}`, 'error')
  } finally {
    state.op = null
    state.coverBust = Date.now()
    emit('op')
    refreshOverview()
    refreshQuery()
    refreshDisk()
  }
}
