// Michael Burgus — https://github.com/NeuralDrifter
// Epub builder: shallow git clone + zip src/ into a valid epub, with live
// per-book progress, a build queue, per-book logs and build records.

use crate::books::{repo_name, BookStore};
use crate::enrich::{apply_enrichment, fetch_enrichment, Needs};
use crate::settings::JsonStore;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;

const ORG: &str = "standardebooks";

/// Share of a book's progress bar used by the clone; packaging fills the rest.
const CLONE_END: f32 = 85.0;
const RECEIVE_END: f32 = 75.0;

pub type Progress = Arc<dyn Fn(u8, &str) + Send + Sync>;

// --- Records and logs ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildRecord {
    /// "built" or "failed"
    pub status: String,
    #[serde(default)]
    pub sha: String,
    pub built_at: u64,
    pub duration_ms: u64,
    #[serde(default)]
    pub error: String,
}

pub type BuildRecords = JsonStore<BTreeMap<String, BuildRecord>>;

/// Lines of one build, timestamped relative to its start. When given a file,
/// every line is appended as it happens so the UI can tail a running build.
pub struct BuildLog {
    start: Instant,
    lines: Mutex<Vec<String>>,
    file: Mutex<Option<std::fs::File>>,
}

impl BuildLog {
    pub fn new() -> Self {
        Self { start: Instant::now(), lines: Mutex::new(Vec::new()), file: Mutex::new(None) }
    }

    /// Log to `path` too, replacing the previous build's log.
    pub fn to_file(path: &Path) -> Self {
        let log = Self::new();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        *log.file.lock().unwrap() = std::fs::File::create(path).ok();
        log
    }

    pub fn line(&self, msg: impl AsRef<str>) {
        let t = self.start.elapsed().as_secs_f32();
        let line = format!("[+{t:5.1}s] {}", msg.as_ref());
        if let Some(f) = self.file.lock().unwrap().as_mut() {
            let _ = writeln!(f, "{line}");
        }
        self.lines.lock().unwrap().push(line);
    }

    pub fn text(&self) -> String {
        self.lines.lock().unwrap().join("\n")
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

impl Default for BuildLog {
    fn default() -> Self {
        Self::new()
    }
}

pub fn log_path(logs_dir: &Path, repo: &str) -> PathBuf {
    logs_dir.join(format!("{repo}.log"))
}

// --- git progress ---

/// Map one line of `git clone --progress` output to overall book progress (0–85).
pub fn parse_git_progress(line: &str) -> Option<u8> {
    let pct = |rest: &str| -> Option<f32> {
        let n = rest.trim_start().split('%').next()?.trim();
        n.parse::<f32>().ok()
    };
    let line = line.trim();
    if let Some(rest) = line.strip_prefix("Receiving objects:") {
        return Some((pct(rest)? / 100.0 * RECEIVE_END) as u8);
    }
    if let Some(rest) = line.strip_prefix("Resolving deltas:") {
        return Some((RECEIVE_END + pct(rest)? / 100.0 * (CLONE_END - RECEIVE_END)) as u8);
    }
    None
}

/// git rewrites progress lines with `\r`; split a byte stream into lines on `\r` and `\n`.
#[derive(Default)]
pub struct LineSplitter {
    buf: String,
}

impl LineSplitter {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut out = Vec::new();
        while let Some(i) = self.buf.find(['\r', '\n']) {
            let line: String = self.buf.drain(..=i).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if !line.trim().is_empty() {
                out.push(line.to_string());
            }
        }
        out
    }

    pub fn finish(self) -> Option<String> {
        let rest = self.buf.trim();
        (!rest.is_empty()).then(|| rest.to_string())
    }
}

async fn clone_repo(repo: &str, dir: &Path, log: &BuildLog, progress: &Progress, cancel: &AtomicBool) -> Result<(), String> {
    let url = format!("https://github.com/{ORG}/{repo}.git");
    log.line(format!("$ git clone --depth 1 --progress {url}"));
    let mut child = tokio::process::Command::new("git")
        .args(["clone", "--depth", "1", "--progress", &url])
        .arg(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "git not found — install Git and restart StandardIssue".to_string()
            } else {
                format!("git clone spawn: {e}")
            }
        })?;

    let mut stderr = child.stderr.take().ok_or("no git stderr")?;
    let mut splitter = LineSplitter::default();
    let mut buf = [0u8; 4096];
    let mut last_logged = String::new();
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill().await;
            return Err("cancelled".into());
        }
        let n = match tokio::time::timeout(Duration::from_millis(250), stderr.read(&mut buf)).await {
            Err(_) => continue, // no output yet; re-check cancel
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(format!("read git output: {e}")),
        };
        for line in splitter.push(&buf[..n]) {
            if let Some(p) = parse_git_progress(&line) {
                progress(p, "Cloning");
            }
            // Log each stage once, at its final state, instead of every percent
            let stage = line.split(':').next().unwrap_or("").to_string();
            if stage != last_logged || line.contains("done") {
                if line.contains("done") || parse_git_progress(&line).is_none() {
                    log.line(&line);
                }
                last_logged = stage;
            }
        }
    }
    if let Some(rest) = splitter.finish() {
        log.line(rest);
    }
    let status = child.wait().await.map_err(|e| format!("git clone: {e}"))?;
    if !status.success() {
        return Err(format!("git clone failed ({status})"));
    }
    Ok(())
}

