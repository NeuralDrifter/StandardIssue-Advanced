// Michael Burgus — https://github.com/NeuralDrifter
// Pure Tauri v2 Rust backend for StandardIssue (Advanced)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use ebook_manager::books::{era_of, repo_name, Book, BookStore, Published};
use ebook_manager::build::{self, BuildQueue, BuildRecord, BuildRecords, Job, JobEnv, Live, QueueSummary};
use ebook_manager::catalog;
use ebook_manager::colophon::COLOPHON_REV;
use ebook_manager::covers;
use ebook_manager::enrich::{self, META_REV};
use ebook_manager::instance::{self, unix_now, Acquire, HolderInfo, InstanceLock};
use ebook_manager::library::{self, MigrationReport};
use ebook_manager::query::{self, BookFacts, Facets, Query, Status};
use ebook_manager::reader::{BookProgress, Bookmark, PaceInfo, ReaderSettings, ReadingStore};
use ebook_manager::settings::{Activity, ActivityStore, AppSettings, OpRecord, SettingsStore};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

/// Which edition this build is. Shown to the other edition while this one holds the lock.
const EDITION: &str = "Advanced";
const OP_LOG_LINES: usize = 500;
/// Per-book progress events are sent at most this often (phase changes always go out).
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// Stores that exist only once this process holds the instance lock.
struct Loaded {
    reading: ReadingStore,
    settings: SettingsStore,
    activity: ActivityStore,
    records: BuildRecords,
}

pub struct AppState {
    store: BookStore,
    operation: Mutex<Option<String>>,
    cancel: Arc<AtomicBool>,
    client: reqwest::Client,
    queue: BuildQueue,
    op_log: Mutex<VecDeque<String>>,
    /// True once this process holds the instance lock and has loaded its data.
    ready: AtomicBool,
    lock: Mutex<Option<InstanceLock>>,
    data_dir: PathBuf,
    project_dir: PathBuf,
    loaded: OnceLock<Loaded>,
}

impl AppState {
    /// Cover image path for the frontend, or "" when there is none on disk.
    fn cover_path(&self, repo: &str) -> String {
        let cover = self.store.cover_file(repo);
        if cover.exists() { cover.to_string_lossy().to_string() } else { String::new() }
    }

    fn loaded(&self) -> Result<&Loaded, String> {
        if !self.ready.load(Ordering::SeqCst) {
            return Err("not ready: another copy of StandardIssue is running".to_string());
        }
        self.loaded.get().ok_or_else(|| "state not loaded".to_string())
    }

    fn settings(&self) -> AppSettings {
        self.loaded.get().map(|l| l.settings.get()).unwrap_or_default()
    }

    fn start_op(&self, name: &str) -> Result<(), String> {
        self.loaded()?;
        let mut op = self.operation.lock().unwrap();
        if let Some(running) = op.as_ref() {
            return Err(format!("{running} is already running"));
        }
        self.cancel.store(false, Ordering::SeqCst);
        *op = Some(name.to_string());
        Ok(())
    }

    fn finish_op(&self) {
        *self.operation.lock().unwrap() = None;
    }

    fn current_op(&self) -> Option<String> {
        self.operation.lock().unwrap().clone()
    }

    fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("build-logs")
    }

    /// Keep a line in the in-memory operation log and append it to operations.log.
    fn log_op(&self, msg: impl Into<String>) {
        let line = format!("{} {}", unix_now(), msg.into());
        {
            let mut log = self.op_log.lock().unwrap();
            if log.len() == OP_LOG_LINES {
                log.pop_front();
            }
            log.push_back(line.clone());
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.data_dir.join("operations.log"))
        {
            let _ = writeln!(f, "{line}");
        }
    }

    fn status_of(&self, repo: &str, records: &BTreeMap<String, BuildRecord>) -> Status {
        match self.queue.live(repo) {
            Some(l) if l.state == "building" => Status::Building,
            Some(_) => Status::Queued,
            None if self.store.is_downloaded(repo) => Status::Built,
            None if records.get(repo).is_some_and(|r| r.status == "failed") => Status::Failed,
            None => Status::NotBuilt,
        }
    }

    fn row(&self, book: &Book, records: &BTreeMap<String, BuildRecord>) -> BookRow {
        let rn = repo_name(book);
        let epub = self.store.epub_file(&rn);
        BookRow {
            downloaded: epub.is_some(),
            epub_size: epub.and_then(|p| std::fs::metadata(p).ok()).map_or(0, |m| m.len()),
            cover_path: self.cover_path(&rn),
            era: era_of(book),
            status: self.status_of(&rn, records),
            live: self.queue.live(&rn),
            last_build: records.get(&rn).cloned(),
            book: book.clone(),
        }
    }

    fn reading_set(&self) -> HashSet<String> {
        self.loaded
            .get()
            .map(|l| query::reading_set(l.reading.all_progress().iter().map(|(r, p)| (r, p.fraction))))
            .unwrap_or_default()
    }

    fn record_activity(&self, f: impl FnOnce(&mut Activity)) {
        if let Some(l) = self.loaded.get() {
            if let Err(e) = l.activity.update(f) {
                self.log_op(format!("could not save activity.json: {e}"));
            }
        }
    }

    /// Re-file epubs into genre folders when that setting is on.
    fn relocate(&self) -> MigrationReport {
        let mut report = MigrationReport::default();
        library::relocate_epubs(&self.store, &mut report);
        if !report.is_empty() {
            self.log_op(format!(
                "Re-filed {} epubs into genre folders ({} failed)",
                report.moved,
                report.failures.len()
            ));
        }
        report
    }
}

type State<'a> = tauri::State<'a, Arc<AppState>>;

// --- Library queries ---

#[derive(Serialize)]
struct BookRow {
    #[serde(flatten)]
    book: Book,
    downloaded: bool,
    epub_size: u64,
    /// Local cover file, or "" when none is cached yet.
    cover_path: String,
    era: &'static str,
    status: Status,
    live: Option<Live>,
    last_build: Option<BuildRecord>,
}

#[derive(Serialize)]
struct QueryResponse {
    books: Vec<BookRow>,
    total: usize,
    page: usize,
    pages: usize,
    per_page: usize,
    facets: Facets,
}

#[tauri::command]
fn query_books(state: State<'_>, query: Query) -> Result<QueryResponse, String> {
    let loaded = state.loaded()?;
    let records = loaded.records.get();
    let reading = state.reading_set();
    state.store.read(|books| {
        let result = query::run(books, &query, |b| {
            let rn = repo_name(b);
            BookFacts {
                status: state.status_of(&rn, &records),
                reading: reading.contains(&rn),
                built_at: records.get(&rn).map_or(0, |r| r.built_at),
            }
        });
        Ok(QueryResponse {
            books: result.indices.iter().map(|&i| state.row(&books[i], &records)).collect(),
            total: result.total,
            page: result.page,
            pages: result.pages,
            per_page: result.per_page,
            facets: result.facets,
        })
    })
}

#[tauri::command]
fn get_book(state: State<'_>, repo: String) -> Result<Option<BookRow>, String> {
    let records = state.loaded()?.records.get();
    Ok(state.store.get(&repo).map(|b| state.row(&b, &records)))
}

#[tauri::command]
fn live_states(state: State<'_>, repos: Vec<String>) -> HashMap<String, Option<Live>> {
    repos.into_iter().map(|r| (state.queue.live(&r), r)).map(|(l, r)| (r, l)).collect()
}

#[derive(Serialize)]
struct Overview {
    total: usize,
    built: usize,
    not_built: usize,
    queued: usize,
    building: usize,
    failed: usize,
    reading: usize,
    with_metadata: usize,
    with_colophon: usize,
    with_covers: usize,
    operation: Option<String>,
    activity: Activity,
    queue: QueueSummary,
}

