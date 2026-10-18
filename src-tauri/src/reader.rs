// Michael Burgus — https://github.com/NeuralDrifter
// Reading state (positions + reader settings), stored in reading.json

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

const LAYOUTS: &[&str] = &["paginated", "spread", "scroll"];
const THEMES: &[&str] = &["light", "sepia", "dark"];
const FONTS: &[&str] = &["publisher", "serif", "sans"];
const LINE_HEIGHTS: &[&str] = &["compact", "normal", "relaxed"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReaderSettings {
    pub layout: String,
    pub theme: String,
    pub font_family: String,
    pub font_size: u32,
    pub line_height: String,
    pub toc_open: bool,
    /// Fields written by later versions are kept, not dropped.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ReaderSettings {
    fn default() -> Self {
        Self {
            layout: "paginated".into(),
            theme: "light".into(),
            font_family: "publisher".into(),
            font_size: 100,
            line_height: "normal".into(),
            toc_open: false,
            extra: Map::new(),
        }
    }
}

impl ReaderSettings {
    /// Force every field into its allowed range.
    pub fn clamp(&mut self) {
        let d = Self::default();
        let pick = |v: &mut String, allowed: &[&str], fallback: &str| {
            if !allowed.contains(&v.as_str()) {
                *v = fallback.to_string();
            }
        };
        pick(&mut self.layout, LAYOUTS, &d.layout);
        pick(&mut self.theme, THEMES, &d.theme);
        pick(&mut self.font_family, FONTS, &d.font_family);
        pick(&mut self.line_height, LINE_HEIGHTS, &d.line_height);
        let rounded = ((self.font_size + 5) / 10) * 10;
        self.font_size = rounded.clamp(70, 200);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadingState {
    pub version: u32,
    pub settings: ReaderSettings,
    pub books: BTreeMap<String, BookProgress>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for ReadingState {
    fn default() -> Self {
        Self {
            version: 1,
            settings: ReaderSettings::default(),
            books: BTreeMap::new(),
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

    fn persist(&self, state: &ReadingState) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(state).map_err(|e| format!("serialize: {e}"))?;
        write_atomic(&self.path, &data).map_err(|e| format!("write reading.json: {e}"))
    }
}

pub use crate::fsutil::write_atomic;

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
        settings.theme = "sepia".into();
        settings.font_size = 130;
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
            r#"{"version":1,"annotations":{"x":1},
                "settings":{"theme":"dark","ai_panel":true},
                "books":{"a":{"cfi":"c","fraction":0.5,"updated_at":1,"bookmarks":[1]}}}"#,
        )
        .unwrap();

        let s = store_in(t.path());
        s.save_progress("a", "c2", 0.6).unwrap();

        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["annotations"]["x"], 1);
        assert_eq!(v["settings"]["ai_panel"], true);
        assert_eq!(v["settings"]["theme"], "dark");
        assert_eq!(v["books"]["a"]["bookmarks"][0], 1);
        assert_eq!(v["books"]["a"]["cfi"], "c2");
    }

    #[test]
    fn settings_clamped() {
        let t = tempfile::tempdir().unwrap();
        let s = store_in(t.path());
        let mut bad = ReaderSettings::default();
        bad.font_size = 999;
        bad.layout = "sideways".into();
        let saved = s.save_settings(bad).unwrap();
        assert_eq!(saved.font_size, 200);
        assert_eq!(saved.layout, "paginated");

        let mut small = ReaderSettings::default();
        small.font_size = 64;
        assert_eq!(s.save_settings(small).unwrap().font_size, 70);
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
