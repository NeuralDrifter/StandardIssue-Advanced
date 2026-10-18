// Michael Burgus — https://github.com/NeuralDrifter
// Every backend call and event subscription, in one place

const { invoke } = window.__TAURI__.core
const { listen } = window.__TAURI__.event

export const api = {
  instanceStatus: () => invoke('instance_status'),
  queryBooks: query => invoke('query_books', { query }),
  getBook: repo => invoke('get_book', { repo }),
  liveStates: repos => invoke('live_states', { repos }),
  overview: () => invoke('get_overview'),
  diskUsage: () => invoke('get_disk_usage'),

  buildBooks: (repos, { metadata, covers, force = false }) =>
    invoke('build_books', { repos, metadata, covers, force }),
  buildAll: () => invoke('build_all'),
  cancelBuilds: () => invoke('cancel_builds'),
  buildLog: repo => invoke('get_build_log', { repo }),
  buildRecords: limit => invoke('list_build_records', { limit }),
  opLog: () => invoke('get_op_log'),

  getSettings: () => invoke('get_settings'),
  saveSettings: settings => invoke('save_settings', { settings }),

  syncCatalog: () => invoke('sync_catalog'),
  enrichAll: () => invoke('enrich_all'),
  fetchCovers: () => invoke('fetch_covers'),
  cancelOperation: () => invoke('cancel_operation'),

  openEpub: repo => invoke('open_epub', { repo }),
  revealEpub: repo => invoke('reveal_epub', { repo }),
  openLibraryFolder: () => invoke('open_library_folder'),
  openUrl: url => invoke('plugin:opener|open_url', { url }),
  readingProgress: () => invoke('reading_progress_all'),
}

export const events = {
  buildUpdate: fn => listen('build-update', e => fn(e.payload)),
  buildSummary: fn => listen('build-summary', e => fn(e.payload)),
  opProgress: fn => listen('op-progress', e => fn(e.payload)),
  opDone: fn => listen('op-done', e => fn(e.payload)),
  opError: fn => listen('op-error', e => fn(e.payload)),
}
