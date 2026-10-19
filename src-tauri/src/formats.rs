// Michael Burgus — https://github.com/NeuralDrifter
// Other formats made from a built epub: Kobo KEPUB (native) and Kindle AZW3 (Calibre), plus Calibre itself

use crate::kepub;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::AsyncReadExt;

/// A Calibre install: the folder holding `ebook-convert` and `calibre`.
#[derive(Debug, Clone, PartialEq)]
pub struct Calibre {
    pub dir: PathBuf,
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

impl Calibre {
    pub fn ebook_convert(&self) -> PathBuf {
        self.dir.join(exe("ebook-convert"))
    }
    pub fn gui(&self) -> PathBuf {
        self.dir.join(exe("calibre"))
    }
}

/// Where Calibre installs itself, then every folder on PATH.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(p) = std::env::var_os(var) {
            dirs.push(PathBuf::from(p).join("Calibre2"));
        }
    }
    dirs.push(PathBuf::from("/Applications/calibre.app/Contents/MacOS"));
    dirs.push(PathBuf::from("/opt/calibre"));
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs
}

/// The first folder that has Calibre's converter in it.
pub fn find_calibre_in(dirs: impl IntoIterator<Item = PathBuf>) -> Option<Calibre> {
    dirs.into_iter()
        .map(|dir| Calibre { dir })
        .find(|c| c.ebook_convert().is_file())
}

pub fn find_calibre() -> Option<Calibre> {
    find_calibre_in(candidate_dirs())
}

// --- Paths and status ---

pub fn azw3_path(epub: &Path) -> PathBuf {
    let stem = epub.file_stem().unwrap_or_default().to_string_lossy();
    epub.with_file_name(format!("{stem}.azw3"))
}

/// Files made from this epub, which move and go stale with it.
pub fn companions(epub: &Path) -> [PathBuf; 2] {
    [kepub::kepub_path(epub), azw3_path(epub)]
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    /// Older than the epub it was made from.
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FormatStatus {
    pub epub: FileInfo,
    pub kepub: Option<FileInfo>,
    pub azw3: Option<FileInfo>,
    pub calibre: bool,
}

fn info(path: &Path, epub_modified: Option<std::time::SystemTime>) -> Option<FileInfo> {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    let stale = matches!((meta.modified().ok(), epub_modified), (Some(m), Some(e)) if m < e);
    Some(FileInfo { path: path.to_string_lossy().to_string(), size: meta.len(), stale })
}

pub fn status(epub: &Path, calibre: bool) -> Option<FormatStatus> {
    let modified = std::fs::metadata(epub).ok()?.modified().ok();
    Some(FormatStatus {
        epub: info(epub, None)?,
        kepub: info(&kepub::kepub_path(epub), modified),
        azw3: info(&azw3_path(epub), modified),
        calibre,
    })
}

/// True when the format at `path` is missing or older than the epub.
pub fn needs(epub: &Path, path: &Path) -> bool {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (modified(path), modified(epub)) {
        (Some(made), Some(e)) => made < e,
        _ => true,
    }
}

// --- Making them ---

/// Build the KEPUB beside the epub (CPU work, kept off the async threads).
pub async fn make_kepub(epub: &Path) -> Result<kepub::Report, String> {
    let (epub, out) = (epub.to_path_buf(), kepub::kepub_path(epub));
    tokio::task::spawn_blocking(move || kepub::convert(&epub, &out))
        .await
        .map_err(|e| format!("kepub: {e}"))?
}

/// Convert to AZW3 beside the epub with Calibre's ebook-convert. The output goes to a
/// temporary name first, so a failed or cancelled conversion leaves no broken file.
pub async fn make_azw3(calibre: &Calibre, epub: &Path, cancel: &AtomicBool) -> Result<PathBuf, String> {
    let out = azw3_path(epub);
    let stem = epub.file_stem().unwrap_or_default().to_string_lossy();
    // ebook-convert picks the format from the extension, so the temporary name keeps it
    let tmp = epub.with_file_name(format!("{stem}.converting.azw3"));
    let mut child = crate::proc::command(calibre.ebook_convert())
        .arg(epub)
        .arg(&tmp)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("could not start Calibre's ebook-convert: {e}"))?;

    let mut stderr = child.stderr.take().ok_or("no ebook-convert output")?;
    let errors = tokio::spawn(async move {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s).await;
        s
    });
    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill().await;
            let _ = std::fs::remove_file(&tmp);
            return Err("cancelled".into());
        }
        match tokio::time::timeout(Duration::from_millis(250), child.wait()).await {
            Err(_) => continue,
            Ok(r) => break r.map_err(|e| format!("ebook-convert: {e}"))?,
        }
    };
    let errors = errors.await.unwrap_or_default();
    if !status.success() || !tmp.is_file() {
        let _ = std::fs::remove_file(&tmp);
        let last = errors.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        return Err(if last.is_empty() { format!("ebook-convert failed ({status})") } else { format!("ebook-convert failed: {last}") });
    }
    std::fs::rename(&tmp, &out).map_err(|e| format!("save azw3: {e}"))?;
    Ok(out)
}

