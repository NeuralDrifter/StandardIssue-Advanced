// Michael Burgus — https://github.com/NeuralDrifter
// Books JSON storage, per-book updates, epub location tracking and eras

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Published {
    /// Start year (negative = BCE). Ranges and centuries use their first year.
    pub year: i32,
    /// As the colophon puts it: "1912", "1855–1863", "11th century CE".
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CoverArt {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub artist_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Book {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cover: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub epub: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub long_description: String,
    #[serde(default, rename = "abstract", skip_serializing_if = "String::is_empty")]
    pub abstract_text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub se_published: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub word_count: i64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub reading_ease: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reading_time: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub wikipedia: String,
    /// Genres (schema:genre), formerly se:subject.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub series: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub series_position: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<Published>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_art: Option<CoverArt>,
    /// Version of the OPF parser that last enriched this book (0 = never).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub meta_rev: u32,
    /// Version of the colophon parser that last enriched this book (0 = never).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub colophon_rev: u32,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}
fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}
fn is_zero_f64(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Default)]
struct Catalog {
    books: Vec<Book>,
    index: HashMap<String, usize>,
}

impl Catalog {
    fn new(books: Vec<Book>) -> Self {
        let index = books
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let rn = repo_name(b);
                (!rn.is_empty()).then_some((rn, i))
            })
            .collect();
        Self { books, index }
    }
}

pub struct BookStore {
    catalog: RwLock<Catalog>,
    /// repo -> epub file found on disk
    epubs: RwLock<HashMap<String, PathBuf>>,
    path: PathBuf,
    pub epubs_dir: PathBuf,
    pub covers_dir: PathBuf,
    /// When on, new epubs go into genre folders; off keeps the Uncategorized layout.
    organize: AtomicBool,
}

impl BookStore {
    pub fn new(json_path: PathBuf, epubs_dir: PathBuf, covers_dir: PathBuf) -> Self {
        Self {
            catalog: RwLock::new(Catalog::default()),
            epubs: RwLock::new(HashMap::new()),
            path: json_path,
            epubs_dir,
            covers_dir,
            organize: AtomicBool::new(false),
        }
    }

    pub fn set_organize(&self, on: bool) {
        self.organize.store(on, Ordering::SeqCst);
    }

    pub fn organize(&self) -> bool {
        self.organize.load(Ordering::SeqCst)
    }

    pub fn load(&self) -> Result<(), String> {
        let data = match std::fs::read_to_string(&self.path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(format!("read books.json: {e}")),
        };
        let books: Vec<Book> =
            serde_json::from_str(&data).map_err(|e| format!("parse books.json: {e}"))?;
        *self.catalog.write().unwrap() = Catalog::new(books);
        self.refresh_download_status();
        Ok(())
    }

    pub fn save(&self) -> Result<(), String> {
        let catalog = self.catalog.read().unwrap();
        crate::fsutil::save_json(&self.path, &catalog.books)
    }

    /// Run `f` over the books without cloning them.
    pub fn read<R>(&self, f: impl FnOnce(&[Book]) -> R) -> R {
        f(&self.catalog.read().unwrap().books)
    }

    pub fn all(&self) -> Vec<Book> {
        self.read(|b| b.to_vec())
    }

    pub fn get(&self, repo: &str) -> Option<Book> {
        let c = self.catalog.read().unwrap();
        c.index.get(repo).map(|&i| c.books[i].clone())
    }

    /// Change one book in place. Returns false if the repo is unknown.
    pub fn update(&self, repo: &str, f: impl FnOnce(&mut Book)) -> bool {
        let mut c = self.catalog.write().unwrap();
        match c.index.get(repo).copied() {
            Some(i) => {
                f(&mut c.books[i]);
                true
            }
            None => false,
        }
    }

    pub fn replace_all(&self, books: Vec<Book>) {
        *self.catalog.write().unwrap() = Catalog::new(books);
    }

    /// Replace the catalog with `fresh`, keeping the stored (enriched) entry for
    /// every repo that already exists. Done under one lock so a concurrent
    /// update is never lost. Returns (new, removed, total).
    pub fn merge_catalog(&self, fresh: Vec<Book>) -> (usize, usize, usize) {
        let mut c = self.catalog.write().unwrap();
        let fresh_repos: std::collections::HashSet<String> = fresh.iter().map(repo_name).collect();
        let removed = c.index.keys().filter(|r| !fresh_repos.contains(*r)).count();
        let mut new = 0;
        let merged: Vec<Book> = fresh
            .into_iter()
            .map(|b| match c.index.get(&repo_name(&b)) {
                Some(&i) => c.books[i].clone(),
                None => {
                    new += 1;
                    b
                }
            })
            .collect();
        let total = merged.len();
        *c = Catalog::new(merged);
        (new, removed, total)
    }