#[tauri::command]
fn get_overview(state: State<'_>) -> Result<Overview, String> {
    let loaded = state.loaded()?;
    let records = loaded.records.get();
    let covers: HashSet<String> = std::fs::read_dir(&state.store.covers_dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    let queue = state.queue.summary();

    let mut o = Overview {
        total: 0,
        built: 0,
        not_built: 0,
        queued: queue.queued,
        building: queue.active,
        failed: 0,
        reading: state.reading_set().len(),
        with_metadata: 0,
        with_colophon: 0,
        with_covers: 0,
        operation: state.current_op(),
        activity: loaded.activity.get(),
        queue,
    };
    state.store.read(|books| {
        for b in books {
            let rn = repo_name(b);
            o.total += 1;
            match state.status_of(&rn, &records) {
                Status::Built => o.built += 1,
                Status::Failed => o.failed += 1,
                Status::NotBuilt => o.not_built += 1,
                _ => {}
            }
            o.with_metadata += (b.meta_rev >= META_REV) as usize;
            o.with_colophon += (b.colophon_rev >= COLOPHON_REV) as usize;
            o.with_covers += covers.contains(&format!("{rn}.jpg")) as usize;
        }
    });
    Ok(o)
}

#[derive(Serialize)]
struct DiskUsage {
    library_bytes: u64,
    free_bytes: u64,
    total_bytes: u64,
    drive: String,
    library_dir: String,
}

#[tauri::command]
async fn get_disk_usage(state: State<'_>) -> Result<DiskUsage, String> {
    state.loaded()?;
    let (epubs, covers) = (state.store.epubs_dir.clone(), state.store.covers_dir.clone());
    let library = epubs.parent().unwrap_or(&epubs).to_path_buf();
    tokio::task::spawn_blocking(move || {
        let size = |dir: &Path| -> u64 {
            walkdir::WalkDir::new(dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter_map(|e| e.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        };
        let drive = library
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_default();
        DiskUsage {
            library_bytes: size(&epubs) + size(&covers),
            free_bytes: fs4::available_space(&library).unwrap_or(0),
            total_bytes: fs4::total_space(&library).unwrap_or(0),
            drive,
            library_dir: library.to_string_lossy().to_string(),
        }
    })
    .await
    .map_err(|e| format!("disk usage: {e}"))
}

// --- Builds ---

fn emit_summary(handle: &tauri::AppHandle, state: &AppState) {
    let _ = handle.emit("build-summary", state.queue.summary());
}

/// Start build workers up to the configured concurrency.
fn pump_builds(handle: &tauri::AppHandle, state: &Arc<AppState>) {
    let max = state.settings().build_concurrency as usize;
    while state.queue.try_add_worker(max) {
        let (handle, state) = (handle.clone(), Arc::clone(state));
        tauri::async_runtime::spawn(async move { build_worker(handle, state).await });
    }
    emit_summary(handle, state);
}

async fn build_worker(handle: tauri::AppHandle, state: Arc<AppState>) {
    while let Some(job) = state.queue.next_job() {
        let repo = job.repo.clone();
        let _ = handle.emit(
            "build-update",
            serde_json::json!({"repo": repo, "state": "building", "pct": 0, "phase": "Starting"}),
        );
        emit_summary(&handle, &state);

        let progress: build::Progress = {
            let (handle, state, repo) = (handle.clone(), Arc::clone(&state), repo.clone());
            let last = Mutex::new((Instant::now() - PROGRESS_EVERY, 0u8, String::new()));
            Arc::new(move |pct: u8, phase: &str| {
                state.queue.set_progress(&repo, pct, phase);
                let mut last = last.lock().unwrap();
                if (pct, phase) == (last.1, last.2.as_str()) {
                    return;
                }
                if phase != last.2 || last.0.elapsed() >= PROGRESS_EVERY {
                    *last = (Instant::now(), pct, phase.to_string());
                    let _ = handle.emit(
                        "build-update",
                        serde_json::json!({"repo": repo, "state": "building", "pct": pct, "phase": phase}),
                    );
                }
            })
        };

        let logs_dir = state.logs_dir();
        let env = JobEnv { store: &state.store, client: &state.client, logs_dir: &logs_dir, cancel: &state.queue.cancel };
        let result = build::run_job(&env, &job, &progress).await;

        let record = match &result {
            Ok((sha, ms)) => BuildRecord { status: "built".into(), sha: sha.clone(), built_at: unix_now(), duration_ms: *ms, error: String::new() },
            Err(e) => BuildRecord { status: "failed".into(), sha: String::new(), built_at: unix_now(), duration_ms: 0, error: e.clone() },
        };
        if let Ok(l) = state.loaded() {
            let _ = l.records.update(|r| {
                r.insert(repo.clone(), record.clone());
            });
        }
        state.queue.finish(&repo, result.is_ok());
        match &result {
            Ok(_) => state.log_op(format!("Built {}", job.title)),
            Err(e) => state.log_op(format!("Build failed: {} — {e}", job.title)),
        }
        let _ = handle.emit(
            "build-update",
            serde_json::json!({
                "repo": repo,
                "state": if result.is_ok() { "built" } else { "failed" },
                "pct": 100,
                "phase": "",
                "error": result.as_ref().err(),
            }),
        );
        emit_summary(&handle, &state);
    }

    let summary = state.queue.summary();
    if !summary.running {
        state.log_op(format!("Build run finished: {} built, {} failed", summary.done, summary.failed));
        let _ = handle.emit("build-summary", summary);
    }
}

fn jobs_for(state: &AppState, repos: &[String], metadata: bool, covers: bool, force: bool) -> Vec<Job> {
    repos
        .iter()
        .filter_map(|r| state.store.get(r).map(|b| (r, b)))
        .map(|(r, b)| Job { repo: r.clone(), title: b.title, metadata, covers, force })
        .collect()
}

#[tauri::command]
fn build_books(
    handle: tauri::AppHandle,
    state: State<'_>,
    repos: Vec<String>,
    metadata: bool,
    covers: bool,
    force: bool,
) -> Result<serde_json::Value, String> {
    state.loaded()?;
    let added = state.queue.enqueue(jobs_for(&state, &repos, metadata, covers, force));
    if added > 0 {
        state.log_op(format!("Queued {added} book(s) to build"));
    }
    pump_builds(&handle, &state);
    Ok(serde_json::json!({ "added": added }))
}

/// Queue every book that has no epub yet, with the default toggles.
#[tauri::command]
fn build_all(handle: tauri::AppHandle, state: State<'_>) -> Result<serde_json::Value, String> {
    state.loaded()?;
    let s = state.settings();
    let repos: Vec<String> = state.store.read(|books| {
        books.iter().map(repo_name).filter(|r| !r.is_empty() && !state.store.is_downloaded(r)).collect()
    });
    let added = state.queue.enqueue(jobs_for(&state, &repos, s.build_metadata, s.build_covers, false));
    state.log_op(format!("Build All: queued {added} book(s)"));
    pump_builds(&handle, &state);
    Ok(serde_json::json!({ "added": added }))
}

/// Build one book and wait for it (used by the reader when an epub is missing).
#[tauri::command]
async fn build_and_wait(handle: tauri::AppHandle, state: State<'_>, repo: String) -> Result<(), String> {
    state.loaded()?;
    let s = state.settings();
    state.queue.enqueue(jobs_for(&state, std::slice::from_ref(&repo), s.build_metadata, s.build_covers, true));
    pump_builds(&handle, &state);
    while state.queue.live(&repo).is_some() {
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    if state.store.is_downloaded(&repo) {
        Ok(())
    } else {
        let records = state.loaded()?.records.get();
        Err(records.get(&repo).map(|r| r.error.clone()).unwrap_or_else(|| "build failed".into()))
    }
}

#[tauri::command]
fn cancel_builds(handle: tauri::AppHandle, state: State<'_>) {
    state.queue.cancel_all();
    state.log_op("Builds cancelled");
    emit_summary(&handle, &state);
}

#[tauri::command]
fn get_build_log(state: State<'_>, repo: String) -> String {
    std::fs::read_to_string(build::log_path(&state.logs_dir(), &repo)).unwrap_or_default()
}

#[derive(Serialize)]
struct RecordRow {
    repo: String,
    title: String,
    #[serde(flatten)]
    record: BuildRecord,
}

#[tauri::command]
fn list_build_records(state: State<'_>, limit: usize) -> Result<Vec<RecordRow>, String> {
    let records = state.loaded()?.records.get();
    let mut rows: Vec<RecordRow> = records
        .into_iter()
        .map(|(repo, record)| RecordRow {
            title: state.store.get(&repo).map(|b| b.title).unwrap_or_else(|| repo.clone()),
            repo,
            record,
        })
        .collect();
    rows.sort_by(|a, b| b.record.built_at.cmp(&a.record.built_at));
    rows.truncate(limit.max(1));
    Ok(rows)
}

#[tauri::command]
fn get_op_log(state: State<'_>) -> Vec<String> {
    state.op_log.lock().unwrap().iter().cloned().collect()
}

// --- Settings ---

#[tauri::command]
fn get_settings(state: State<'_>) -> Result<AppSettings, String> {
    Ok(state.loaded()?.settings.get())
}

#[tauri::command]
async fn save_settings(state: State<'_>, settings: AppSettings) -> Result<AppSettings, String> {
    let saved = state.loaded()?.settings.save(settings)?;
    let was = state.store.organize();
    state.store.set_organize(saved.organize_by_genre);
    if saved.organize_by_genre && !was {
        let st = Arc::clone(&state);
        tokio::task::spawn_blocking(move || st.relocate()).await.map_err(|e| format!("{e}"))?;
    }
    Ok(saved)
}

// --- Long-running operations (sync / enrich / covers) ---

type OpProgress = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

/// Shared wrapper: exclusive start, progress events, op log, activity record.
async fn run_op<F, Fut>(
    handle: tauri::AppHandle,
    state: Arc<AppState>,
    name: &'static str,
    record: fn(&mut Activity, OpRecord),
    work: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(Arc<AppState>, OpProgress) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value, String>> + Send + 'static,
{
    state.start_op(name)?;
    state.log_op(format!("{name} started"));
    let h = handle.clone();
    let progress: OpProgress = Arc::new(move |v| {
        let _ = h.emit("op-progress", v);
    });
    let st = Arc::clone(&state);
    let result = tauri::async_runtime::spawn(async move { work(st, progress).await })
        .await
        .map_err(|e| format!("task panic: {e}"))
        .and_then(|r| r);
    state.finish_op();

    match &result {
        Ok(summary) => {
            state.log_op(format!("{name} finished: {summary}"));
            state.record_activity(|a| record(a, OpRecord { at: unix_now(), summary: summary.clone() }));
            let _ = handle.emit("op-done", serde_json::json!({ "operation": name, "summary": summary }));
        }
        Err(e) => {
            state.log_op(format!("{name} failed: {e}"));
            let _ = handle.emit("op-error", serde_json::json!({ "operation": name, "error": e }));
        }
    }
    result
}

fn counted(progress: &OpProgress) -> impl Fn(usize, usize, String, String) + '_ {
    move |current, total, title, status| {
        // Throttle large runs to ~200 events, but always report errors and the last one
        let step = (total / 200).max(1);
        if current % step == 0 || current == total || status.starts_with("error") {
            progress(serde_json::json!({ "current": current, "total": total, "title": title, "status": status }));
        }
    }
}

#[tauri::command]
async fn sync_catalog(handle: tauri::AppHandle, state: State<'_>) -> Result<serde_json::Value, String> {
    let token = state.settings().github_token;
    run_op(handle, Arc::clone(&state), "Catalog sync", |a, r| a.last_sync = Some(r), move |st, progress| async move {
        catalog::sync_catalog(&st.store, &st.client, &token, |msg: String| {
            st.log_op(&msg);
            progress(serde_json::json!({ "message": msg }));
        })
        .await
    })
    .await
}

#[tauri::command]
async fn enrich_all(handle: tauri::AppHandle, state: State<'_>) -> Result<serde_json::Value, String> {
    let concurrency = state.settings().enrich_concurrency as usize;
    run_op(handle, Arc::clone(&state), "Metadata enrichment", |a, r| a.last_enrich = Some(r), move |st, progress| async move {
        let r = enrich::enrich_all(&st.store, &st.client, concurrency, true, counted(&progress), Arc::clone(&st.cancel)).await;
        let st2 = Arc::clone(&st);
        let moved = tokio::task::spawn_blocking(move || st2.relocate().moved).await.unwrap_or(0);
        Ok(serde_json::json!({ "enriched": r.enriched, "skipped": r.skipped, "errors": r.errors, "refiled": moved }))
    })
    .await
}

#[tauri::command]
async fn fetch_covers(handle: tauri::AppHandle, state: State<'_>) -> Result<serde_json::Value, String> {
    run_op(handle, Arc::clone(&state), "Cover fetch", |a, r| a.last_covers = Some(r), |st, progress| async move {
        let dir = st.store.covers_dir.clone();
        let r = covers::fetch_all_covers(&st.store, &st.client, &dir, 10, counted(&progress), Arc::clone(&st.cancel)).await;
        Ok(serde_json::json!({ "fetched": r.fetched, "cached": r.cached, "errors": r.errors }))
    })
    .await
}

#[tauri::command]
fn cancel_operation(state: State<'_>) -> serde_json::Value {
    match state.current_op() {
        None => serde_json::json!({"status": "nothing running"}),
        Some(op) => {
            state.cancel.store(true, Ordering::SeqCst);
            serde_json::json!({"status": "cancelling", "operation": op})
        }
    }
}

// --- Files ---

#[tauri::command]
fn open_epub(handle: tauri::AppHandle, state: State<'_>, repo: String) -> Result<(), String> {
    state.loaded()?;
    let path = state.store.epub_file(&repo).ok_or("epub not found")?;
    handle.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(|e| format!("open epub: {e}"))
}

#[tauri::command]
fn reveal_epub(handle: tauri::AppHandle, state: State<'_>, repo: String) -> Result<(), String> {
    state.loaded()?;
    let path = state.store.epub_file(&repo).ok_or("epub not found")?;
    handle.opener().reveal_item_in_dir(path).map_err(|e| format!("show in folder: {e}"))
}

#[tauri::command]
fn open_library_folder(handle: tauri::AppHandle, state: State<'_>) -> Result<(), String> {
    let dir = state.store.epubs_dir.parent().unwrap_or(&state.store.epubs_dir).to_path_buf();
    handle.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| format!("open folder: {e}"))
}

// --- Instance lock / startup ---

#[derive(Serialize)]
struct StatusResponse {
    ready: bool,
    edition: &'static str,
    held_by: Option<HolderInfo>,
    migration: Option<MigrationReport>,
    refiled: Option<MigrationReport>,
    warnings: Vec<String>,
}

/// Called by the frontend at boot and every 2 s while blocked. Tries to take the
/// instance lock; on success migrates the old library, loads data and marks ready.
#[tauri::command]
async fn instance_status(state: State<'_>) -> Result<StatusResponse, String> {
    let mut status = StatusResponse {
        ready: true,
        edition: EDITION,
        held_by: None,
        migration: None,
        refiled: None,
        warnings: vec![],
    };
    let mut guard = state.lock.lock().unwrap();
    if state.ready.load(Ordering::SeqCst) {
        return Ok(status);
    }

    let lock = match instance::try_acquire(&state.data_dir, EDITION) {
        Err(e) => return Err(format!("cannot create the instance lock in {}: {e}", state.data_dir.display())),
        Ok(Acquire::HeldBy(info)) => {
            status.ready = false;
            status.held_by = info;
            return Ok(status);
        }
        Ok(Acquire::Acquired(lock)) => lock,
    };
    *guard = Some(lock);

    let mut report = MigrationReport::default();
    for sub in ["epubs", "covers"] {
        library::migrate_dir(
            &state.project_dir.join(sub),
            &state.store.epubs_dir.parent().unwrap_or(&state.project_dir).join(sub),
            &mut report,
        );
    }
    if !report.is_empty() {
        log::info!("library migration: {report:?}");
    }
    status.migration = Some(report);

    let dd = &state.data_dir;
    let mut warn = |w: Option<String>| {
        if let Some(w) = w {
            log::warn!("{w}");
            status.warnings.push(w);
        }
    };
    let (reading, w) = ReadingStore::open(dd.join("reading.json"));
    warn(w);
    let (settings, w) = SettingsStore::open(dd.join("settings.json"));
    warn(w);
    let (activity, w) = ActivityStore::open(dd.join("activity.json"));
    warn(w);
    let (records, w) = BuildRecords::open(dd.join("builds.json"));
    warn(w);

    state.store.set_organize(settings.get().organize_by_genre);
    if let Err(e) = state.store.load() {
        warn(Some(e));
    }
    let _ = state.loaded.set(Loaded { reading, settings, activity, records });
    state.ready.store(true, Ordering::SeqCst);

    let refiled = state.relocate();
    status.refiled = (!refiled.is_empty()).then_some(refiled);
    state.log_op(format!("StandardIssue {EDITION} started"));
    Ok(status)
}

// --- Reader ---

#[derive(Serialize)]
struct ReaderOpenResponse {
    path: String,
    title: String,
    author: String,
    word_count: i64,
    published: Option<Published>,
    cover_path: String,
    progress: Option<BookProgress>,
    bookmarks: Vec<Bookmark>,
    pace: PaceInfo,
}

#[tauri::command]
fn reader_open(state: State<'_>, repo: String) -> Result<ReaderOpenResponse, String> {
    let reading = &state.loaded()?.reading;
    let path = state.store.epub_file(&repo).ok_or_else(|| "not-found".to_string())?;
    let book = state.store.get(&repo).unwrap_or_default();
    Ok(ReaderOpenResponse {
        path: path.to_string_lossy().to_string(),
        title: book.title,
        author: book.author,
        word_count: book.word_count,
        published: book.published,
        cover_path: state.cover_path(&repo),
        progress: reading.progress(&repo),
        bookmarks: reading.bookmarks(&repo),
        pace: reading.pace(),
    })
}

#[tauri::command]
fn reader_add_bookmark(state: State<'_>, repo: String, bookmark: Bookmark) -> Result<Vec<Bookmark>, String> {
    state.loaded()?.reading.add_bookmark(&repo, bookmark)
}

#[tauri::command]
fn reader_remove_bookmark(state: State<'_>, repo: String, cfi: String) -> Result<Vec<Bookmark>, String> {
    state.loaded()?.reading.remove_bookmark(&repo, &cfi)
}

#[tauri::command]
fn reader_record_pace(state: State<'_>, words: f64, seconds: f64) -> Result<PaceInfo, String> {
    state.loaded()?.reading.record_pace(words, seconds)
}

#[tauri::command]
fn reader_save_progress(state: State<'_>, repo: String, cfi: String, fraction: f64) -> Result<(), String> {
    state.loaded()?.reading.save_progress(&repo, &cfi, fraction)
}

#[tauri::command]
fn reader_get_settings(state: State<'_>) -> Result<ReaderSettings, String> {
    Ok(state.loaded()?.reading.get_settings())
}

#[tauri::command]
fn reader_save_settings(state: State<'_>, settings: ReaderSettings) -> Result<ReaderSettings, String> {
    state.loaded()?.reading.save_settings(settings)
}

#[derive(Serialize)]
struct ProgressWithTitle {
    #[serde(flatten)]
    progress: BookProgress,
    title: String,
    cover_path: String,
}

#[tauri::command]
fn reading_progress_all(state: State<'_>) -> Result<BTreeMap<String, ProgressWithTitle>, String> {
    let all = state.loaded()?.reading.all_progress();
    Ok(all
        .into_iter()
        .map(|(repo, progress)| {
            let title = state.store.get(&repo).map(|b| b.title).unwrap_or_default();
            let cover_path = state.cover_path(&repo);
            (repo, ProgressWithTitle { progress, title, cover_path })
        })
        .collect())
}

fn main() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Project root: two levels up from the executable (target/debug/exe → src-tauri → project)
            // In dev mode, use the manifest dir via env; in release, resolve from exe path.
            let project_dir = if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            } else {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| PathBuf::from("."))
            };

            let data_dir = app.path().app_data_dir().unwrap_or_else(|_| PathBuf::from("."));
            std::fs::create_dir_all(&data_dir).ok();

            // Shared by both editions: Documents\StandardIssue Library\{epubs,covers}
            let library_dir = app
                .path()
                .document_dir()
                .unwrap_or_else(|_| project_dir.clone())
                .join(library::LIBRARY_DIR_NAME);
            let epubs_dir = library_dir.join("epubs");
            let covers_dir = library_dir.join("covers");
            std::fs::create_dir_all(&epubs_dir).ok();
            std::fs::create_dir_all(&covers_dir).ok();

            // The webview may read only the library folders through the asset protocol
            let scope = app.asset_protocol_scope();
            scope.allow_directory(&epubs_dir, true)?;
            scope.allow_directory(&covers_dir, true)?;

            // Data is loaded by instance_status once the instance lock is held
            let store = BookStore::new(data_dir.join("books.json"), epubs_dir, covers_dir);

            app.manage(Arc::new(AppState {
                store,
                operation: Mutex::new(None),
                cancel: Arc::new(AtomicBool::new(false)),
                client: reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap(),
                queue: BuildQueue::default(),
                op_log: Mutex::new(VecDeque::new()),
                ready: AtomicBool::new(false),
                lock: Mutex::new(None),
                data_dir,
                project_dir,
                loaded: OnceLock::new(),
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            query_books,
            get_book,
            live_states,
            get_overview,
            get_disk_usage,
            build_books,
            build_all,
            build_and_wait,
            cancel_builds,
            get_build_log,
            list_build_records,
            get_op_log,
            get_settings,
            save_settings,
            sync_catalog,
            enrich_all,
            fetch_covers,
            cancel_operation,
            open_epub,
            reveal_epub,
            open_library_folder,
            instance_status,
            reader_open,
            reader_save_progress,
            reader_get_settings,
            reader_save_settings,
            reader_add_bookmark,
            reader_remove_bookmark,
            reader_record_pace,
            reading_progress_all,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
