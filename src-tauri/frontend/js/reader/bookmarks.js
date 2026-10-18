// Michael Burgus — https://github.com/NeuralDrifter
// Bookmarks: toggle the current page, list and jump, remove

import { api } from '../api.js'
import { collapse, compare } from '../../vendor/foliate-js/epubcfi.js'
import { R, cleanLabel } from './ctx.js'
import { esc, icon, ago } from '../util.js'

/** The bookmark that falls on the visible page, if any. */
export function bookmarkHere() {
  const cfi = R.position?.cfi
  if (!cfi) return null
  try {
    const start = collapse(cfi)
    const end = collapse(cfi, true)
    return R.bookmarks.find(b => compare(b.cfi, start) >= 0 && compare(b.cfi, end) <= 0) ?? null
  } catch {
    return R.bookmarks.find(b => b.cfi === cfi) ?? null
  }
}

/** Add a bookmark at the top of the visible page, or remove the one already there. */
export async function toggleBookmark() {
  if (!R.position?.cfi) return
  const here = bookmarkHere()
  try {
    R.bookmarks = here
      ? await api.readerRemoveBookmark(R.repo, here.cfi)
      : await api.readerAddBookmark(R.repo, {
          cfi: collapse(R.position.cfi),
          label: cleanLabel(R.position.tocItem?.label) || R.info.title,
          fraction: R.position.fraction ?? 0,
          created_at: 0,
        })
  } catch (err) {
    console.error('bookmark', err)
  }
}

export function bookmarksHtml() {
  const list = R.bookmarks
  return `
    <div class="flex items-center justify-between">
      <h3 class="flex items-center gap-1.5 font-display text-headline-sm text-on-surface">${icon('bookmarks', 'text-[18px] text-primary')}Bookmarks</h3>
      <span class="text-label-sm text-outline">${list.length}</span>
    </div>
    ${list.length ? list.map((b, i) => `
      <div class="group flex items-start gap-2 p-2 rounded-lg bg-surface-container hover:bg-surface-container-high">
        <button data-mark="${i}" class="flex-1 min-w-0 text-left">
          <div class="text-body-sm text-on-surface truncate">${esc(b.label)}</div>
          <div class="text-[10px] text-outline">${Math.floor(b.fraction * 100)}% · added ${ago(b.created_at)}</div>
        </button>
        <button data-unmark="${i}" class="opacity-0 group-hover:opacity-100 text-outline hover:text-error" title="Remove bookmark">${icon('close', 'text-[16px]')}</button>
      </div>`).join('') : `
      <p class="text-body-sm text-outline">No bookmarks yet. Press ${icon('bookmark_add', 'text-[14px] align-middle')} or <kbd class="font-mono text-label-sm">B</kbd> to mark the page you are on.</p>`}`
}

/** Returns 'rerender' after a change to the list, true when a jump was made. */
export async function handleBookmarksClick(e) {
  const go = e.target.closest('[data-mark]')
  if (go) {
    const b = R.bookmarks[+go.dataset.mark]
    if (b) R.view.goTo(b.cfi).catch(err => console.error(err))
    return true
  }
  const del = e.target.closest('[data-unmark]')
  if (del) {
    const b = R.bookmarks[+del.dataset.unmark]
    if (b) {
      try {
        R.bookmarks = await api.readerRemoveBookmark(R.repo, b.cfi)
      } catch (err) {
        console.error('remove bookmark', err)
      }
    }
    return 'rerender'
  }
  return false
}