    pub fn count(&self) -> usize {
        self.catalog.read().unwrap().books.len()
    }

    /// Where a new epub for this book is written.
    pub fn target_path(&self, b: &Book) -> PathBuf {
        epub_path(&self.epubs_dir, b, self.organize())
    }

    /// An existing epub for this book: genre path, Uncategorized path, then flat `{repo}.epub`.
    pub fn locate(&self, b: &Book) -> Option<PathBuf> {
        let rn = repo_name(b);
        if rn.is_empty() {
            return None;
        }
        [
            epub_path(&self.epubs_dir, b, true),
            epub_path(&self.epubs_dir, b, false),
            self.epubs_dir.join(format!("{rn}.epub")),
        ]
        .into_iter()
        .find(|p| p.is_file())
    }

    pub fn refresh_download_status(&self) {
        let found: HashMap<String, PathBuf> = self.read(|books| {
            books
                .iter()
                .filter_map(|b| self.locate(b).map(|p| (repo_name(b), p)))
                .collect()
        });
        *self.epubs.write().unwrap() = found;
    }

    pub fn refresh_one(&self, repo: &str) {
        let found = self.get(repo).and_then(|b| self.locate(&b));
        let mut epubs = self.epubs.write().unwrap();
        match found {
            Some(p) => epubs.insert(repo.to_string(), p),
            None => epubs.remove(repo),
        };
    }

    pub fn epub_file(&self, repo: &str) -> Option<PathBuf> {
        self.epubs.read().unwrap().get(repo).cloned()
    }

    pub fn is_downloaded(&self, repo: &str) -> bool {
        self.epubs.read().unwrap().contains_key(repo)
    }

    pub fn count_downloaded(&self) -> usize {
        self.epubs.read().unwrap().len()
    }

    pub fn cover_file(&self, repo: &str) -> PathBuf {
        self.covers_dir.join(format!("{repo}.jpg"))
    }
}

/// Derive repo name from a book's repo_name field or URL.
pub fn repo_name(b: &Book) -> String {
    if !b.repo_name.is_empty() {
        return b.repo_name.clone();
    }
    if let Some(idx) = b.url.find("/ebooks/") {
        let path = b.url[idx + "/ebooks/".len()..].trim_end_matches('/');
        return path.replace('/', "_");
    }
    String::new()
}

/// Epub path: {epubs_dir}/{Genre or Uncategorized}/{Author}/{Series?}/{Title}_{Author} (Standard Ebooks).epub
pub fn epub_path(epubs_dir: &Path, b: &Book, by_genre: bool) -> PathBuf {
    let rn = repo_name(b);
    if rn.is_empty() {
        return PathBuf::new();
    }

    let category = match b.categories.first() {
        Some(c) if by_genre => sanitize_path(c),
        _ => "Uncategorized".to_string(),
    };
    let author = if b.author.is_empty() {
        "Unknown".to_string()
    } else {
        sanitize_path(&b.author)
    };
    let title = if b.title.is_empty() {
        rn
    } else {
        sanitize_path(&b.title)
    };
    let filename = format!("{title}_{author} (Standard Ebooks).epub");

    let mut dir = epubs_dir.join(category).join(author);
    if !b.series.is_empty() {
        dir = dir.join(sanitize_path(&b.series));
    }
    dir.join(filename)
}

/// Remove characters unsafe for filenames/directories.
pub fn sanitize_path(s: &str) -> String {
    let s = s
        .trim()
        .replace(['/', '\\'], "-")
        .replace(':', " -")
        .replace(['*', '?'], "")
        .replace('"', "'")
        .replace(['<', '>'], "")
        .replace('|', "-");
    let collapsed = s.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    collapsed.trim_end_matches(['.', ' ']).to_string()
}

// --- Eras ---

pub const UNDATED: &str = "Undated";

/// Era buckets in display order.
pub const ERAS: [&str; 9] = [
    "Antiquity",
    "Medieval",
    "1500–1699",
    "1700s",
    "1800–1849",
    "1850–1899",
    "1900–1929",
    "1930 & later",
    UNDATED,
];

pub fn era_of_year(year: i32) -> &'static str {
    match year {
        i32::MIN..=499 => ERAS[0],
        500..=1499 => ERAS[1],
        1500..=1699 => ERAS[2],
        1700..=1799 => ERAS[3],
        1800..=1849 => ERAS[4],
        1850..=1899 => ERAS[5],
        1900..=1929 => ERAS[6],
        _ => ERAS[7],
    }
}

