// Michael Burgus — https://github.com/NeuralDrifter
// App state with topic subscriptions. Components re-render from here;
// backend events update state, never the DOM directly.

export const state = {
  settings: null,
  overview: null,
  disk: null,
  /** Filters and paging sent to query_books */
  query: { search: '', author: '', genre: '', era: '', status: 'all', sort: 'title', page: 1 },
  /** Last query_books response */
  result: null,
  /** 'library' or a page: 'sync' | 'enrich' | 'logs' | 'settings' */
  route: 'library',
  /** Repo shown in the inspector */
  inspected: null,
  inspectorOpen: true,
  /** Repos ticked for Build Selected */
  selection: new Set(),
  /** repo -> { state, pct, phase, queue_pos, error } for books in the pipeline */
  live: {},
  queue: { queued: 0, active: 0, done: 0, failed: 0, running: false, active_list: [], next: [] },
  /** Running sync/enrich/covers: { name, message, current, total } */
  op: null,
  /** Last notice for the pipeline strip: { kind: 'info'|'error', text, at } */
  notice: null,
  reading: {},
  version: '',
  coverBust: '',
}

const subs = new Map()

/** Subscribe to one or more topics. */
export function on(topics, fn) {
  for (const t of [].concat(topics)) {
    if (!subs.has(t)) subs.set(t, new Set())
    subs.get(t).add(fn)
  }
}

/** Notify subscribers of the given topics (each listener runs once). */
export function emit(...topics) {
  const fns = new Set()
  for (const t of topics) for (const fn of subs.get(t) ?? []) fns.add(fn)
  for (const fn of fns) {
    try {
      fn()
    } catch (err) {
      console.error(`render failed for ${topics.join(',')}`, err)
    }
  }
}

export function notify(text, kind = 'info') {
  state.notice = { kind, text, at: Date.now() }
  emit('notice')
}
