// Michael Burgus — https://github.com/NeuralDrifter
// Left panel: the book card, chapter filter, contents with reading times, and the pace card

import { R, rq, bookWords, sectionWords, currentSection, fractionInSection, duration, cleanLabel } from './ctx.js'
import { esc, num, icon, coverUrl } from '../util.js'

let entries = []   // flattened TOC: { item, depth, index, group, minutes }
let filter = ''

/** Build the contents model once the book is open. */
export function loadContents() {
  filter = ''
  entries = []
  const walk = (items, depth) => {
    for (const item of items ?? []) {
      let index = -1
      try {
        index = item.href ? R.view.resolveNavigation(item.href)?.index ?? -1 : -1
      } catch { /* unresolvable entry */ }
      entries.push({ item, depth, index, group: !!item.subitems?.length })
      walk(item.subitems, depth + 1)
    }
  }
  walk(R.book.toc, 0)

  // Several entries can point into one file; they share its reading time
  const perSection = new Map()
  for (const e of entries) if (!e.group && e.index >= 0) perSection.set(e.index, (perSection.get(e.index) ?? 0) + 1)
  for (const e of entries) e.share = e.group || e.index < 0 ? 0 : 1 / perSection.get(e.index)
}

/** Chapters: leaf entries between the start of the body matter and the back matter. */
export function chapterCount() {
  const at = type => {
    const mark = R.book.landmarks?.find(m => m.type?.includes(type))
    try {
      return mark ? R.view.resolveNavigation(mark.href)?.index ?? -1 : -1
    } catch {
      return -1
    }
  }
  const start = Math.max(0, at('bodymatter'))
  const ends = ['backmatter', 'endnotes', 'colophon'].map(at).filter(i => i > start)
  const end = ends.length ? Math.min(...ends) : Infinity
  return entries.filter(e => !e.group && e.index >= start && e.index < end).length
}

export function contentsShell() {
  const info = R.info
  const year = info.published?.label ? ` · ${esc(info.published.label)}` : ''
  return `
    <div class="flex gap-space-sm p-space-sm rounded-xl bg-surface-container-low shadow-lg">
      <div class="w-14 shrink-0 aspect-[2/3] rounded-lg overflow-hidden cover-fallback shadow-md">
        ${info.cover_path ? `<img class="w-full h-full object-cover" src="${esc(coverUrl(info.cover_path))}" alt="">` : ''}
      </div>
      <div class="min-w-0 flex flex-col gap-0.5">
        <div class="font-display text-body-lg text-on-surface leading-tight line-clamp-2">${esc(info.title)}</div>
        <div class="text-label-sm text-on-surface-variant truncate">${esc(info.author)}${year}</div>
        <div class="flex items-center gap-1 text-label-sm text-tertiary"><span class="dot bg-tertiary"></span>Standard Ebooks · EPUB 3</div>
        <div class="text-label-sm text-outline" data-book-stats></div>
      </div>
    </div>
    <div class="relative">
      ${icon('filter_list', 'absolute left-2 top-1/2 -translate-y-1/2 text-[16px] text-outline pointer-events-none')}
      <input data-toc-filter type="search" placeholder="Filter chapter or section…"
        class="field w-full pl-8 text-body-sm" autocomplete="off" spellcheck="false">
    </div>
    <div class="flex-1 min-h-0 flex flex-col rounded-xl bg-surface-container-low shadow-lg overflow-hidden">
      <div class="flex items-center justify-between px-space-sm pt-space-sm pb-1">
        <span class="eyebrow">Index &amp; Manifest</span>
        <span class="text-label-sm text-primary font-semibold" data-read-pct></span>
      </div>
      <nav data-toc class="flex-1 min-h-0 overflow-y-auto px-1 pb-space-sm"></nav>
    </div>
    <div data-pace class="p-space-sm rounded-xl bg-surface-container-low shadow-lg"></div>`
}

export function renderContents() {
  const words = bookWords()
  rq('[data-book-stats]').textContent =
    [words ? `${num(words)} words` : '', `${chapterCount()} chapters`].filter(Boolean).join(' • ')
  renderToc()
}

