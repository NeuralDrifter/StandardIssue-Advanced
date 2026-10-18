// Michael Burgus — https://github.com/NeuralDrifter
// Measures reading speed from ordinary forward reading; jumps, skimming and idle time are ignored

import { api } from '../api.js'
import { R, bookWords } from './ctx.js'

const MIN_SPAN_MS = 20_000        // count a stretch once it covers at least this long
const IDLE_MS = 10 * 60_000       // longer than this between moves = away from the book
const MAX_STEP = 0.02             // a forward move bigger than this is a jump, not reading
const PLAUSIBLE_WPM = [60, 1200]  // anything outside this is skimming or a stalled page
const FLUSH_MS = 2 * 60_000

let anchor = null   // { fraction, at } start of the stretch being timed
let words = 0       // measured but not yet sent
let seconds = 0
let flushTimer = null

export function startPace() {
  anchor = null
  words = 0
  seconds = 0
  clearInterval(flushTimer)
  flushTimer = setInterval(flushPace, FLUSH_MS)
}

/** Call on every relocation with the new book fraction. */
export function trackPace(fraction) {
  const now = Date.now()
  const total = bookWords()
  if (!total || document.hidden) {
    anchor = null
    return
  }
  if (!anchor) {
    anchor = { fraction, at: now }
    return
  }
  const step = fraction - anchor.fraction
  const ms = now - anchor.at
  if (step < 0 || step > MAX_STEP || ms > IDLE_MS) {
    anchor = { fraction, at: now }
    return
  }
  if (ms < MIN_SPAN_MS) return // keep extending this stretch

  const w = step * total
  const wpm = w / (ms / 60_000)
  if (wpm >= PLAUSIBLE_WPM[0] && wpm <= PLAUSIBLE_WPM[1]) {
    words += w
    seconds += ms / 1000
  }
  anchor = { fraction, at: now }
}

export async function flushPace() {
  if (words <= 0 || seconds <= 0) return
  const [w, s] = [words, seconds]
  words = 0
  seconds = 0
  try {
    R.pace = await api.readerRecordPace(w, s)
  } catch (err) {
    console.error('could not record reading pace', err)
  }
}

export async function stopPace() {
  clearInterval(flushTimer)
  flushTimer = null
  anchor = null
  await flushPace()
}
