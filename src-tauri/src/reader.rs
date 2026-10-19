// Michael Burgus — https://github.com/NeuralDrifter
// Reading state (positions, bookmarks, annotations, reading pace, reader settings), stored in reading.json

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

const LAYOUTS: &[&str] = &["paginated", "spread", "scroll"];
const THEMES: &[&str] = &["obsidian", "parchment", "cream", "oled"];
const FONTS: &[&str] = &["publisher", "serif", "sans", "mono"];
const LINE_HEIGHTS: &[&str] = &["compact", "normal", "relaxed"];
const MEASURES: &[&str] = &["compact", "optimal", "wide"];
pub const HIGHLIGHT_COLORS: &[&str] = &["gold", "sage", "rose", "sky"];

const FONT_PX_MIN: f32 = 12.0;
const FONT_PX_MAX: f32 = 32.0;
const FONT_PX_DEFAULT: f32 = 18.0;

/// Reading speed used until enough real reading has been measured.
pub const DEFAULT_WPM: u32 = 250;
/// Measured seconds needed before the measured pace is trusted.
const PACE_MIN_SECONDS: f64 = 300.0;
/// Past this much measured time, old totals are halved so the pace follows recent reading.
const PACE_DECAY_SECONDS: f64 = 4.0 * 3600.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderSettings {
    pub layout: String,
    pub theme: String,
    pub font_family: String,
    /// Body text size in CSS px (older versions stored a percentage; see `clamp`).
    pub font_size: f32,
    pub line_height: String,
    /// Line length: compact / optimal / wide.
    pub measure: String,
    pub justify: bool,
    pub drop_cap: bool,
    pub toc_open: bool,
    pub typeset_open: bool,
    /// Fields written by later versions are kept, not dropped.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ReaderSettings {
    fn default() -> Self {
        Self {
            layout: "paginated".into(),
            theme: "obsidian".into(),
            font_family: "publisher".into(),
            font_size: FONT_PX_DEFAULT,
            line_height: "normal".into(),
            measure: "optimal".into(),
            justify: true,
            drop_cap: true,
            toc_open: true,
            typeset_open: false,
            extra: Map::new(),
        }
    }
}

impl ReaderSettings {
    /// Force every field into its allowed range, upgrading values written by older versions.
    pub fn clamp(&mut self) {
        let d = Self::default();
        let legacy_theme = match self.theme.as_str() {
            "light" => Some("cream"),
            "sepia" => Some("parchment"),
            "dark" => Some("obsidian"),
            _ => None,
        };
        if let Some(t) = legacy_theme {
            self.theme = t.into();
        }
        // Older versions stored a percentage (70–200) of the book's own size
        if self.font_size > 40.0 {
            self.font_size = FONT_PX_DEFAULT * self.font_size / 100.0;
        }
        let pick = |v: &mut String, allowed: &[&str], fallback: &str| {
            if !allowed.contains(&v.as_str()) {
                *v = fallback.to_string();
            }
        };
        pick(&mut self.layout, LAYOUTS, &d.layout);
        pick(&mut self.theme, THEMES, &d.theme);
        pick(&mut self.font_family, FONTS, &d.font_family);
        pick(&mut self.line_height, LINE_HEIGHTS, &d.line_height);
        pick(&mut self.measure, MEASURES, &d.measure);
        self.font_size = if self.font_size.is_finite() {
            ((self.font_size * 2.0).round() / 2.0).clamp(FONT_PX_MIN, FONT_PX_MAX)
        } else {
            FONT_PX_DEFAULT
        };
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookProgress {
    pub cfi: String,
    pub fraction: f64,
    pub updated_at: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    pub cfi: String,
    /// Chapter name shown in the bookmarks list.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub fraction: f64,
    #[serde(default)]
    pub created_at: u64,
}

/// A highlighted passage, optionally with a note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    /// Range CFI of the highlighted text; identifies the annotation.
    pub cfi: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub note: String,
    /// Chapter name shown in the annotations list.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub fraction: f64,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
}

/// Accumulated measured reading: words read during page turns and the time it took.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pace {
    pub words: f64,
    pub seconds: f64,
}

impl Pace {
    pub fn add(&mut self, words: f64, seconds: f64) {
        if !(words.is_finite() && seconds.is_finite()) || words <= 0.0 || seconds <= 0.0 {
            return;
        }
        self.words += words;
        self.seconds += seconds;
        if self.seconds > PACE_DECAY_SECONDS {
            self.words /= 2.0;
            self.seconds /= 2.0;
        }
    }