function renderToc() {
  const q = filter.toLowerCase()
  const visible = q ? matching(q) : null
  const current = R.position?.tocItem?.id
  const reading = currentSection()
  const words = bookWords()

  rq('[data-toc]').innerHTML = entries.map((e, i) => {
    if (visible && !visible.has(i)) return ''
    const label = esc(cleanLabel(e.item.label) || '—')
    const pad = `padding-left:${0.5 + e.depth * 0.75}rem`
    if (e.group) {
      return `<button data-goto="${i}" class="w-full flex items-center justify-between gap-1 pr-2 pt-space-sm pb-1 text-left eyebrow hover:text-primary" style="${pad}">
        <span class="truncate">${label}</span>${icon('expand_more', 'text-[14px]')}</button>`
    }
    const isCurrent = e.item.id === current
    const mins = words && e.index >= 0 ? (sectionWords(e.index) * e.share) / R.wpm : 0
    const done = e.index >= 0 && e.index < reading
    return `
      <button data-goto="${i}" class="relative w-full flex items-center gap-1.5 pr-2 py-1.5 rounded-lg text-left text-body-sm transition-colors
        ${isCurrent ? 'bg-surface-container text-on-surface' : done ? 'text-outline hover:bg-surface-container' : 'text-on-surface-variant hover:bg-surface-container hover:text-on-surface'}" style="${pad}">
        ${isCurrent ? '<span class="absolute left-0 top-1.5 bottom-1.5 w-0.5 rounded-full bg-primary"></span>' : ''}
        <span class="flex-1 min-w-0 truncate">${label}</span>
        ${isCurrent
          ? '<span class="shrink-0 text-[10px] font-semibold px-1.5 rounded-lg bg-primary/15 text-primary">Reading</span>'
          : mins >= 1 ? `<span class="shrink-0 text-[10px] text-outline">${duration(mins)}</span>` : ''}
      </button>`
  }).join('') || '<div class="px-2 py-space-sm text-body-sm text-outline">No matching chapters.</div>'

  rq('[data-toc] .bg-primary')?.closest('button').scrollIntoView({ block: 'nearest' })
}

/** Indices of entries whose label matches, plus the groups that contain them. */
function matching(q) {
  const keep = new Set()
  const parents = []
  for (const [i, e] of entries.entries()) {
    parents.length = e.depth
    parents[e.depth] = i
    if (cleanLabel(e.item.label).toLowerCase().includes(q)) parents.forEach(p => keep.add(p))
  }
  return keep
}

/** Position changed: refresh the % read, current entry and pace card. */
export function markContents() {
  rq('[data-read-pct]').textContent = `${Math.floor((R.position?.fraction ?? 0) * 100)}% Read`
  renderToc()
  renderPace()
}

export function renderPace() {
  const el = rq('[data-pace]')
  const words = bookWords()
  if (!words) {
    el.innerHTML = `<div class="flex items-center gap-1.5 text-label-sm text-outline">${icon('schedule', 'text-[16px]')}
      Reading times appear after Enrich Metadata fills in the word count.</div>`
    return
  }
  const f = R.position?.fraction ?? 0
  const i = currentSection()
  const chapterLeft = i >= 0 ? (sectionWords(i) * (1 - fractionInSection(i, f))) / R.wpm : 0
  const bookLeft = (words * (1 - f)) / R.wpm
  const chapter = cleanLabel(R.position?.tocItem?.label)
  const measured = R.pace.measured
  el.innerHTML = `
    <div class="flex items-center justify-between gap-2">
      <div class="flex items-center gap-2 min-w-0">
        ${icon('schedule', 'text-[18px] text-primary')}
        <div class="min-w-0">
          <div class="text-label-md text-on-surface" title="${measured ? 'Measured from your own page turns' : 'Default pace until five minutes of reading have been measured'}">${R.wpm} wpm pace${measured ? '' : ' <span class="text-outline">(default)</span>'}</div>
          <div class="text-[10px] text-outline truncate">${duration(chapterLeft)} left in ${esc(shortLabel(chapter) || 'this section')}</div>
        </div>
      </div>
      <span class="shrink-0 text-label-sm font-semibold px-2 py-0.5 rounded-lg bg-secondary-container text-on-secondary-container">${duration(bookLeft)} left</span>
    </div>`
}

/** "Chapter I: There Are Heroisms All Round Us" → "Chapter I"; "IV: It's Just…" → "ch. IV" */
export function shortLabel(label) {
  if (!label) return ''
  const head = label.split(/[:.—–]/)[0].trim()
  if (/^[IVXLCDM]+$/.test(head) || /^\d+$/.test(head)) return `ch. ${head}`
  return head.length > 22 ? `${head.slice(0, 21)}…` : head
}

export function handleContentsClick(e) {
  const go = e.target.closest('[data-goto]')
  if (!go) return false
  const entry = entries[+go.dataset.goto]
  if (entry?.item.href) R.view.goTo(entry.item.href).catch(err => console.error(err))
  return true
}

export function handleContentsInput(e) {
  if (!e.target.matches('[data-toc-filter]')) return false
  filter = e.target.value.trim()
  renderToc()
  return true
}

/** Section start fractions for chapters, used by the scrubber ticks and labels. */
export function chapterMarks() {
  const seen = new Set()
  return entries
    .filter(e => !e.group && e.index >= 0 && !seen.has(e.index) && seen.add(e.index))
    .map(e => ({ fraction: R.fractions[e.index] ?? 0, label: cleanLabel(e.item.label), index: e.index }))
}

export const contentsEntries = () => entries
