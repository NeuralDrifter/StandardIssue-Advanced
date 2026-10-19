// Michael Burgus — https://github.com/NeuralDrifter
// App settings (settings.json) and last-run summaries of operations (activity.json)

use crate::fsutil::{load_json, save_json};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;

const PAGE_SIZES: &[u32] = &[24, 48, 96];
const VIEWS: &[&str] = &["grid", "compact", "list"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub build_concurrency: u32,
    pub enrich_concurrency: u32,
    pub page_size: u32,
    pub view: String,
    /// Default state of the Metadata / Cover art toggles on single and bulk builds.
    pub build_metadata: bool,
    pub build_covers: bool,
    /// File epubs into genre folders (see spec §5.7). Off keeps Uncategorized/.
    pub organize_by_genre: bool,
    /// Also make a Kobo KEPUB / Kindle AZW3 (needs Calibre) after each build.
    pub make_kepub: bool,
    pub make_azw3: bool,
    /// Optional; raises the GitHub API rate limit for catalog sync.
    pub github_token: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            build_concurrency: 3,
            enrich_concurrency: 10,
            page_size: 24,
            view: "grid".into(),
            build_metadata: true,
            build_covers: true,
            organize_by_genre: false,
            make_kepub: false,
            make_azw3: false,
            github_token: String::new(),
            extra: Map::new(),
        }
    }
}

impl AppSettings {
    pub fn clamp(&mut self) {
        self.build_concurrency = self.build_concurrency.clamp(1, 6);
        self.enrich_concurrency = self.enrich_concurrency.clamp(1, 20);
        if !PAGE_SIZES.contains(&self.page_size) {
            self.page_size = 24;
        }
        if !VIEWS.contains(&self.view.as_str()) {
            self.view = "grid".into();
        }
        self.github_token = self.github_token.trim().to_string();
    }
}

/// When an operation last ran and what it reported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpRecord {
    pub at: u64,
    pub summary: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Activity {
    pub last_sync: Option<OpRecord>,
    pub last_enrich: Option<OpRecord>,
    pub last_covers: Option<OpRecord>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A JSON file held in memory and written atomically on every change.
pub struct JsonStore<T> {
    path: PathBuf,
    value: Mutex<T>,
}

impl<T: Clone + Serialize + serde::de::DeserializeOwned + Default> JsonStore<T> {
    pub fn open(path: PathBuf) -> (Self, Option<String>) {
        let (value, warning) = load_json(&path);
        (Self { path, value: Mutex::new(value) }, warning)
    }

    pub fn get(&self) -> T {
        self.value.lock().unwrap().clone()
    }

    pub fn update(&self, f: impl FnOnce(&mut T)) -> Result<T, String> {
        let mut v = self.value.lock().unwrap();
        f(&mut v);
        save_json(&self.path, &*v)?;
        Ok(v.clone())
    }
}

pub type SettingsStore = JsonStore<AppSettings>;
pub type ActivityStore = JsonStore<Activity>;

impl SettingsStore {
    pub fn save(&self, mut s: AppSettings) -> Result<AppSettings, String> {
        s.clamp();
        self.update(|v| *v = s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_clamped_and_roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let (s, w) = SettingsStore::open(t.path().join("settings.json"));
        assert!(w.is_none());
        assert_eq!(s.get(), AppSettings::default());

        let bad = AppSettings {
            build_concurrency: 50,
            enrich_concurrency: 0,
            page_size: 7,
            view: "carousel".into(),
            github_token: "  tok  ".into(),
            ..Default::default()
        };
        let saved = s.save(bad).unwrap();
        assert_eq!(
            (saved.build_concurrency, saved.enrich_concurrency, saved.page_size, saved.view.as_str(), saved.github_token.as_str()),
            (6, 1, 24, "grid", "tok")
        );

        let (s2, _) = SettingsStore::open(t.path().join("settings.json"));
        assert_eq!(s2.get(), saved);
    }

    #[test]
    fn activity_records_persist() {
        let t = tempfile::tempdir().unwrap();
        let (a, _) = ActivityStore::open(t.path().join("activity.json"));
        a.update(|v| v.last_sync = Some(OpRecord { at: 5, summary: serde_json::json!({"new": 2}) })).unwrap();
        let (a2, _) = ActivityStore::open(t.path().join("activity.json"));
        assert_eq!(a2.get().last_sync.unwrap().summary["new"], 2);
    }
}
