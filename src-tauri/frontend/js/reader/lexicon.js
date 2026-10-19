// Michael Burgus — https://github.com/NeuralDrifter
// Dictionary card: Wiktionary definition of a word from the page, where it was seen, encyclopedia link

import { api } from '../api.js'
import { R, cleanLabel } from './ctx.js'
import { esc, icon } from '../util.js'

let word = ''
let seenIn = ''
let state = 'idle'   // idle | loading | found | missing | error
let entry = null
let error = ''
let run = 0

/** Look a word up; onUpdate() is called whenever the card needs repainting. */
export async function define(raw, onUpdate) {
  word = raw.replace(/\s+/g, ' ').trim()
  if (!word) return
  seenIn = cleanLabel(R.position?.tocItem?.label)
  state = 'loading'
  entry = null
  const mine = ++run
  onUpdate()
  try {
    entry = await api.dictionaryLookup(word)
    if (mine !== run) return
    state = entry ? 'found' : 'missing'
    if (entry) word = entry.word
  } catch (err) {
    if (mine !== run) return
    state = 'error'
    error = String(err)
  }
  onUpdate()
}

export function resetLexicon() {
  run++
  word = ''
  state = 'idle'
  entry = null
}

const wikipedia = w => `https://en.wikipedia.org/w/index.php?search=${encodeURIComponent(w)}`
const wiktionary = w => `https://en.wiktionary.org/wiki/${encodeURIComponent(w)}#English`

export function lexiconHtml() {
  const head = `
    <div class="flex items-center justify-between">
      <span class="eyebrow">Archival Lexicon</span>
      ${icon('dictionary', 'text-[18px] text-primary')}
    </div>`
  if (state === 'idle')
    return `${head}<p class="text-body-sm text-outline">Double-click a word on the page, or select it and choose Define, to look it up.</p>`

  const links = `
    <div class="flex flex-col gap-1.5 pt-space-sm border-t border-outline-variant/40">
      ${seenIn ? `<div class="text-label-sm text-outline">Seen in <span class="text-on-surface-variant">${esc(seenIn)}</span></div>` : ''}
      <div class="flex flex-wrap gap-1.5">
        <button data-lex-url="${esc(wikipedia(word))}" class="btn-glass">${icon('travel_explore', 'text-[16px]')}Explore Encyclopedia</button>
        ${state === 'found' ? `<button data-lex-url="${esc(wiktionary(word))}" class="btn-glass">${icon('open_in_new', 'text-[16px]')}Wiktionary</button>` : ''}
      </div>
    </div>`
  const title = `<h3 class="font-display text-headline-md text-on-surface break-words">${esc(word)}</h3>`

  if (state === 'loading') return `${head}${title}<p class="text-body-sm text-outline">Looking it up…</p>`
  if (state === 'missing') return `${head}${title}<p class="text-body-sm text-outline">Wiktionary has no English entry for this word.</p>${links}`
  if (state === 'error')
    return `${head}${title}<p class="text-body-sm text-outline">The dictionary could not be reached. Words you have looked up before still work offline.</p>
      <p class="text-[10px] text-outline break-words">${esc(error)}</p>${links}`

  return `${head}${title}
    ${entry.root ? `<button data-lex-word="${esc(entry.root)}" class="self-start text-label-sm text-primary hover:underline">${icon('subdirectory_arrow_right', 'text-[14px] align-middle')} See ${esc(entry.root)}</button>` : ''}
    ${entry.parts.map(p => `
      <div class="flex flex-col gap-1">
        <div class="text-label-sm italic text-primary">${esc(p.part)}</div>
        <ol class="flex flex-col gap-1.5 list-decimal pl-5 text-body-sm text-on-surface marker:text-outline">
          ${p.senses.map(s => `
            <li>${esc(s.text)}${s.example ? `<div class="mt-0.5 text-on-surface-variant italic">“${esc(s.example)}”</div>` : ''}</li>`).join('')}
        </ol>
      </div>`).join('')}
    ${links}`
}

/** Returns true when the click was on the card. */
export function handleLexiconClick(e, onUpdate) {
  const url = e.target.closest('[data-lex-url]')?.dataset.lexUrl
  if (url) {
    api.openUrl(url).catch(err => console.error(err))
    return true
  }
  const w = e.target.closest('[data-lex-word]')?.dataset.lexWord
  if (w) {
    define(w, onUpdate)
    return true
  }
  return false
}
