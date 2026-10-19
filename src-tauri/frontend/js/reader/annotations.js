// Michael Burgus — https://github.com/NeuralDrifter
// Highlights and notes: selection popover, drawing on the page, the Annotations panel, Markdown export

import { api } from '../api.js'
import { Overlayer } from '../../vendor/foliate-js/overlayer.js'
import { R, cleanLabel } from './ctx.js'
import { esc, icon, ago } from '../util.js'

export const COLORS = {
  gold: { label: 'Gold', css: '#e9c176' },
  sage: { label: 'Sage', css: '#9cc5a1' },
  rose: { label: 'Rose', css: '#e8a0a8' },
  sky: { label: 'Sky', css: '#8ab4e8' },
}
const DEFAULT_COLOR = 'gold'
const MAX_DEFINE_CHARS = 40

let pop = null        // popover element
let target = null     // { cfi, text, existing } the popover acts on
let openedAt = 0
let editing = null    // cfi whose note is being edited in the panel
let hooks = {}        // { onChange, onDefine }

/** onChange(): annotations list changed; onDefine(word): look a word up. */
export function initAnnotations(h) {
  hooks = h
}

const find = cfi => R.annotations.find(a => a.cfi === cfi)

// --- Drawing ---

/** Wire the view: draw saved highlights per section, open the popover on a highlight click. */
export function attachAnnotations(view) {
  view.addEventListener('draw-annotation', ({ detail: { draw, annotation } }) => {
    const color = COLORS[annotation.color]?.css ?? COLORS[DEFAULT_COLOR].css
    draw(Overlayer.highlight, { color })
  })
  view.addEventListener('create-overlay', ({ detail: { index } }) => {
    for (const a of R.annotations) {
      try {
        if (view.resolveNavigation(a.cfi)?.index === index) view.addAnnotation({ value: a.cfi, color: a.color })
      } catch (err) {
        console.warn('annotation no longer resolves', a.cfi, err)
      }
    }
  })
  view.addEventListener('show-annotation', ({ detail: { value, range } }) => {
    const a = find(value)
    if (a) showPopover({ cfi: a.cfi, text: a.text, existing: true }, range)
  })
}

const draw = a => R.view?.addAnnotation({ value: a.cfi, color: a.color }).catch(err => console.error(err))

// --- Selection popover ---

/** Call on pointerup / keyup inside a book page. */
export function onPageSelection(doc, index) {
  // Selection settles after the event; read it on the next frame
  requestAnimationFrame(() => {
    const sel = doc.getSelection()
    if (!sel || sel.isCollapsed || !sel.rangeCount) return
    const range = sel.getRangeAt(0)
    const text = sel.toString().replace(/\s+/g, ' ').trim()
    if (!text) return
    const cfi = R.view.getCFI(index, range)
    showPopover({ cfi, text, existing: !!find(cfi) }, range)
  })
}

/** True for a click that opened the popover just now (so it must not also turn the page). */
export const popoverJustOpened = () => Date.now() - openedAt < 300

export const popoverOpen = () => !!pop && !pop.hidden

export function hidePopover() {
  if (pop) pop.hidden = true
  target = null
}

function showPopover(t, range) {
  target = t
  openedAt = Date.now()
  if (!pop) {
    pop = document.createElement('div')
    pop.className = 'fixed z-50 flex flex-col gap-1.5 p-1.5 rounded-xl bg-surface-container-highest shadow-2xl border border-outline-variant/40 text-on-surface'
    pop.addEventListener('click', onPopoverClick)
    pop.addEventListener('pointerdown', e => e.stopPropagation())
    R.el.append(pop)
  }
  pop.innerHTML = popoverHtml(t)
  pop.hidden = false
  place(range)
}

