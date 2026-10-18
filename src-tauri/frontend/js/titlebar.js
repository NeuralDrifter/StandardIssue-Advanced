// Michael Burgus — https://github.com/NeuralDrifter
// Custom window chrome: version chip, sync status chip, window controls

import { state, on } from './state.js'
import { setRoute } from './actions.js'
import { $, esc, ago } from './util.js'

const win = window.__TAURI__.window.getCurrentWindow()

export function initTitlebar() {
  $('#win-min').addEventListener('click', () => win.minimize())
  $('#win-max').addEventListener('click', () => win.toggleMaximize())
  $('#win-close').addEventListener('click', () => win.close())
  $('#tb-sync').addEventListener('click', () => setRoute('sync'))
  $('#titlebar').addEventListener('dblclick', e => {
    if (e.target.closest('button')) return
    win.toggleMaximize()
  })

  const updateMaxIcon = async () => {
    const max = await win.isMaximized()
    $('#win-max span').textContent = max ? 'filter_none' : 'crop_square'
    $('#win-max').title = max ? 'Restore' : 'Maximize'
  }
  win.onResized(updateMaxIcon)
  updateMaxIcon()

  window.__TAURI__.app.getVersion().then(v => {
    state.version = v
    $('#tb-version').textContent = `Advanced v${v}`
  })

  on(['overview', 'op'], renderSync)
  setInterval(renderSync, 60_000) // keep "14m ago" fresh
  renderSync()
}

function renderSync() {
  const last = state.overview?.activity?.last_sync
  const syncing = state.op?.name === 'Catalog sync'
  const [dot, text] = syncing
    ? ['bg-primary animate-pulse', 'github.com/standardebooks · syncing…']
    : last
      ? ['bg-tertiary', `github.com/standardebooks · synced ${ago(last.at)}`]
      : ['bg-outline', 'github.com/standardebooks · never synced']
  $('#tb-sync').innerHTML = `<span class="dot ${dot}"></span><span class="text-label-sm text-on-surface-variant">${esc(text)}</span>`
}
