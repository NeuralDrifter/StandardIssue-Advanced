// Michael Burgus — https://github.com/NeuralDrifter
// Operations pages: GitHub Sync Status, Metadata Enrichment, Build Logs, Settings

import { api } from './api.js'
import { state, on, emit, notify } from './state.js'
import { runOp, setRoute, inspect, refreshQuery, refreshOverview } from './actions.js'
import { $, esc, num, ago, dateTime, icon } from './util.js'

const TITLES = {
  sync: ['sync_alt', 'GitHub Sync Status', 'The catalog comes from the Standard Ebooks organization on GitHub. A sync is all-or-nothing: it fetches the full list of ebook repositories and merges it with what you already have.'],
  enrich: ['auto_fix_high', 'Metadata Enrichment', 'Each book’s content.opf gives genres, word counts and descriptions; its colophon gives the original publication date and the cover painting’s credit.'],
  logs: ['receipt_long', 'Archival Build Logs', 'Every epub build keeps its last log. Operations (sync, enrichment, artwork) are listed separately.'],
  settings: ['settings', 'Settings', 'Stored in settings.json in the app data folder.'],
}

let logsTab = 'builds'

export function initPages() {
  on('route', show)
  on(['overview', 'op'], () => {
    if (state.route === 'sync' || state.route === 'enrich') render()
  })
  on('settings', () => {
    if (state.route === 'settings') render()
  })
  $('#page-view').addEventListener('click', onClick)
  $('#page-view').addEventListener('change', onChange)
  show()
}

function show() {
  const library = state.route === 'library'
  $('#library-view').hidden = !library
  $('#page-view').hidden = library
  if (!library) {
    $('#main').scrollTo({ top: 0 })
    render()
  }
}

function header(route) {
  const [i, title, blurb] = TITLES[route]
  return `
    <div class="flex items-start justify-between gap-space-md mb-space-lg">
      <div class="flex items-start gap-space-sm">
        <div class="w-10 h-10 rounded-xl bg-surface-container-low grid place-items-center text-primary shrink-0">${icon(i, 'text-[22px]')}</div>
        <div>
          <h1 class="font-display text-headline-lg">${title}</h1>
          <p class="text-body-sm text-outline max-w-2xl">${blurb}</p>
        </div>
      </div>
      <button data-back class="btn-glass">${icon('arrow_back', 'text-[16px]')}Back to shelves</button>
    </div>`
}

const card = (body, cls = '') => `<div class="bg-surface-container-low rounded-xl p-space-md shadow-md ${cls}">${body}</div>`
const stat = (value, label, tone = 'text-primary') => `
  <div class="flex flex-col"><span class="font-display text-headline-lg ${tone} font-bold">${value}</span><span class="eyebrow">${label}</span></div>`

async function render() {
  const route = state.route
  const el = $('#page-view')
  if (route === 'sync') el.innerHTML = header(route) + syncPage()
  else if (route === 'enrich') el.innerHTML = header(route) + enrichPage()
  else if (route === 'logs') {
    el.innerHTML = header(route) + `<div id="logs-body">${card('<div class="text-outline">Loading…</div>')}</div>`
    renderLogs()
  } else if (route === 'settings') el.innerHTML = header(route) + settingsPage()
}

// --- Sync ---

function opRunning(name) {
  return state.op?.name === name
    ? `<div class="flex items-center gap-2 text-body-sm text-secondary">${icon('progress_activity', 'text-[16px] animate-spin')}${esc(state.op.message ?? 'Running…')}${state.op.total ? ` (${num(state.op.current)}/${num(state.op.total)})` : ''}</div>`
    : ''
}