function popoverHtml(t) {
  const a = t.existing ? find(t.cfi) : null
  const swatch = ([key, c]) => `
    <button data-hl="${key}" title="${c.label} highlight" class="w-6 h-6 rounded-full grid place-items-center ring-offset-2 ring-offset-surface-container-highest ${a?.color === key ? 'ring-2 ring-on-surface' : 'hover:ring-2 ring-outline'}" style="background:${c.css}"></button>`
  const action = (act, iconName, label) => `
    <button data-pop="${act}" class="flex items-center gap-1 h-7 px-2 rounded-lg text-label-md text-on-surface-variant hover:bg-surface-container-high hover:text-on-surface">${icon(iconName, 'text-[16px]')}${label}</button>`
  const word = t.text.length <= MAX_DEFINE_CHARS && !/\s/.test(t.text.trim())
  return `
    <div class="flex items-center gap-1">
      <div class="flex items-center gap-1.5 px-1">${Object.entries(COLORS).map(swatch).join('')}</div>
      <span class="w-px h-5 bg-outline-variant mx-0.5"></span>
      ${action('note', 'edit_note', a?.note ? 'Edit note' : 'Note')}
      ${action('copy', 'content_copy', 'Copy')}
      ${word && hooks.onDefine ? action('define', 'dictionary', 'Define') : ''}
      ${a ? action('delete', 'delete', '') : ''}
    </div>
    <form data-pop-note class="flex flex-col gap-1.5 p-1" hidden>
      <textarea rows="3" placeholder="Write a note…" class="field w-72 text-body-sm resize-y">${esc(a?.note ?? '')}</textarea>
      <div class="flex justify-end gap-1.5">
        <button type="button" data-pop="cancel-note" class="btn-glass">Cancel</button>
        <button type="submit" class="btn-gold">Save note</button>
      </div>
    </form>`
}

/** Put the popover above the selection (below it when there is no room), inside the window. */
function place(range) {
  const frame = range.startContainer.ownerDocument?.defaultView?.frameElement?.getBoundingClientRect()
  const rects = [...range.getClientRects()]
  const r = rects[0] ?? range.getBoundingClientRect()
  const last = rects[rects.length - 1] ?? r
  const ox = frame?.left ?? 0
  const oy = frame?.top ?? 0
  const w = pop.offsetWidth
  const h = pop.offsetHeight
  let left = ox + r.left
  let top = oy + r.top - h - 8
  if (top < 8) top = oy + last.bottom + 8
  left = Math.max(8, Math.min(left, innerWidth - w - 8))
  top = Math.max(8, Math.min(top, innerHeight - h - 8))
  pop.style.left = `${left}px`
  pop.style.top = `${top}px`
}

async function onPopoverClick(e) {
  e.stopPropagation()
  if (!target) return
  const hl = e.target.closest('[data-hl]')?.dataset.hl
  if (hl) {
    await save({ color: hl })
    return hidePopover()
  }
  switch (e.target.closest('[data-pop]')?.dataset.pop) {
    case 'note': {
      const form = pop.querySelector('[data-pop-note]')
      form.hidden = false
      form.onsubmit = async ev => {
        ev.preventDefault()
        await save({ note: form.querySelector('textarea').value })
        hidePopover()
      }
      form.querySelector('textarea').focus()
      return
    }
    case 'cancel-note': return hidePopover()
    case 'copy':
      await copyText(target.text)
      return hidePopover()
    case 'define': {
      const word = target.text
      hidePopover()
      return hooks.onDefine?.(word)
    }
    case 'delete':
      await remove(target.cfi)
      return hidePopover()
  }
}

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text)
  } catch (err) {
    console.error('copy', err)
  }
}

/** Create or update the annotation the popover is on. */
async function save(changes) {
  const old = find(target.cfi)
  const a = {
    cfi: target.cfi,
    text: old?.text ?? target.text,
    color: old?.color ?? DEFAULT_COLOR,
    note: old?.note ?? '',
    label: old?.label ?? (cleanLabel(R.position?.tocItem?.label) || R.info?.title || ''),
    fraction: old?.fraction ?? R.position?.fraction ?? 0,
    created_at: old?.created_at ?? 0,
    updated_at: 0,
    ...changes,
  }
  try {
    R.annotations = await api.readerSaveAnnotation(R.repo, a)
    draw(find(a.cfi) ?? a)
    clearSelection()
    hooks.onChange?.()
  } catch (err) {
    console.error('save annotation', err)
  }
}

async function remove(cfi) {
  try {
    R.annotations = await api.readerRemoveAnnotation(R.repo, cfi)
    R.view?.deleteAnnotation({ value: cfi }).catch(err => console.error(err))
    hooks.onChange?.()
  } catch (err) {
    console.error('remove annotation', err)
  }
}