    /// True once enough reading has been timed to trust the measurement.
    pub fn measured(&self) -> bool {
        self.seconds >= PACE_MIN_SECONDS && self.words > 0.0
    }

    /// Measured words per minute, or the default until enough reading has been timed.
    pub fn wpm(&self) -> u32 {
        if !self.measured() {
            return DEFAULT_WPM;
        }
        ((self.words / self.seconds * 60.0).round() as u32).clamp(60, 1500)
    }

    pub fn info(&self) -> PaceInfo {
        PaceInfo { wpm: self.wpm(), measured: self.measured() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PaceInfo {
    pub wpm: u32,
    pub measured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadingState {
    pub version: u32,
    pub settings: ReaderSettings,
    pub books: BTreeMap<String, BookProgress>,
    pub bookmarks: BTreeMap<String, Vec<Bookmark>>,
    pub annotations: BTreeMap<String, Vec<Annotation>>,
    pub pace: Pace,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ReadingState {
    fn default() -> Self {
        Self {
            version: 1,
            settings: ReaderSettings::default(),
            books: BTreeMap::new(),
            bookmarks: BTreeMap::new(),
            annotations: BTreeMap::new(),
            pace: Pace::default(),
            extra: Map::new(),
        }
    }
}

pub struct ReadingStore {
    path: PathBuf,
    state: Mutex<ReadingState>,
}

impl ReadingStore {
    /// Load reading.json. A corrupt file is moved aside and defaults are used;
    /// the returned warning describes what happened.
    pub fn open(path: PathBuf) -> (Self, Option<String>) {
        let (state, warning) = match std::fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (ReadingState::default(), None),
            Err(e) => (ReadingState::default(), Some(format!("could not read reading.json: {e}"))),
            Ok(data) => match serde_json::from_str::<ReadingState>(&data) {
                Ok(mut s) => {
                    s.settings.clamp();
                    (s, None)
                }
                Err(e) => {
                    let aside = path.with_file_name(format!(
                        "reading.json.corrupt-{}",
                        crate::instance::unix_now()
                    ));
                    let _ = std::fs::rename(&path, &aside);
                    (
                        ReadingState::default(),
                        Some(format!(
                            "reading.json was corrupt ({e}); moved to {}",
                            aside.display()
                        )),
                    )
                }
            },
        };
        (Self { path, state: Mutex::new(state) }, warning)
    }

    pub fn get_settings(&self) -> ReaderSettings {
        self.state.lock().unwrap().settings.clone()
    }

    pub fn save_settings(&self, mut settings: ReaderSettings) -> Result<ReaderSettings, String> {
        settings.clamp();
        let mut state = self.state.lock().unwrap();
        state.settings = settings.clone();
        self.persist(&state)?;
        Ok(settings)
    }

    pub fn progress(&self, repo: &str) -> Option<BookProgress> {
        self.state.lock().unwrap().books.get(repo).cloned()
    }

    pub fn all_progress(&self) -> BTreeMap<String, BookProgress> {
        self.state.lock().unwrap().books.clone()
    }

    pub fn save_progress(&self, repo: &str, cfi: &str, fraction: f64) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        let extra = state.books.get(repo).map(|p| p.extra.clone()).unwrap_or_default();
        state.books.insert(
            repo.to_string(),
            BookProgress {
                cfi: cfi.to_string(),
                fraction: fraction.clamp(0.0, 1.0),
                updated_at: crate::instance::unix_now(),
                extra,
            },
        );
        self.persist(&state)
    }

    pub fn bookmarks(&self, repo: &str) -> Vec<Bookmark> {
        self.state.lock().unwrap().bookmarks.get(repo).cloned().unwrap_or_default()
    }

    /// Add a bookmark (replacing one at the same CFI); returns the book's list in reading order.
    pub fn add_bookmark(&self, repo: &str, mut mark: Bookmark) -> Result<Vec<Bookmark>, String> {
        mark.fraction = if mark.fraction.is_finite() { mark.fraction.clamp(0.0, 1.0) } else { 0.0 };
        if mark.created_at == 0 {
            mark.created_at = crate::instance::unix_now();
        }
        let mut state = self.state.lock().unwrap();
        let list = state.bookmarks.entry(repo.to_string()).or_default();
        list.retain(|b| b.cfi != mark.cfi);
        list.push(mark);
        list.sort_by(|a, b| a.fraction.total_cmp(&b.fraction));
        let out = list.clone();
        self.persist(&state)?;
        Ok(out)
    }

    pub fn remove_bookmark(&self, repo: &str, cfi: &str) -> Result<Vec<Bookmark>, String> {
        let mut state = self.state.lock().unwrap();
        let Some(list) = state.bookmarks.get_mut(repo) else {
            return Ok(Vec::new());
        };
        list.retain(|b| b.cfi != cfi);
        let out = list.clone();
        if out.is_empty() {
            state.bookmarks.remove(repo);
        }
        self.persist(&state)?;
        Ok(out)
    }

    pub fn annotations(&self, repo: &str) -> Vec<Annotation> {
        self.state.lock().unwrap().annotations.get(repo).cloned().unwrap_or_default()
    }

    /// Add an annotation, or update the one at the same CFI (keeping when it was made);
    /// returns the book's list in reading order.
    pub fn save_annotation(&self, repo: &str, mut note: Annotation) -> Result<Vec<Annotation>, String> {
        if note.cfi.is_empty() {
            return Err("annotation has no location".into());
        }
        if !HIGHLIGHT_COLORS.contains(&note.color.as_str()) {
            note.color = HIGHLIGHT_COLORS[0].into();
        }
        note.fraction = if note.fraction.is_finite() { note.fraction.clamp(0.0, 1.0) } else { 0.0 };
        let now = crate::instance::unix_now();
        note.updated_at = now;
        let mut state = self.state.lock().unwrap();
        let list = state.annotations.entry(repo.to_string()).or_default();
        match list.iter_mut().find(|a| a.cfi == note.cfi) {
            Some(old) => {
                note.created_at = old.created_at;
                *old = note;
            }
            None => {
                note.created_at = now;
                list.push(note);
            }
        }
        list.sort_by(|a, b| a.fraction.total_cmp(&b.fraction));
        let out = list.clone();
        self.persist(&state)?;
        Ok(out)
    }

    pub fn remove_annotation(&self, repo: &str, cfi: &str) -> Result<Vec<Annotation>, String> {
        let mut state = self.state.lock().unwrap();
        let Some(list) = state.annotations.get_mut(repo) else {
            return Ok(Vec::new());
        };
        list.retain(|a| a.cfi != cfi);
        let out = list.clone();
        if out.is_empty() {
            state.annotations.remove(repo);
        }
        self.persist(&state)?;
        Ok(out)
    }

    pub fn pace(&self) -> PaceInfo {
        self.state.lock().unwrap().pace.info()
    }

    /// Fold a reading session's measured words and seconds into the pace.
    pub fn record_pace(&self, words: f64, seconds: f64) -> Result<PaceInfo, String> {
        let mut state = self.state.lock().unwrap();
        state.pace.add(words, seconds);
        let info = state.pace.info();
        self.persist(&state)?;
        Ok(info)
    }

    fn persist(&self, state: &ReadingState) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(state).map_err(|e| format!("serialize: {e}"))?;
        write_atomic(&self.path, &data).map_err(|e| format!("write reading.json: {e}"))
    }
}

pub use crate::fsutil::write_atomic;

/// Annotations as Markdown: one section per chapter, each highlight quoted with its note below.
pub fn annotations_markdown(title: &str, author: &str, notes: &[Annotation]) -> String {
    let mut out = format!("# {title}\n\n");
    if !author.is_empty() {
        out.push_str(&format!("{author}\n\n"));
    }
    out.push_str(&format!(
        "{} {} from StandardIssue\n",
        notes.len(),
        if notes.len() == 1 { "annotation" } else { "annotations" }
    ));
    let mut chapter: Option<&str> = None;
    for a in notes {
        if chapter != Some(a.label.as_str()) {
            chapter = Some(a.label.as_str());
            let heading = if a.label.is_empty() { "Untitled section" } else { &a.label };
            out.push_str(&format!("\n## {heading}\n"));
        }
        out.push('\n');
        for line in a.text.trim().lines() {
            out.push_str(&format!("> {}\n", line.trim()));
        }
        let note = a.note.trim();
        if !note.is_empty() {
            out.push_str(&format!("\n{note}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn store_in(dir: &Path) -> ReadingStore {
        ReadingStore::open(dir.join("reading.json")).0
    }

    #[test]
    fn roundtrip_progress_and_settings() {
        let t = tempfile::tempdir().unwrap();
        let s = store_in(t.path());
        s.save_progress("joyce_dubliners", "epubcfi(/6/4!/4/2)", 0.37).unwrap();
        let mut settings = ReaderSettings::default();
        settings.theme = "parchment".into();
        settings.font_size = 21.5;
        s.save_settings(settings.clone()).unwrap();

        let s2 = store_in(t.path());
        let p = s2.progress("joyce_dubliners").unwrap();
        assert_eq!(p.cfi, "epubcfi(/6/4!/4/2)");
        assert_eq!(p.fraction, 0.37);
        assert_eq!(s2.get_settings(), settings);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let t = tempfile::tempdir().unwrap();
        let (s, warning) = ReadingStore::open(t.path().join("reading.json"));
        assert!(warning.is_none());
        assert_eq!(s.get_settings(), ReaderSettings::default());
        assert!(s.all_progress().is_empty());
        assert_eq!(s.pace(), PaceInfo { wpm: DEFAULT_WPM, measured: false });
    }

    #[test]
    fn corrupt_file_moved_aside_and_defaults() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("reading.json");
        std::fs::write(&path, "{ not json").unwrap();

        let (s, warning) = ReadingStore::open(path.clone());

        assert!(warning.unwrap().contains("corrupt"));
        assert!(!path.exists());
        let moved = std::fs::read_dir(t.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().starts_with("reading.json.corrupt-"));
        assert!(moved);
        assert_eq!(s.get_settings(), ReaderSettings::default());
    }

    #[test]
    fn unknown_fields_preserved() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("reading.json");
        std::fs::write(
            &path,
            r#"{"version":1,"highlights_v2":{"x":1},
                "settings":{"theme":"dark","ai_panel":true},
                "books":{"a":{"cfi":"c","fraction":0.5,"updated_at":1,"notes":[1]}}}"#,
        )
        .unwrap();

        let s = store_in(t.path());
        s.save_progress("a", "c2", 0.6).unwrap();

        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["highlights_v2"]["x"], 1);
        assert_eq!(v["settings"]["ai_panel"], true);
        assert_eq!(v["settings"]["theme"], "obsidian");
        assert_eq!(v["books"]["a"]["notes"][0], 1);
        assert_eq!(v["books"]["a"]["cfi"], "c2");
    }

    #[test]
    fn settings_clamped() {
        let t = tempfile::tempdir().unwrap();
        let s = store_in(t.path());
        let mut bad = ReaderSettings::default();
        bad.font_size = 39.0;
        bad.layout = "sideways".into();
        bad.measure = "huge".into();
        let saved = s.save_settings(bad).unwrap();
        assert_eq!(saved.font_size, 32.0);
        assert_eq!(saved.layout, "paginated");
        assert_eq!(saved.measure, "optimal");

        let mut small = ReaderSettings::default();
        small.font_size = 8.0;
        assert_eq!(s.save_settings(small).unwrap().font_size, 12.0);

        let mut odd = ReaderSettings::default();
        odd.font_size = 18.3;
        assert_eq!(s.save_settings(odd).unwrap().font_size, 18.5);
    }

    #[test]
    fn legacy_settings_upgraded() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("reading.json");
        std::fs::write(
            &path,
            r#"{"version":1,"settings":{"layout":"scroll","theme":"sepia","font_family":"serif",
                "font_size":120,"line_height":"relaxed","toc_open":false},"books":{}}"#,
        )
        .unwrap();
        let got = store_in(t.path()).get_settings();
        assert_eq!(got.theme, "parchment");
        assert_eq!(got.font_size, 21.5); // 18 px × 120 % = 21.6, rounded to the half pixel
        assert_eq!(got.layout, "scroll");
        assert!(!got.toc_open);
        assert!(got.justify && got.drop_cap);
        assert_eq!(got.measure, "optimal");

        for (old, new) in [("light", "cream"), ("dark", "obsidian")] {
            let mut s = ReaderSettings::default();
            s.theme = old.into();
            s.clamp();
            assert_eq!(s.theme, new);
        }
    }

    #[test]
    fn bookmarks_add_dedupe_remove_persist() {
        let t = tempfile::tempdir().unwrap();
        let s = store_in(t.path());
        let mark = |cfi: &str, fraction: f64| Bookmark {
            cfi: cfi.into(),
            label: format!("at {fraction}"),
            fraction,
            created_at: 0,
        };
        s.add_bookmark("a", mark("late", 0.8)).unwrap();
        s.add_bookmark("a", mark("early", 0.1)).unwrap();
        let list = s.add_bookmark("a", mark("late", 0.8)).unwrap();
        assert_eq!(list.iter().map(|b| b.cfi.as_str()).collect::<Vec<_>>(), ["early", "late"]);
        assert!(list.iter().all(|b| b.created_at > 0));

        let s2 = store_in(t.path());
        assert_eq!(s2.bookmarks("a").len(), 2);
        assert_eq!(s2.remove_bookmark("a", "early").unwrap().len(), 1);
        assert!(s2.remove_bookmark("a", "late").unwrap().is_empty());
        assert!(s2.remove_bookmark("missing", "x").unwrap().is_empty());

        let raw = std::fs::read_to_string(t.path().join("reading.json")).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert!(v["bookmarks"].get("a").is_none());
    }

    fn note(cfi: &str, fraction: f64, label: &str) -> Annotation {
        Annotation {
            cfi: cfi.into(),
            text: format!("text at {fraction}"),
            color: "sage".into(),
            note: String::new(),
            label: label.into(),
            fraction,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn annotations_upsert_sort_remove_persist() {
        let t = tempfile::tempdir().unwrap();
        let s = store_in(t.path());
        s.save_annotation("a", note("late", 0.9, "II")).unwrap();
        let list = s.save_annotation("a", note("early", 0.2, "I")).unwrap();
        assert_eq!(list.iter().map(|a| a.cfi.as_str()).collect::<Vec<_>>(), ["early", "late"]);
        let made = list[1].created_at;
        assert!(made > 0);

        // Same CFI updates in place, keeps its creation time, bad colour falls back
        let mut edit = note("late", 0.9, "II");
        edit.note = "worth rereading".into();
        edit.color = "purple".into();
        let list = s.save_annotation("a", edit).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].note, "worth rereading");
        assert_eq!(list[1].color, "gold");
        assert_eq!(list[1].created_at, made);

        assert!(s.save_annotation("a", note("", 0.5, "I")).is_err());

        let s2 = store_in(t.path());
        assert_eq!(s2.annotations("a").len(), 2);
        assert_eq!(s2.remove_annotation("a", "early").unwrap().len(), 1);
        assert!(s2.remove_annotation("a", "late").unwrap().is_empty());
        assert!(s2.remove_annotation("missing", "x").unwrap().is_empty());
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(t.path().join("reading.json")).unwrap()).unwrap();
        assert!(v["annotations"].get("a").is_none());
    }

    #[test]
    fn annotations_export_as_markdown() {
        let mut first = note("a", 0.1, "I: The Beginning");
        first.text = "It was a\n  dark night.".into();
        first.note = "  Opening line.  ".into();
        let second = note("b", 0.2, "I: The Beginning");
        let third = note("c", 0.5, "");
        let md = annotations_markdown("The Lost World", "Arthur Conan Doyle", &[first, second, third]);
        assert_eq!(
            md,
            "# The Lost World\n\nArthur Conan Doyle\n\n3 annotations from StandardIssue\n\
             \n## I: The Beginning\n\n> It was a\n> dark night.\n\nOpening line.\n\
             \n> text at 0.2\n\
             \n## Untitled section\n\n> text at 0.5\n"
        );
        assert!(annotations_markdown("T", "", &[]).starts_with("# T\n\n0 annotations"));
    }

    #[test]
    fn pace_needs_enough_reading_and_follows_recent_speed() {
        let mut p = Pace::default();
        assert_eq!(p.wpm(), DEFAULT_WPM);
        p.add(400.0, 120.0); // 200 wpm, but only 2 minutes measured
        assert_eq!(p.wpm(), DEFAULT_WPM);
        p.add(1000.0, 300.0);
        assert_eq!(p.wpm(), 200);
        p.add(-5.0, 10.0);
        p.add(10.0, f64::NAN);
        assert_eq!(p.wpm(), 200);

        // Hours of faster reading pull the average toward the new speed
        for _ in 0..40 {
            p.add(3000.0, 600.0); // 300 wpm
        }
        assert!(p.seconds <= PACE_DECAY_SECONDS);
        assert!(p.wpm() > 290, "pace {}", p.wpm());

        let t = tempfile::tempdir().unwrap();
        let s = store_in(t.path());
        assert_eq!(s.record_pace(1500.0, 360.0).unwrap(), PaceInfo { wpm: 250, measured: true });
        assert_eq!(store_in(t.path()).pace().wpm, 250);
    }

    #[test]
    fn atomic_write_leaves_no_tmp() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("reading.json");
        write_atomic(&path, b"{}").unwrap();
        write_atomic(&path, b"{\"version\":1}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"version\":1}");
        assert!(!t.path().join("reading.json.tmp").exists());
    }
}
