// Michael Burgus — https://github.com/NeuralDrifter
// Shared state for the reader's parts (one book open at a time)

export const R = {
  el: null,          // #reader
  view: null,        // <foliate-view>
  book: null,        // foliate book
  info: null,        // reader_open response
  repo: null,
  settings: null,    // ReaderSettings
  position: null,    // last foliate relocate detail
  fractions: [],     // section start fractions (length = sections + 1)
  bookmarks: [],
  annotations: [],   // highlights and notes, in reading order
  pace: { wpm: 250, measured: false },
  get wpm() { return this.pace.wpm },
  panel: null,       // right panel: 'typeset' | 'search' | 'bookmarks' | 'annotations' | null
  backCfi: null,     // where to return after following an internal link
}

export const rq = sel => R.el.querySelector(sel)
export const rqa = sel => [...R.el.querySelectorAll(sel)]

/** Words in the book, or 0 when metadata has not been enriched yet. */
export const bookWords = () => R.info?.word_count ?? 0

/** Index of the section the reader is in. */
export const currentSection = () => R.position?.section?.current ?? -1

/** Section-relative progress (0..1) of a book fraction inside section `i`. */
export function fractionInSection(i, fraction) {
  const start = R.fractions[i] ?? 0
  const end = R.fractions[i + 1] ?? 1
  return end > start ? Math.min(1, Math.max(0, (fraction - start) / (end - start))) : 0
}

/** Estimated words in section `i` (its share of the book's bytes × the word count). */
export const sectionWords = i =>
  bookWords() * Math.max(0, (R.fractions[i + 1] ?? 0) - (R.fractions[i] ?? 0))

/** "5h 12m" / "18m" / "<1m" */
export function duration(minutes) {
  if (!Number.isFinite(minutes) || minutes < 0) return ''
  if (minutes < 1) return '<1m'
  const m = Math.round(minutes)
  const h = Math.floor(m / 60)
  return h ? `${h}h ${m % 60}m` : `${m}m`
}

/** "Chapter I: There Are Heroisms…" → kept; whitespace collapsed. */
export const cleanLabel = label => (label ?? '').replace(/\s+/g, ' ').trim()