function clearSelection() {
  for (const { doc } of R.view?.renderer?.getContents?.() ?? []) doc.getSelection()?.removeAllRanges()
}

// --- Panel ---

export function annotationsHtml() {
  const list = R.annotations
  let lastLabel = null
  return `
    <div class="flex items-center justify-between">
      <h3 class="flex items-center gap-1.5 font-display text-headline-sm text-on-surface">${icon('ink_highlighter', 'text-[18px] text-primary')}Annotations</h3>
      <span class="text-label-sm text-outline">${list.length}</span>
    </div>
    ${list.length ? `
      <button data-ann="export" class="btn-glass self-start" title="Save as Markdown next to the epub">${icon('download', 'text-[16px]')}Export Markdown</button>
      ${list.map((a, i) => {
        const heading = a.label !== lastLabel ? `<div class="eyebrow truncate mt-1">${esc(a.label || 'Untitled section')}</div>` : ''
        lastLabel = a.label
        return `${heading}
        <div class="group flex flex-col gap-1 p-2 rounded-lg bg-surface-container hover:bg-surface-container-high border-l-[3px]" style="border-color:${COLORS[a.color]?.css ?? COLORS[DEFAULT_COLOR].css}">
          <button data-ann-go="${i}" class="text-left text-body-sm text-on-surface leading-snug line-clamp-4">${esc(a.text)}</button>
          ${editing === a.cfi ? `
            <form data-ann-form="${i}" class="flex flex-col gap-1.5">
              <textarea rows="3" class="field w-full text-body-sm resize-y" placeholder="Write a note…">${esc(a.note)}</textarea>
              <div class="flex justify-end gap-1.5">
                <button type="button" data-ann="cancel" class="btn-glass">Cancel</button>
                <button type="submit" class="btn-gold">Save</button>
              </div>
            </form>`
          : a.note ? `<p class="text-body-sm text-on-surface-variant italic whitespace-pre-line">${esc(a.note)}</p>` : ''}
          <div class="flex items-center justify-between text-[10px] text-outline">
            <span>${Math.floor(a.fraction * 100)}% · ${ago(a.updated_at || a.created_at)}</span>
            <span class="flex gap-1 opacity-0 group-hover:opacity-100">
              <button data-ann-edit="${i}" class="hover:text-primary" title="${a.note ? 'Edit note' : 'Add a note'}">${icon('edit_note', 'text-[16px]')}</button>
              <button data-ann-del="${i}" class="hover:text-error" title="Delete">${icon('delete', 'text-[16px]')}</button>
            </span>
          </div>
        </div>`
      }).join('')}`
    : `<p class="text-body-sm text-outline">Select text on the page to highlight it or add a note. Highlights are saved with your reading progress.</p>`}`
}

/** Returns 'rerender' after a change, true when handled. */
export async function handleAnnotationsClick(e) {
  const go = e.target.closest('[data-ann-go]')
  if (go) {
    const a = R.annotations[+go.dataset.annGo]
    if (a) R.view.goTo(a.cfi).catch(err => console.error(err))
    return true
  }
  const edit = e.target.closest('[data-ann-edit]')
  if (edit) {
    editing = R.annotations[+edit.dataset.annEdit]?.cfi ?? null
    return 'rerender'
  }
  const del = e.target.closest('[data-ann-del]')
  if (del) {
    const a = R.annotations[+del.dataset.annDel]
    if (a) await remove(a.cfi)
    return 'rerender'
  }
  switch (e.target.closest('[data-ann]')?.dataset.ann) {
    case 'cancel':
      editing = null
      return 'rerender'
    case 'export':
      try {
        await api.readerExportAnnotations(R.repo)
      } catch (err) {
        console.error('export annotations', err)
      }
      return true
  }
  return false
}

/** Note form submitted in the panel. Returns true when it was one of ours. */
export async function handleAnnotationsSubmit(e) {
  const form = e.target.closest('[data-ann-form]')
  if (!form) return false
  e.preventDefault()
  const a = R.annotations[+form.dataset.annForm]
  editing = null
  if (a) {
    target = { cfi: a.cfi, text: a.text, existing: true }
    await save({ note: form.querySelector('textarea').value })
    target = null
  }
  return true
}

export function resetAnnotations() {
  hidePopover()
  editing = null
}
