// Michael Burgus — https://github.com/NeuralDrifter
// Hero status panel and the live pipeline strip beneath it

import { state, on } from './state.js'
import { runOp, buildSelected, stopAll, inspect } from './actions.js'
import { toggleHtml, handleToggleClick } from './toolbar.js'
import { $, esc, num, ago, icon } from './util.js'

const RING = 'M18 2.0845 a 15.9155 15.9155 0 0 1 0 31.831 a 15.9155 15.9155 0 0 1 0 -31.831'

export function initHero() {
  const el = $('#hero')
  el.innerHTML = `
    <div class="relative overflow-hidden rounded-xl bg-surface-container-low shadow-xl p-space-md">
      <div class="absolute -right-16 -top-16 w-64 h-64 rounded-full bg-primary/5 blur-3xl pointer-events-none"></div>
      <div class="relative flex flex-col 2xl:flex-row 2xl:items-center justify-between gap-space-md">
        <div id="hero-status" class="flex flex-wrap items-center gap-space-md"></div>
        <div class="flex flex-col items-end gap-1.5">
          <div class="flex flex-wrap items-center gap-space-xs">
            <button class="btn-glass-lg group" data-op="sync">${icon('sync', 'text-[18px] text-primary group-hover:rotate-180 transition-transform duration-500')}<span>Sync GitHub</span></button>
            <button class="btn-glass-lg" data-op="enrich">${icon('auto_fix_high', 'text-[18px] text-secondary')}<span>Enrich Metadata</span></button>
            <button class="btn-glass-lg" data-op="covers">${icon('palette', 'text-[18px] text-tertiary')}<span>Fetch Artwork</span></button>
            <button class="btn-gold-lg" id="build-selected">${icon('build_circle', 'text-[18px]')}<span></span></button>
          </div>
          <div id="hero-toggles" class="flex items-center gap-space-md pr-1"></div>
        </div>
      </div>
      <div id="pipeline" class="mt-space-md pt-space-sm border-t border-surface-container-highest/30 flex flex-wrap items-center justify-between gap-space-sm text-label-sm min-h-8"></div>
    </div>`

  el.addEventListener('click', e => {
    if (handleToggleClick(e)) return
    const op = e.target.closest('[data-op]')
    if (op) runOp(op.dataset.op)
    if (e.target.closest('#build-selected')) buildSelected()
    if (e.target.closest('#stop-all')) stopAll()
    const book = e.target.closest('[data-inspect]')
    if (book) inspect(book.dataset.inspect)
  })

  on(['overview', 'queue', 'op'], renderStatus)
  on(['selection', 'op'], renderBuildSelected)
  on('settings', renderToggles)
  on(['queue', 'queue-progress', 'op', 'notice', 'overview'], renderPipeline)
  renderStatus()
  renderBuildSelected()
  renderToggles()
  renderPipeline()
}

function renderStatus() {
  const o = state.overview
  if (!o) return
  const pct = o.total ? Math.floor((o.built / o.total) * 100) : 0
  const last = o.activity?.last_sync
  const synced = state.op?.name === 'Catalog sync'
    ? `${icon('sync', 'text-[16px] animate-spin')}<span class="font-semibold">Syncing catalog…</span>`
    : last
      ? `${icon('check_circle', 'text-[16px]')}<span class="font-semibold">Catalog synced ${ago(last.at)}</span>`
      : `${icon('error', 'text-[16px]')}<span class="font-semibold">Never synced — run Sync GitHub</span>`
  $('#hero-status').innerHTML = `
    <div class="flex items-center gap-space-sm bg-surface-container px-space-md py-space-sm rounded-xl shadow-sm">
      <div class="w-2.5 h-2.5 rounded-full ${last ? 'bg-tertiary' : 'bg-outline'} animate-pulse shadow-[0_0_8px_currentColor] ${last ? 'text-tertiary' : 'text-outline'}"></div>
      <div class="flex flex-col">
        <span class="eyebrow">Catalog Source</span>
        <span class="text-body-sm text-on-surface font-semibold flex items-center gap-1">standardebooks
          <span class="text-label-sm text-secondary bg-surface-container-high px-1 rounded-lg">GitHub</span></span>
      </div>
    </div>
    <div class="flex items-center gap-space-md pl-space-xs">
      <div class="flex flex-col">
        <div class="flex items-baseline gap-1.5">
          <span class="font-display text-headline-lg text-primary font-bold tracking-tight">${num(o.built)}</span>
          <span class="text-label-md text-outline">/ ${num(o.total)}</span>
        </div>
        <span class="text-label-sm text-on-surface-variant uppercase tracking-wider">Built Editions</span>
      </div>
      <div class="relative w-12 h-12 flex items-center justify-center" title="${pct}% of the catalog built">
        <svg class="w-12 h-12 -rotate-90" viewBox="0 0 36 36">
          <path class="text-surface-container-highest" d="${RING}" fill="none" stroke="currentColor" stroke-width="3"></path>
          <path class="text-tertiary" d="${RING}" fill="none" stroke="currentColor" stroke-dasharray="${pct}, 100" stroke-linecap="round" stroke-width="3"></path>
        </svg>
        <span class="absolute text-label-sm text-tertiary font-bold">${pct}%</span>
      </div>
    </div>
    <div class="hidden md:flex flex-col border-l border-surface-container-highest/40 pl-space-md">
      <div class="flex items-center gap-1.5 ${last ? 'text-tertiary' : 'text-outline'} text-label-md">${synced}</div>
      <span class="text-body-sm text-outline">Metadata ${num(o.with_metadata)} / ${num(o.total)} · Colophons ${num(o.with_colophon)} · Artwork ${num(o.with_covers)} / ${num(o.total)}</span>
    </div>`
}