function syncPage() {
  const o = state.overview
  const last = o?.activity?.last_sync
  const s = last?.summary ?? {}
  return `<div class="grid grid-cols-1 lg:grid-cols-3 gap-gutter">
    ${card(`
      <div class="flex items-center justify-between mb-space-md">
        <div class="eyebrow">Last sync</div>
        <button data-op="sync" class="btn-gold" ${state.op ? 'disabled' : ''}>${icon('sync', 'text-[16px]')}Sync now</button>
      </div>
      ${last ? `
        <div class="text-headline-sm font-display mb-1">${ago(last.at)}</div>
        <div class="text-body-sm text-outline mb-space-md">${esc(dateTime(last.at))}</div>
        <div class="flex gap-space-lg">${stat(num(s.total), 'Books')}${stat(num(s.new), 'New', 'text-tertiary')}${stat(num(s.removed), 'Removed', 'text-secondary')}</div>
        ${s.rate_remaining != null ? `<div class="mt-space-md text-body-sm text-outline">GitHub API requests left this hour: <b class="text-on-surface">${num(s.rate_remaining)}</b>${state.settings?.github_token ? ' (with your token)' : ' — add a token in Settings for a higher limit'}</div>` : ''}`
      : `<div class="text-body-sm text-outline">The catalog has never been synced on this computer.</div>`}
      <div class="mt-space-md">${opRunning('Catalog sync')}</div>`, 'lg:col-span-2')}
    ${card(`
      <div class="eyebrow mb-space-sm">Library</div>
      <div class="flex flex-col gap-space-md">${stat(num(o?.total), 'In catalog')}${stat(num(o?.built), 'Built', 'text-tertiary')}</div>`)}
  </div>`
}

// --- Enrichment ---

function enrichPage() {
  const o = state.overview
  if (!o) return ''
  const bar = (n, label, tone) => {
    const pct = o.total ? (n / o.total) * 100 : 0
    return `<div class="flex flex-col gap-1">
      <div class="flex justify-between text-body-sm"><span>${label}</span><span class="text-outline">${num(n)} / ${num(o.total)} · ${num(o.total - n)} missing</span></div>
      <div class="h-1.5 rounded-full bg-surface-container-highest overflow-hidden"><div class="h-full ${tone}" style="width:${pct}%"></div></div>
    </div>`
  }
  const le = o.activity?.last_enrich
  const lc = o.activity?.last_covers
  return `<div class="grid grid-cols-1 lg:grid-cols-3 gap-gutter">
    ${card(`
      <div class="flex items-center justify-between mb-space-md">
        <div class="eyebrow">Coverage</div>
        <div class="flex gap-space-xs">
          <button data-op="enrich" class="btn-gold" ${state.op ? 'disabled' : ''}>${icon('auto_fix_high', 'text-[16px]')}Enrich metadata</button>
          <button data-op="covers" class="btn-glass" ${state.op ? 'disabled' : ''}>${icon('palette', 'text-[16px] text-tertiary')}Fetch artwork</button>
        </div>
      </div>
      <div class="flex flex-col gap-space-md">
        ${bar(o.with_metadata, 'Metadata (genres, word counts, descriptions)', 'bg-primary')}
        ${bar(o.with_colophon, 'Colophon (publication date, artwork credit)', 'bg-secondary')}
        ${bar(o.with_covers, 'Cover artwork', 'bg-tertiary')}
      </div>
      <div class="mt-space-md">${opRunning('Metadata enrichment')}${opRunning('Cover fetch')}</div>`, 'lg:col-span-2')}
    ${card(`
      <div class="eyebrow mb-space-sm">Last runs</div>
      <div class="flex flex-col gap-space-md text-body-sm">
        <div><div class="text-on-surface">Enrichment · ${le ? ago(le.at) : 'never'}</div>
          ${le ? `<div class="text-outline">${num(le.summary.enriched)} enriched, ${num(le.summary.skipped)} already current, ${num(le.summary.errors)} errors${le.summary.refiled ? `, ${num(le.summary.refiled)} re-filed` : ''}</div>` : ''}</div>
        <div><div class="text-on-surface">Artwork · ${lc ? ago(lc.at) : 'never'}</div>
          ${lc ? `<div class="text-outline">${num(lc.summary.fetched)} fetched, ${num(lc.summary.cached)} cached, ${num(lc.summary.errors)} errors</div>` : ''}</div>
      </div>`)}
  </div>`
}

// --- Logs ---