/// Open the epub in Calibre, which adds it to the library (or hands it to the copy already running).
pub fn add_to_calibre(calibre: &Calibre, epub: &Path) -> Result<(), String> {
    std::process::Command::new(calibre.gui())
        .arg(epub)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not start Calibre: {e}"))
}

/// Remove formats made from an epub that is being replaced or deleted.
pub fn remove_companions(epub: &Path) {
    for p in companions(epub) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibre_found_in_first_folder_that_has_the_converter() {
        let t = tempfile::tempdir().unwrap();
        let empty = t.path().join("empty");
        let gui_only = t.path().join("gui-only");
        let install = t.path().join("Calibre2");
        let later = t.path().join("later");
        for d in [&empty, &gui_only, &install, &later] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(gui_only.join(exe("calibre")), b"").unwrap();
        std::fs::write(install.join(exe("ebook-convert")), b"").unwrap();
        std::fs::write(later.join(exe("ebook-convert")), b"").unwrap();

        let found = find_calibre_in([t.path().join("missing"), empty, gui_only, install.clone(), later]).unwrap();
        assert_eq!(found.dir, install);
        assert_eq!(found.ebook_convert(), install.join(exe("ebook-convert")));
        assert_eq!(find_calibre_in([t.path().join("missing")]), None);
    }

    #[test]
    fn formats_live_beside_the_epub() {
        let epub = Path::new("lib/Adventure/A/The Lost World_Arthur Conan Doyle (Standard Ebooks).epub");
        assert_eq!(
            azw3_path(epub),
            Path::new("lib/Adventure/A/The Lost World_Arthur Conan Doyle (Standard Ebooks).azw3")
        );
        let [k, a] = companions(epub);
        assert!(k.to_string_lossy().ends_with("(Standard Ebooks).kepub.epub"));
        assert_eq!(a, azw3_path(epub));
    }

    #[test]
    fn status_reports_missing_present_and_stale() {
        let t = tempfile::tempdir().unwrap();
        let epub = t.path().join("B (Standard Ebooks).epub");
        assert_eq!(status(&epub, true), None);
        std::fs::write(&epub, b"epub").unwrap();
        let s = status(&epub, false).unwrap();
        assert_eq!((s.kepub.is_none(), s.azw3.is_none(), s.calibre), (true, true, false));
        assert!(needs(&epub, &azw3_path(&epub)));

        std::fs::write(kepub::kepub_path(&epub), b"kepub").unwrap();
        // An azw3 from before the epub was rebuilt
        std::fs::write(azw3_path(&epub), b"old").unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(3600);
        std::fs::File::options().write(true).open(azw3_path(&epub)).unwrap().set_modified(old).unwrap();

        let s = status(&epub, true).unwrap();
        assert_eq!(s.kepub.as_ref().map(|k| (k.size, k.stale)), Some((5, false)));
        assert_eq!(s.azw3.as_ref().map(|a| a.stale), Some(true));
        assert!(needs(&epub, &azw3_path(&epub)));
        assert!(!needs(&epub, &kepub::kepub_path(&epub)));

        remove_companions(&epub);
        let s = status(&epub, true).unwrap();
        assert!(s.kepub.is_none() && s.azw3.is_none() && epub.exists());
    }

    /// Converts one real library book with the installed Calibre.
    /// Run with: cargo test --lib formats::tests::real_azw3 -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_azw3() {
        let calibre = find_calibre().expect("Calibre is not installed");
        let dir = std::env::var("USERPROFILE").map(PathBuf::from).unwrap().join("Documents/StandardIssue Library/epubs");
        let src = walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .find(|e| e.path().to_string_lossy().ends_with("(Standard Ebooks).epub"))
            .unwrap();
        let t = tempfile::tempdir().unwrap();
        let epub = t.path().join(src.file_name());
        std::fs::copy(src.path(), &epub).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let out = rt.block_on(make_azw3(&calibre, &epub, &AtomicBool::new(false))).unwrap();
        let size = std::fs::metadata(&out).unwrap().len();
        println!("{} → {} bytes", src.path().display(), size);
        assert!(size > 10_000);
        let kepub = rt.block_on(make_kepub(&epub)).unwrap();
        println!("kepub: {kepub:?}");
        assert!(!status(&epub, true).unwrap().azw3.unwrap().stale);
    }
}
