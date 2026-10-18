// Michael Burgus — https://github.com/NeuralDrifter
// Full-text search through the open book

import { R, rq, cleanLabel } from './ctx.js'
import { esc, icon, num } from '../util.js'

const MAX_RESULTS = 500

let run = 0          // bumps to abandon a running search
let query = ''
let groups = []      // [{ label, items: [{ cfi, excerpt }] }]
let total = 0
let progress = 0
let done = true

export function resetSearch() {
  run++
  query = ''
  groups = []
  total = 0
  progress = 0
  done = true
  R.view?.clearSearch()
}

export function searchHtml() {
  return `
    <div class="flex items-center justify-between">
      <h3 class="flex items-center gap-1.5 font-display text-headline-sm text-on-surface">${icon('search', 'text-[18px] text-primary')}Search the Text</h3>
      ${query ? '<button data-search="clear" class="text-label-sm text-outline hover:text-primary">Clear</button>' : ''}
    </div>
    <form data-search-form class="relative">
      ${icon('search', 'absolute left-2 top-1/2 -translate-y-1/2 text-[16px] text-outline pointer-events-none')}
      <input data-search-input type="search" value="${esc(query)}" placeholder="Words or a phrase…"
        class="field w-full pl-8 text-body-sm" autocomplete="off" spellcheck="false">
    </form>
    <div data-search-status class="text-label-sm text-outline">${statusText()}</div>
    <div data-search-results class="flex flex-col gap-space-sm">${resultsHtml()}</div>`
}

function statusText() {
  if (!query) return 'Press Enter to search every chapter.'
  const found = total ? `${num(total)} match${total === 1 ? '' : 'es'}` : 'No matches'
  if (!done) return `Searching… ${Math.round(progress * 100)}% · ${found.toLowerCase()} so far`
  return total >= MAX_RESULTS ? `First ${num(MAX_RESULTS)} matches shown` : found
}

function resultsHtml() {
  return groups.map((g, gi) => `
    <div class="flex flex-col gap-1">
      <div class="eyebrow truncate">${esc(g.label || 'Untitled section')}</div>
      ${g.items.map((it, ii) => `
        <button data-hit="${gi}:${ii}" class="text-left text-body-sm text-on-surface-variant leading-snug p-1.5 rounded-lg hover:bg-surface-container">
          ${esc(it.excerpt.pre)}<mark class="bg-primary/25 text-on-surface rounded-sm px-0.5">${esc(it.excerpt.match)}</mark>${esc(it.excerpt.post)}
        </button>`).join('')}
    </div>`).join('')
}

function paint() {
  const status = rq('[data-search-status]')
  if (!status) return // panel closed meanwhile
  status.textContent = statusText()
  rq('[data-search-results]').innerHTML = resultsHtml()
}

async function start(text) {
  resetSearch()
  query = text.trim()
  if (!query || !R.view) {
    paint()
    return
  }
  const mine = run
  done = false
  paint()
  try {
    for await (const result of R.view.search({ query })) {
      if (mine !== run) return
      if (result === 'done') break
      if (result.progress != null) progress = result.progress
      if (result.subitems?.length) {
        const room = MAX_RESULTS - total
        const items = result.subitems.slice(0, room)
        groups.push({ label: cleanLabel(result.label), items })
        total += items.length
        if (total >= MAX_RESULTS) break
      }
      paint()
    }
  } catch (err) {
    console.error('search failed', err)
  }
  if (mine !== run) return
  done = true
  paint()
}

export function handleSearchSubmit(e) {
  if (!e.target.matches('[data-search-form]')) return false
  e.preventDefault()
  start(rq('[data-search-input]').value)
  return true
}

export function handleSearchClick(e) {
  if (e.target.closest('[data-search="clear"]')) {
    resetSearch()
    return 'rerender'
  }
  const hit = e.target.closest('[data-hit]')
  if (!hit) return false
  const [gi, ii] = hit.dataset.hit.split(':').map(Number)
  const cfi = groups[gi]?.items[ii]?.cfi
  if (cfi) {
    R.backCfi ??= R.position?.cfi ?? null
    R.view.goTo(cfi).catch(err => console.error(err))
  }
  return true
}

export const focusSearch = () => rq('[data-search-input]')?.focus()