async function renderLogs() {
  const tabs = `
    <div class="flex gap-1.5 mb-space-md">
      ${[['builds', 'Builds'], ['ops', 'Operations']].map(([k, l]) => `<button data-logs-tab="${k}" class="px-space-md py-1 rounded-full text-label-sm ${logsTab === k ? 'bg-primary-container text-on-primary-container font-bold' : 'bg-surface-container text-on-surface-variant hover:text-on-surface'}">${l}</button>`).join('')}
    </div>`
  try {
    let body
    if (logsTab === 'builds') {
      const rows = await api.buildRecords(300)
      body = rows.length
        ? `<table class="w-full text-body-sm"><thead><tr class="text-left eyebrow border-b border-surface-container-highest">
            <th class="py-2">When</th><th>Book</th><th>Status</th><th class="text-right">Duration</th><th class="pl-3">Commit</th></tr></thead><tbody>
            ${rows.map(r => `<tr data-log-repo="${esc(r.repo)}" class="border-b border-surface-container-highest/40 hover:bg-surface-container cursor-pointer">
              <td class="py-1.5 text-outline whitespace-nowrap pr-3">${esc(dateTime(r.built_at))}</td>
              <td class="py-1.5 pr-3">${esc(r.title)}</td>
              <td class="py-1.5 pr-3">${r.status === 'built' ? '<span class="text-tertiary">built</span>' : `<span class="text-error" title="${esc(r.error)}">failed</span>`}</td>
              <td class="py-1.5 text-right text-outline">${r.duration_ms ? `${(r.duration_ms / 1000).toFixed(1)}s` : ''}</td>
              <td class="py-1.5 pl-3 font-mono text-primary">${esc(r.sha.slice(0, 7))}</td></tr>`).join('')}
          </tbody></table>
          <pre id="log-view" class="mt-space-md bg-surface-container-lowest rounded-xl p-space-sm font-mono text-[11px] text-on-surface-variant whitespace-pre-wrap max-h-96 overflow-auto selectable" hidden></pre>`
        : '<div class="text-outline text-body-sm">No builds yet.</div>'
    } else {
      const lines = await api.opLog()
      body = lines.length
        ? `<pre class="bg-surface-container-lowest rounded-xl p-space-sm font-mono text-[11px] text-on-surface-variant whitespace-pre-wrap max-h-[60vh] overflow-auto selectable">${lines.map(l => {
            const [ts, ...rest] = l.split(' ')
            return `<span class="text-outline">${esc(new Date(Number(ts) * 1000).toLocaleTimeString())}</span> ${esc(rest.join(' '))}`
          }).reverse().join('\n')}</pre>`
        : '<div class="text-outline text-body-sm">Nothing logged in this session yet.</div>'
    }
    $('#logs-body').innerHTML = tabs + card(body)
  } catch (err) {
    $('#logs-body').innerHTML = card(`<div class="text-error">Could not load logs: ${esc(err)}</div>`)
  }
}

// --- Settings ---