async fn git_head(dir: &Path) -> String {
    let out = tokio::process::Command::new("git").arg("-C").arg(dir).args(["rev-parse", "HEAD"]).output().await;
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

// --- zip ---

/// Zip `src_dir` into an epub: `mimetype` first and uncompressed, per spec.
/// Calls `on_file(done, total)` after each file. Returns the file count.
pub fn zip_epub(epub_path: &Path, src_dir: &Path, mut on_file: impl FnMut(usize, usize)) -> Result<usize, String> {
    let mimetype_path = src_dir.join("mimetype");
    if !mimetype_path.exists() {
        return Err("no mimetype file in src/".into());
    }
    let mut files: Vec<PathBuf> = WalkDir::new(src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|e| e.into_path())
        .filter(|p| p.is_file() && *p != mimetype_path)
        .collect();
    files.sort();
    let total = files.len() + 1;

    let file = std::fs::File::create(epub_path).map_err(|e| format!("create epub: {e}"))?;
    let mut zw = zip::ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zw.start_file("mimetype", stored).map_err(|e| format!("zip mimetype: {e}"))?;
    let mime = std::fs::read(&mimetype_path).map_err(|e| format!("read mimetype: {e}"))?;
    zw.write_all(&mime).map_err(|e| format!("write mimetype: {e}"))?;
    on_file(1, total);

    let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (i, path) in files.iter().enumerate() {
        let rel = path.strip_prefix(src_dir).map_err(|e| format!("strip prefix: {e}"))?;
        let rel = rel.to_string_lossy().replace('\\', "/");
        zw.start_file(&rel, deflated).map_err(|e| format!("zip {rel}: {e}"))?;
        let data = std::fs::read(path).map_err(|e| format!("read {rel}: {e}"))?;
        zw.write_all(&data).map_err(|e| format!("write {rel}: {e}"))?;
        on_file(i + 2, total);
    }
    zw.finish().map_err(|e| format!("zip finish: {e}"))?;
    Ok(total)
}

/// Clone and package one repo into `dest`. The epub is written to a `.part`
/// file and renamed into place, so a failed or cancelled rebuild never
/// damages an existing epub. Returns the commit SHA.
pub async fn build_epub(repo: &str, dest: &Path, log: &BuildLog, progress: &Progress, cancel: &AtomicBool) -> Result<String, String> {
    let parent = dest.parent().ok_or("bad destination")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;

    let tmp = tempfile::tempdir().map_err(|e| format!("temp dir: {e}"))?;
    let clone_dir = tmp.path().join(repo);
    clone_repo(repo, &clone_dir, log, progress, cancel).await?;
    let sha = git_head(&clone_dir).await;
    if !sha.is_empty() {
        log.line(format!("HEAD {sha}"));
    }

    let src_dir = clone_dir.join("src");
    if !src_dir.is_dir() {
        return Err(format!("no src/ directory in {repo}"));
    }
    progress(CLONE_END as u8, "Packaging");
    let part = dest.with_extension("epub.part");
    let (p2, part2) = (progress.clone(), part.clone());
    let zipped = tokio::task::spawn_blocking(move || {
        zip_epub(&part2, &src_dir, |done, total| {
            let pct = CLONE_END + (100.0 - CLONE_END) * done as f32 / total as f32;
            p2(pct.min(99.0) as u8, "Packaging");
        })
    })
    .await
    .map_err(|e| format!("zip task panic: {e}"))?;

    let finish = zipped.and_then(|files| {
        if cancel.load(Ordering::SeqCst) {
            return Err("cancelled".into());
        }
        std::fs::rename(&part, dest).map_err(|e| format!("move epub into place: {e}"))?;
        Ok(files)
    });
    match finish {
        Ok(files) => {
            log.line(format!("Packaged {files} files → {}", dest.display()));
            Ok(sha)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

// --- One job, start to finish ---

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub repo: String,
    pub title: String,
    pub metadata: bool,
    pub covers: bool,
    pub force: bool,
}

pub struct JobEnv<'a> {
    pub store: &'a BookStore,
    pub client: &'a reqwest::Client,
    pub logs_dir: &'a Path,
    pub cancel: &'a AtomicBool,
}

/// Enrich (as needed), fetch the cover (if asked), then build. Streams the log
/// file and returns `(sha, duration_ms)`.
pub async fn run_job(env: &JobEnv<'_>, job: &Job, progress: &Progress) -> Result<(String, u64), String> {
    let log = BuildLog::to_file(&log_path(env.logs_dir, &job.repo));
    let result = run_job_inner(env, job, progress, &log).await;
    match &result {
        Ok(_) => log.line("Build succeeded"),
        Err(e) => log.line(format!("Build failed: {e}")),
    }
    result.map(|sha| (sha, log.elapsed_ms()))
}

async fn run_job_inner(env: &JobEnv<'_>, job: &Job, progress: &Progress, log: &BuildLog) -> Result<String, String> {
    let book = env.store.get(&job.repo).ok_or_else(|| format!("unknown book {}", job.repo))?;
    log.line(format!("Building {} ({})", book.title, job.repo));

    // Basic metadata is always needed: the genre decides the folder.
    let needs = Needs::of(&book, job.metadata);
    if needs.any() {
        progress(0, "Fetching metadata");
        log.line(format!(
            "Fetching {}",
            match (needs.opf, needs.colophon) {
                (true, true) => "content.opf and colophon",
                (true, false) => "content.opf",
                _ => "colophon",
            }
        ));
        match fetch_enrichment(env.client, &job.repo, needs).await {
            Ok(e) => {
                env.store.update(&job.repo, |b| apply_enrichment(b, &e));
                let _ = env.store.save();
            }
            // Not fatal: the book still builds, just without fresh metadata
            Err(e) => log.line(format!("Metadata fetch failed: {e}")),
        }
    }

    if job.covers && !env.store.cover_file(&job.repo).exists() {
        progress(0, "Fetching cover");
        log.line("Fetching cover art");
        if let Err(e) = crate::covers::fetch_and_save_cover(env.client, &job.repo, &env.store.covers_dir).await {
            log.line(format!("Cover fetch failed: {e}"));
        }
    }

    let book = env.store.get(&job.repo).unwrap_or(book);
    let dest = env.store.target_path(&book);
    if !job.force {
        if let Some(existing) = env.store.locate(&book) {
            log.line(format!("Already built: {}", existing.display()));
            return Ok(String::new());
        }
    }
    let sha = build_epub(&job.repo, &dest, log, progress, env.cancel).await?;

    // A forced rebuild into a new folder leaves no stale copy behind
    if let Some(old) = env.store.epub_file(&job.repo).filter(|p| *p != dest) {
        let _ = std::fs::remove_file(old);
    }
    env.store.refresh_one(&repo_name(&book));
    Ok(sha)
}

// --- Queue ---

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Live {
    /// "queued" or "building"
    pub state: &'static str,
    pub pct: u8,
    pub phase: String,
    /// 1-based position among queued books (0 while building)
    pub queue_pos: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueItem {
    pub repo: String,
    pub title: String,
    pub pct: u8,
    pub phase: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct QueueSummary {
    pub queued: usize,
    pub active: usize,
    pub done: usize,
    pub failed: usize,
    pub running: bool,
    pub active_list: Vec<QueueItem>,
    pub next: Vec<QueueItem>,
}

#[derive(Default)]
struct QueueInner {
    pending: VecDeque<Job>,
    active: HashMap<String, (Job, u8, String)>,
    done: usize,
    failed: usize,
    workers: usize,
}

#[derive(Default)]
pub struct BuildQueue {
    inner: Mutex<QueueInner>,
    pub cancel: Arc<AtomicBool>,
}

impl BuildQueue {
    /// Add jobs not already queued or building. Starting from idle resets the counters.
    pub fn enqueue(&self, jobs: Vec<Job>) -> usize {
        let mut q = self.inner.lock().unwrap();
        if q.workers == 0 && q.pending.is_empty() && q.active.is_empty() {
            q.done = 0;
            q.failed = 0;
            self.cancel.store(false, Ordering::SeqCst);
        }
        let mut added = 0;
        for job in jobs {
            if q.active.contains_key(&job.repo) || q.pending.iter().any(|j| j.repo == job.repo) {
                continue;
            }
            q.pending.push_back(job);
            added += 1;
        }
        added
    }

    /// Reserve a worker slot if fewer than `max` are running and work is waiting.
    pub fn try_add_worker(&self, max: usize) -> bool {
        let mut q = self.inner.lock().unwrap();
        if q.workers < max && !q.pending.is_empty() {
            q.workers += 1;
            true
        } else {
            false
        }
    }

    /// Take the next job, or release this worker's slot when there is none.
    pub fn next_job(&self) -> Option<Job> {
        let mut q = self.inner.lock().unwrap();
        if self.cancel.load(Ordering::SeqCst) {
            q.pending.clear();
        }
        match q.pending.pop_front() {
            Some(job) => {
                q.active.insert(job.repo.clone(), (job.clone(), 0, "Starting".into()));
                Some(job)
            }
            None => {
                q.workers = q.workers.saturating_sub(1);
                None
            }
        }
    }

    pub fn set_progress(&self, repo: &str, pct: u8, phase: &str) {
        if let Some(a) = self.inner.lock().unwrap().active.get_mut(repo) {
            a.1 = pct;
            a.2 = phase.to_string();
        }
    }

    pub fn finish(&self, repo: &str, ok: bool) {
        let mut q = self.inner.lock().unwrap();
        q.active.remove(repo);
        if ok {
            q.done += 1;
        } else {
            q.failed += 1;
        }
    }

    pub fn cancel_all(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.inner.lock().unwrap().pending.clear();
    }

    pub fn live(&self, repo: &str) -> Option<Live> {
        let q = self.inner.lock().unwrap();
        if let Some((_, pct, phase)) = q.active.get(repo) {
            return Some(Live { state: "building", pct: *pct, phase: phase.clone(), queue_pos: 0 });
        }
        q.pending.iter().position(|j| j.repo == repo).map(|i| Live {
            state: "queued",
            pct: 0,
            phase: "Queued".into(),
            queue_pos: i + 1,
        })
    }

    /// Positions of every queued book, for patching cards after a change.
    pub fn positions(&self) -> Vec<(String, usize)> {
        let q = self.inner.lock().unwrap();
        q.pending.iter().enumerate().map(|(i, j)| (j.repo.clone(), i + 1)).collect()
    }

    pub fn summary(&self) -> QueueSummary {
        let q = self.inner.lock().unwrap();
        let mut active_list: Vec<QueueItem> = q
            .active
            .values()
            .map(|(j, pct, phase)| QueueItem { repo: j.repo.clone(), title: j.title.clone(), pct: *pct, phase: phase.clone() })
            .collect();
        active_list.sort_by(|a, b| a.title.cmp(&b.title));
        QueueSummary {
            queued: q.pending.len(),
            active: q.active.len(),
            done: q.done,
            failed: q.failed,
            running: q.workers > 0 || !q.active.is_empty(),
            active_list,
            next: q
                .pending
                .iter()
                .take(3)
                .map(|j| QueueItem { repo: j.repo.clone(), title: j.title.clone(), pct: 0, phase: "Queued".into() })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(repo: &str) -> Job {
        Job { repo: repo.into(), title: repo.to_uppercase(), metadata: true, covers: true, force: false }
    }

    #[test]
    fn build_log_streams_to_file() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("logs/a.log");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old build").unwrap();
        let log = BuildLog::to_file(&path);
        log.line("cloning");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("cloning") && !text.contains("old build"));
        assert_eq!(log.text().lines().count(), 1);
    }

    #[test]
    fn git_progress_maps_to_clone_share() {
        assert_eq!(parse_git_progress("Receiving objects:  0% (1/267)"), Some(0));
        assert_eq!(parse_git_progress("Receiving objects:  48% (129/267), 1.20 MiB | 2.4 MiB/s"), Some(36));
        assert_eq!(parse_git_progress("Receiving objects: 100% (267/267), 3.1 MiB | 4 MiB/s, done."), Some(75));
        assert_eq!(parse_git_progress("Resolving deltas:  50% (10/20)"), Some(80));
        assert_eq!(parse_git_progress("Resolving deltas: 100% (20/20), done."), Some(85));
        assert_eq!(parse_git_progress("Cloning into 'x'..."), None);
        assert_eq!(parse_git_progress("remote: Counting objects: 100% (5/5)"), None);
    }

    #[test]
    fn splitter_handles_carriage_returns_across_chunks() {
        let mut s = LineSplitter::default();
        assert_eq!(s.push(b"Receiving objects:  10% (1/10)\rReceiving obj"), ["Receiving objects:  10% (1/10)"]);
        assert_eq!(s.push(b"ects:  20% (2/10)\r\nDone\n"), ["Receiving objects:  20% (2/10)", "Done"]);
        s.push(b"tail");
        assert_eq!(s.finish().as_deref(), Some("tail"));
    }

    #[test]
    fn zip_puts_mimetype_first_uncompressed_and_reports_progress() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        std::fs::create_dir_all(src.join("epub/text")).unwrap();
        std::fs::write(src.join("mimetype"), "application/epub+zip").unwrap();
        std::fs::write(src.join("epub/text/a.xhtml"), "<html/>").unwrap();
        std::fs::write(src.join("epub/content.opf"), "<package/>").unwrap();

        let out = t.path().join("book.epub");
        let mut seen = Vec::new();
        let n = zip_epub(&out, &src, |d, total| seen.push((d, total))).unwrap();
        assert_eq!(n, 3);
        assert_eq!(seen, [(1, 3), (2, 3), (3, 3)]);

        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let first = z.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), zip::CompressionMethod::Stored);
        drop(first);
        assert!(z.by_name("epub/text/a.xhtml").is_ok());
    }

    #[test]
    fn zip_without_mimetype_fails_and_writes_nothing() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("src")).unwrap();
        let out = t.path().join("x.epub");
        assert!(zip_epub(&out, &t.path().join("src"), |_, _| {}).is_err());
        assert!(!out.exists());
    }

    #[test]
    fn queue_dedupes_orders_and_tracks_state() {
        let q = BuildQueue::default();
        assert_eq!(q.enqueue(vec![job("a"), job("b"), job("a")]), 2);
        assert!(q.try_add_worker(2));
        assert_eq!(q.live("b").unwrap().queue_pos, 2);

        let a = q.next_job().unwrap();
        assert_eq!(a.repo, "a");
        assert_eq!(q.enqueue(vec![job("a"), job("c")]), 1, "a is building, c is new");
        q.set_progress("a", 40, "Cloning");
        assert_eq!(q.live("a").unwrap(), Live { state: "building", pct: 40, phase: "Cloning".into(), queue_pos: 0 });
        assert_eq!(q.live("b").unwrap().queue_pos, 1);

        let s = q.summary();
        assert_eq!((s.queued, s.active, s.running), (2, 1, true));
        assert_eq!(s.next.iter().map(|i| i.repo.as_str()).collect::<Vec<_>>(), ["b", "c"]);

        q.finish("a", true);
        assert!(q.live("a").is_none());
        assert_eq!(q.next_job().unwrap().repo, "b");
        q.finish("b", false);
        assert_eq!(q.next_job().unwrap().repo, "c");
        q.finish("c", true);
        assert!(q.next_job().is_none(), "worker released");
        let s = q.summary();
        assert_eq!((s.done, s.failed, s.running), (2, 1, false));

        // Starting again from idle resets counters
        q.enqueue(vec![job("d")]);
        assert_eq!(q.summary().done, 0);
    }

    #[test]
    fn cancel_clears_pending_and_releases_workers() {
        let q = BuildQueue::default();
        q.enqueue(vec![job("a"), job("b"), job("c")]);
        assert!(q.try_add_worker(1));
        assert!(!q.try_add_worker(1), "limit reached");
        q.next_job();
        q.cancel_all();
        assert_eq!(q.summary().queued, 0);
        q.finish("a", false);
        assert!(q.next_job().is_none());
        assert!(!q.summary().running);
        q.enqueue(vec![job("x")]);
        assert!(!q.cancel.load(Ordering::SeqCst), "new run clears cancel");
    }
}