pub fn era_of(b: &Book) -> &'static str {
    b.published.as_ref().map_or(UNDATED, |p| era_of_year(p.year))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(repo: &str, title: &str) -> Book {
        Book {
            title: title.into(),
            author: "Arthur Conan Doyle".into(),
            repo_name: repo.into(),
            ..Default::default()
        }
    }

    fn store(dir: &Path) -> BookStore {
        BookStore::new(dir.join("books.json"), dir.join("epubs"), dir.join("covers"))
    }

    #[test]
    fn update_changes_one_book_and_keeps_other_fields() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        s.replace_all(vec![book("a", "A"), book("b", "B")]);

        // An enrichment pass and a build both touching book "a" must not clobber each other.
        assert!(s.update("a", |b| b.word_count = 100));
        assert!(s.update("a", |b| b.categories = vec!["Fantasy".into()]));
        assert!(!s.update("missing", |b| b.word_count = 1));

        let a = s.get("a").unwrap();
        assert_eq!(a.word_count, 100);
        assert_eq!(a.categories, vec!["Fantasy".to_string()]);
        assert_eq!(s.get("b").unwrap().word_count, 0);
    }

    #[test]
    fn save_load_roundtrip_with_new_fields_and_old_files_still_load() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let mut b = book("a", "A");
        b.published = Some(Published { year: 1912, label: "1912".into() });
        b.abstract_text = "short".into();
        b.meta_rev = 2;
        s.replace_all(vec![b]);
        s.save().unwrap();

        let s2 = store(t.path());
        s2.load().unwrap();
        let a = s2.get("a").unwrap();
        assert_eq!(a.published.unwrap().year, 1912);
        assert_eq!(a.abstract_text, "short");
        assert_eq!(a.meta_rev, 2);

        std::fs::write(t.path().join("books.json"), r#"[{"title":"Old","repo_name":"old"}]"#).unwrap();
        s2.load().unwrap();
        assert_eq!(s2.get("old").unwrap().meta_rev, 0);
    }

    #[test]
    fn locate_finds_genre_uncategorized_and_flat_files() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let mut b = book("doyle_lost-world", "The Lost World");
        b.categories = vec!["Fantasy".into()];
        s.replace_all(vec![b.clone()]);

        let old = epub_path(&s.epubs_dir, &b, false);
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "x").unwrap();
        s.refresh_download_status();
        assert_eq!(s.epub_file("doyle_lost-world"), Some(old.clone()));
        assert!(old.to_string_lossy().contains("Uncategorized"));

        std::fs::remove_file(&old).unwrap();
        let flat = s.epubs_dir.join("doyle_lost-world.epub");
        std::fs::write(&flat, "x").unwrap();
        s.refresh_one("doyle_lost-world");
        assert_eq!(s.epub_file("doyle_lost-world"), Some(flat.clone()));

        std::fs::remove_file(&flat).unwrap();
        s.refresh_one("doyle_lost-world");
        assert!(!s.is_downloaded("doyle_lost-world"));
    }

    #[test]
    fn target_path_follows_organize_switch() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        let mut b = book("a", "A");
        b.categories = vec!["Fantasy".into()];
        assert!(s.target_path(&b).to_string_lossy().contains("Uncategorized"));
        s.set_organize(true);
        assert!(s.target_path(&b).to_string_lossy().contains("Fantasy"));
    }

    #[test]
    fn merge_catalog_keeps_enriched_entries() {
        let t = tempfile::tempdir().unwrap();
        let s = store(t.path());
        s.replace_all(vec![book("a", "A"), book("gone", "G")]);
        s.update("a", |b| b.word_count = 9);
        let (new, removed, total) = s.merge_catalog(vec![book("a", "A fresh"), book("b", "B")]);
        assert_eq!((new, removed, total), (1, 1, 2));
        assert_eq!(s.get("a").unwrap().word_count, 9);
        assert!(s.get("gone").is_none());
        assert_eq!(s.get("b").unwrap().title, "B");
    }

    #[test]
    fn era_boundaries() {
        assert_eq!(era_of_year(-400), "Antiquity");
        assert_eq!(era_of_year(499), "Antiquity");
        assert_eq!(era_of_year(500), "Medieval");
        assert_eq!(era_of_year(1499), "Medieval");
        assert_eq!(era_of_year(1500), "1500–1699");
        assert_eq!(era_of_year(1799), "1700s");
        assert_eq!(era_of_year(1849), "1800–1849");
        assert_eq!(era_of_year(1850), "1850–1899");
        assert_eq!(era_of_year(1929), "1900–1929");
        assert_eq!(era_of_year(1930), "1930 & later");
        assert_eq!(era_of(&Book::default()), UNDATED);
    }

    #[test]
    fn sanitize_collapses_spaces_and_strips_unsafe() {
        assert_eq!(sanitize_path(" A:  B?/C. "), "A - B-C");
    }
}