function settingsPage() {
  const s = state.settings
  if (!s) return ''
  const sel = (key, options) => `<select data-setting="${key}" class="field w-auto">${options.map(([v, l]) => `<option value="${v}" ${String(s[key]) === String(v) ? 'selected' : ''}>${l}</option>`).join('')}</select>`
  const sw = (key) => `<button type="button" role="switch" data-setting-toggle="${key}" aria-checked="${!!s[key]}" class="toggle"></button>`
  const row = (label, help, control) => `
    <div class="flex items-center justify-between gap-space-lg py-space-sm border-b border-surface-container-highest/40 last:border-0">
      <div><div class="text-body-md text-on-surface">${label}</div><div class="text-body-sm text-outline max-w-xl">${help}</div></div>
      <div class="shrink-0">${control}</div>
    </div>`
  const range = (a, b) => Array.from({ length: b - a + 1 }, (_, i) => [a + i, a + i])
  return `<div class="flex flex-col gap-gutter max-w-4xl">
    ${card(`<div class="eyebrow mb-space-sm">Library</div>
      ${row('Library folder', esc(state.disk?.library_dir ?? ''), `<button data-open-library class="btn-glass">${icon('folder_open', 'text-[16px]')}Open folder</button>`)}
      ${row('Organize library by genre', 'Files epubs as Genre / Author / Title instead of under Uncategorized. The simple edition of StandardIssue needs its October 2026 update (or later) to find books filed this way.', sw('organize_by_genre'))}`)}
    ${card(`<div class="eyebrow mb-space-sm">Builds</div>
      ${row('Parallel builds', 'How many books are cloned and packaged at once.', sel('build_concurrency', range(1, 6)))}
      ${row('Fetch metadata when building', 'Also read each book’s colophon (publication date, artwork credit). Genres are always fetched — they decide the folder.', sw('build_metadata'))}
      ${row('Fetch cover art when building', 'Download the cover thumbnail for books that don’t have one yet.', sw('build_covers'))}
      ${row('Also make a Kobo KEPUB', 'After each build, save a .kepub.epub beside the epub. Build All also fills in books that are already built.', sw('make_kepub'))}
      ${row('Also make a Kindle AZW3', 'After each build, convert to .azw3 with Calibre (calibre-ebook.com), when it is installed. Takes a few seconds per book.', sw('make_azw3'))}`)}
    ${card(`<div class="eyebrow mb-space-sm">Catalog &amp; metadata</div>
      ${row('Parallel metadata requests', 'Used by Enrich Metadata.', sel('enrich_concurrency', [[5, 5], [10, 10], [15, 15], [20, 20]]))}
      ${row('GitHub token', 'Optional. Raises the GitHub API limit for catalog sync from 60 to 5,000 requests an hour. Needs no scopes.',
        `<input data-setting="github_token" type="password" class="field w-64" placeholder="ghp_…" value="${esc(s.github_token)}" autocomplete="off">`)}`)}
    ${card(`<div class="eyebrow mb-space-sm">Display</div>
      ${row('Books per page', '', sel('page_size', [[24, 24], [48, 48], [96, 96]]))}
      ${row('Shelf view', '', sel('view', [['grid', 'Cover grid'], ['compact', 'Compact covers'], ['list', 'Archival table']]))}`)}
  </div>`
}

async function saveSetting(key, value) {
  try {
    const before = state.settings
    state.settings = await api.saveSettings({ ...before, [key]: value })
    emit('settings')
    if (key === 'page_size' || key === 'view') refreshQuery()
    if (key === 'organize_by_genre' && value) {
      notify('Library re-filed into genre folders.')
      refreshQuery()
      refreshOverview()
    }
  } catch (err) {
    notify(`Could not save settings: ${err}`, 'error')
  }
}

// --- Events ---

function onClick(e) {
  if (e.target.closest('[data-back]')) return setRoute('library')
  const op = e.target.closest('[data-op]')
  if (op) return runOp(op.dataset.op)
  if (e.target.closest('[data-open-library]')) return api.openLibraryFolder()
  const t = e.target.closest('[data-setting-toggle]')
  if (t) return saveSetting(t.dataset.settingToggle, t.getAttribute('aria-checked') !== 'true')
  const tab = e.target.closest('[data-logs-tab]')
  if (tab) {
    logsTab = tab.dataset.logsTab
    return renderLogs()
  }
  const logRow = e.target.closest('[data-log-repo]')
  if (logRow) showLog(logRow.dataset.logRepo, e.detail === 2)
}

async function showLog(repo, openInInspector) {
  if (openInInspector) {
    setRoute('library')
    inspect(repo)
    return
  }
  const pre = $('#log-view')
  pre.hidden = false
  pre.textContent = (await api.buildLog(repo)) || 'No log file for this build.'
  pre.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
}

function onChange(e) {
  const el = e.target.closest('[data-setting]')
  if (!el) return
  const key = el.dataset.setting
  const numeric = ['build_concurrency', 'enrich_concurrency', 'page_size'].includes(key)
  saveSetting(key, numeric ? Number(el.value) : el.value)
}