function renderBuildSelected() {
  const n = state.selection.size
  const btn = $('#build-selected')
  btn.disabled = n === 0
  btn.querySelector('span:last-child').textContent = `Build Selected (${n})`
  btn.title = n ? '' : 'Tick books with the checkbox on their cover to build just those'
  for (const b of $('#hero').querySelectorAll('[data-op]')) b.disabled = !!state.op
}

function renderToggles() {
  $('#hero-toggles').innerHTML = `<span class="text-label-sm text-outline">With build:</span>` +
    toggleHtml('build_metadata', 'Metadata') + toggleHtml('build_covers', 'Cover art')
}

function renderPipeline() {
  const q = state.queue
  const op = state.op
  let left = ''
  let right = ''

  if (q.running || q.active || q.queued) {
    const building = q.active_list.map(a =>
      `<button data-inspect="${esc(a.repo)}" class="hover:underline">${esc(a.title)} <span class="text-primary font-semibold">${a.pct}%</span></button>`).join('<span class="text-outline">·</span>')
    const next = q.next[0]
    left = `
      <span class="flex items-center gap-1.5 flex-wrap">
        <span class="dot bg-primary animate-ping"></span><span class="text-primary font-semibold">Building:</span>
        <span class="text-on-surface-variant flex flex-wrap gap-1.5">${building || 'starting…'}</span>
      </span>
      ${next ? `<span class="hidden sm:inline text-outline">•</span>
        <span class="hidden sm:inline text-on-surface-variant">Next in queue: <button data-inspect="${esc(next.repo)}" class="text-on-surface font-medium hover:underline">${esc(next.title)}</button></span>` : ''}`
    right = `<span class="text-outline">${num(q.queued)} queued · ${num(q.done)} built · ${q.failed ? `<span class="text-error">${num(q.failed)} failed</span>` : '0 failed'}</span>`
  } else if (op) {
    const counter = op.total ? ` <span class="text-outline">[${num(op.current)}/${num(op.total)}]</span>` : ''
    left = `<span class="flex items-center gap-1.5 min-w-0"><span class="dot bg-secondary animate-pulse"></span>
      <span class="text-secondary font-semibold">${esc(op.name)}:</span>${counter}
      <span class="text-on-surface-variant truncate">${esc(op.message ?? '')}</span></span>`
  } else if (state.notice) {
    const err = state.notice.kind === 'error'
    left = `<span class="flex items-center gap-1.5 ${err ? 'text-error' : 'text-on-surface-variant'}">${icon(err ? 'error' : 'info', 'text-[14px]')}${esc(state.notice.text)}</span>`
  } else if (q.done || q.failed) {
    left = `<span class="flex items-center gap-1.5 text-on-surface-variant">${icon('task_alt', 'text-[14px] text-tertiary')}Last build run: ${num(q.done)} built${q.failed ? `, <span class="text-error">${num(q.failed)} failed</span>` : ''}</span>`
  } else {
    left = `<span class="flex items-center gap-1.5 text-outline">${icon('check', 'text-[14px] text-tertiary')}Pipeline idle</span>`
  }

  const busy = q.running || !!op
  $('#pipeline').innerHTML = `
    <div class="flex items-center gap-space-md min-w-0 flex-wrap">${left}</div>
    <div class="flex items-center gap-space-sm">${right}
      ${busy ? `<button id="stop-all" class="btn-danger">${icon('stop_circle', 'text-[16px]')}Stop</button>` : `<span class="flex items-center gap-1 text-outline">Output: EPUB 3 ${icon('verified', 'text-[14px] text-primary')}</span>`}
    </div>`
}
