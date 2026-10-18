// Michael Burgus — https://github.com/NeuralDrifter
// Metadata enrichment from each book's GitHub content.opf and colophon

use crate::books::{repo_name, Book, BookStore, CoverArt};
use crate::colophon::{self, ColophonInfo, COLOPHON_REV};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

const ORG: &str = "standardebooks";

/// Bump when the OPF parser learns something new, so books get re-enriched.
/// 2 = schema.org vocabulary (schema:genre, schema:wordCount, …).
pub const META_REV: u32 = 2;

/// Save books.json after this many enriched books, so a crash loses little.
const SAVE_EVERY: usize = 50;

pub struct EnrichResult {
    pub enriched: usize,
    pub skipped: usize,
    pub errors: usize,
}

/// What a book still needs from GitHub.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Needs {
    pub opf: bool,
    pub colophon: bool,
}

impl Needs {
    pub fn of(b: &Book, want_colophon: bool) -> Self {
        Self {
            opf: b.meta_rev < META_REV,
            colophon: want_colophon && b.colophon_rev < COLOPHON_REV,
        }
    }

    pub fn any(self) -> bool {
        self.opf || self.colophon
    }
}

/// Downloaded metadata for one book, applied later with [`apply_enrichment`].
#[derive(Default)]
pub struct Enrichment {
    pub opf: Option<OpfMeta>,
    /// `Some(None)` = fetched, but the repo has no colophon.
    pub colophon: Option<Option<ColophonInfo>>,
}

/// Fetch whatever `needs` asks for. A missing colophon is not an error.
pub async fn fetch_enrichment(client: &reqwest::Client, repo: &str, needs: Needs) -> Result<Enrichment, String> {
    let mut out = Enrichment::default();
    if needs.opf {
        out.opf = Some(fetch_and_parse_opf(client, repo).await?);
    }
    if needs.colophon {
        out.colophon = Some(match colophon::fetch_colophon(client, repo).await? {
            Some(xhtml) => Some(colophon::parse_colophon(&xhtml)?),
            None => None,
        });
    }
    Ok(out)
}

pub fn apply_enrichment(book: &mut Book, e: &Enrichment) {
    if let Some(meta) = &e.opf {
        apply_metadata(book, meta);
    }
    if let Some(col) = &e.colophon {
        if let Some(info) = col {
            book.published = info.published.clone();
            if let Some(art) = &info.art {
                let merged = book.cover_art.get_or_insert_with(CoverArt::default);
                merged.title = art.title.clone();
                merged.year = art.year;
                if merged.artist.is_empty() {
                    merged.artist = art.artist.clone();
                }
                if merged.artist_url.is_empty() {
                    merged.artist_url = art.artist_url.clone();
                }
            }
        }
        book.colophon_rev = COLOPHON_REV;
    }
}

pub async fn enrich_all(
    store: &BookStore,
    client: &reqwest::Client,
    concurrency: usize,
    want_colophon: bool,
    progress: impl Fn(usize, usize, String, String),
    cancel: Arc<AtomicBool>,
) -> EnrichResult {
    let jobs: Vec<(String, String, Needs)> = store.read(|books| {
        books
            .iter()
            .filter_map(|b| {
                let needs = Needs::of(b, want_colophon);
                let rn = repo_name(b);
                (needs.any() && !rn.is_empty()).then(|| (rn, b.title.clone(), needs))
            })
            .collect()
    });
    let total = jobs.len();
    let skipped = store.count() - total;

    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut set = JoinSet::new();
    for (repo, title, needs) in jobs {
        let (sem, client, cancel) = (sem.clone(), client.clone(), cancel.clone());
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok()?;
            if cancel.load(Ordering::SeqCst) {
                return None;
            }
            Some((repo.clone(), title, fetch_enrichment(&client, &repo, needs).await))
        });
    }

    let (mut done, mut enriched, mut errors) = (0, 0, 0);
    while let Some(joined) = set.join_next().await {
        let Ok(Some((repo, title, result))) = joined else { continue };
        done += 1;
        match result {
            Ok(e) => {
                store.update(&repo, |b| apply_enrichment(b, &e));
                enriched += 1;
                progress(done, total, title, "enriched".into());
                if enriched % SAVE_EVERY == 0 {
                    let _ = store.save();
                }
            }
            Err(e) => {
                errors += 1;
                progress(done, total, title, format!("error: {e}"));
            }
        }
    }
    let _ = store.save();

    EnrichResult { enriched, skipped, errors }
}

// --- OPF ---

#[derive(Default)]
pub struct OpfMeta {
    description: String,
    language: String,
    date: String,
    subjects: Vec<String>,
    sources: Vec<String>,
    metas: Vec<MetaEntry>,
    /// dc:creator / dc:contributor by id
    people: HashMap<String, String>,
    links: Vec<LinkEntry>,
}

struct MetaEntry {
    id: String,
    property: String,
    refines: String,
    text: String,
}

struct LinkEntry {
    href: String,
    rel: String,
    refines: String,
}

pub async fn fetch_and_parse_opf(client: &reqwest::Client, repo_name: &str) -> Result<OpfMeta, String> {
    let url = format!("https://raw.githubusercontent.com/{ORG}/{repo_name}/master/src/epub/content.opf");
    let resp = client.get(&url).send().await.map_err(|e| format!("{e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body = resp.text().await.map_err(|e| format!("{e}"))?;
    parse_opf_xml(&body)
}

pub fn parse_opf_xml(xml: &str) -> Result<OpfMeta, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("XML parse: {e}"))?;
    let metadata = doc
        .descendants()
        .find(|n| n.has_tag_name("metadata"))
        .ok_or("no metadata element")?;

    let mut meta = OpfMeta::default();
    for child in metadata.children().filter(|n| n.is_element()) {
        let text = child.text().unwrap_or("").trim().to_string();
        let attr = |name| child.attribute(name).unwrap_or("").to_string();
        match child.tag_name().name() {
            "description" if meta.description.is_empty() => meta.description = text,
            "language" if meta.language.is_empty() => meta.language = text,
            "date" if meta.date.is_empty() => meta.date = text,
            "subject" if !text.is_empty() => meta.subjects.push(text),
            "source" if !text.is_empty() => meta.sources.push(text),
            "creator" | "contributor" => {
                let id = attr("id");
                if !id.is_empty() {
                    meta.people.insert(id, text);
                }
            }
            "meta" => {
                let property = attr("property");
                if !property.is_empty() {
                    meta.metas.push(MetaEntry { id: attr("id"), property, refines: attr("refines"), text });
                }
            }
            "link" => meta.links.push(LinkEntry { href: attr("href"), rel: attr("rel"), refines: attr("refines") }),
            _ => {}
        }
    }
    Ok(meta)
}

/// Copy OPF metadata onto a book. Lists are replaced, never appended, so
/// re-enriching is idempotent. Reads both the current schema.org vocabulary
/// and the older `se:` one.
pub fn apply_metadata(book: &mut Book, meta: &OpfMeta) {
    if !meta.language.is_empty() {
        book.language = meta.language.clone();
    }
    if !meta.date.is_empty() {
        book.se_published = meta.date.clone();
    }
    if !meta.subjects.is_empty() {
        book.subjects = meta.subjects.clone();
    }
    if !meta.sources.is_empty() {
        book.sources = meta.sources.clone();
    }

    let top = |props: &[&str]| -> Vec<&str> {
        meta.metas
            .iter()
            .filter(|m| m.refines.is_empty() && props.contains(&m.property.as_str()) && !m.text.is_empty())
            .map(|m| m.text.as_str())
            .collect()
    };
    let first = |props: &[&str]| top(props).first().map(|s| s.to_string());

    let genres = top(&["schema:genre", "se:subject"]);
    if !genres.is_empty() {
        book.categories = genres.iter().map(|s| s.to_string()).collect();
    }
    if let Some(n) = first(&["schema:wordCount", "se:word-count"]).and_then(|s| s.parse().ok()) {
        book.word_count = n;
    }
    if let Some(f) = first(&["schema:educationalLevel", "se:reading-ease.flesch"]).and_then(|s| s.parse().ok()) {
        book.reading_ease = f;
    }
    if let Some(a) = first(&["schema:abstract"]) {
        book.description = a.clone();
        book.abstract_text = a;
    }
    // The legacy long description refines #description, so don't require a top-level meta
    let long = meta
        .metas
        .iter()
        .find(|m| m.property == "se:long-description" && !m.text.is_empty())
        .map(|m| m.text.clone())
        .unwrap_or_else(|| meta.description.clone());
    if !long.is_empty() {
        book.long_description = long;
    }

    let wiki = meta
        .links
        .iter()
        .find(|l| l.refines.is_empty() && l.rel == "schema:sameAs" && l.href.contains("wikipedia.org"))
        .map(|l| l.href.clone())
        .or_else(|| first(&["se:url.encyclopedia.wikipedia"]));
    if let Some(w) = wiki {
        book.wikipedia = w;
    }

    // Collections and series
    fn refines_of<'a>(meta: &'a OpfMeta, id: &str) -> impl Iterator<Item = &'a MetaEntry> + 'a {
        let target = format!("#{id}");
        meta.metas.iter().filter(move |m| m.refines == target)
    }
    let mut collections = Vec::new();
    for m in meta.metas.iter().filter(|m| m.property == "belongs-to-collection" && !m.text.is_empty()) {
        collections.push(m.text.clone());
        if m.id.is_empty() || !book.series.is_empty() && book.series != m.text {
            continue;
        }
        let is_series = refines_of(meta, &m.id).any(|r| r.property == "collection-type" && r.text.trim() == "series");
        if is_series {
            book.series = m.text.clone();
            book.series_position = refines_of(meta, &m.id)
                .find(|r| r.property == "group-position")
                .and_then(|r| r.text.trim().parse().ok())
                .unwrap_or(0);
        }
    }
    if !collections.is_empty() {
        book.collections = collections;
    }

    // Cover artist (dc:contributor id="artist")
    if let Some(artist) = meta.people.get("artist").filter(|a| !a.is_empty()) {
        let url = meta
            .links
            .iter()
            .find(|l| l.refines == "#artist" && l.href.contains("wikipedia.org"))
            .map(|l| l.href.clone())
            .unwrap_or_default();
        let art = book.cover_art.get_or_insert_with(CoverArt::default);
        art.artist = artist.clone();
        art.artist_url = url;
    }

    if book.word_count > 0 {
        let minutes = book.word_count / 250;
        let (hours, mins) = (minutes / 60, minutes % 60);
        book.reading_time = if hours > 0 { format!("{hours}h {mins}m") } else { format!("{mins}m") };
    }
    book.meta_rev = META_REV;
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURRENT: &str = include_str!("../tests/fixtures/opf_current.opf");
    const LEGACY: &str = include_str!("../tests/fixtures/opf_legacy.opf");

    #[test]
    fn current_schema_vocabulary() {
        let mut b = Book::default();
        apply_metadata(&mut b, &parse_opf_xml(CURRENT).unwrap());
        assert_eq!(b.categories, vec!["Adventure", "Fantasy"]);
        assert_eq!(b.word_count, 75964);
        assert_eq!(b.reading_ease, 70.23);
        assert_eq!(b.abstract_text, "A team of scientists and adventurers explores a prehistoric land.");
        assert!(b.long_description.contains("<p>Even though"));
        assert_eq!(b.wikipedia, "https://en.wikipedia.org/wiki/The_Lost_World_%28Conan_Doyle_novel%29");
        assert_eq!(b.series, "Professor Challenger");
        assert_eq!(b.series_position, 1);
        assert_eq!(b.subjects.len(), 5);
        let art = b.cover_art.unwrap();
        assert_eq!(art.artist, "Charles R. Knight");
        assert_eq!(art.artist_url, "https://en.wikipedia.org/wiki/Charles_R._Knight");
        assert_eq!(b.meta_rev, META_REV);
        assert_eq!(b.reading_time, "5h 3m");
    }

    #[test]
    fn legacy_se_vocabulary() {
        let mut b = Book::default();
        apply_metadata(&mut b, &parse_opf_xml(LEGACY).unwrap());
        assert_eq!(b.categories, vec!["Fiction", "Adventure"]);
        assert_eq!(b.word_count, 12345);
        assert_eq!(b.reading_ease, 65.5);
        assert_eq!(b.long_description, "<p>Long.</p>");
        assert_eq!(b.wikipedia, "https://en.wikipedia.org/wiki/Old");
    }

    #[test]
    fn re_enriching_does_not_duplicate_lists() {
        let mut b = Book::default();
        let meta = parse_opf_xml(CURRENT).unwrap();
        apply_metadata(&mut b, &meta);
        apply_metadata(&mut b, &meta);
        assert_eq!(b.categories.len(), 2);
        assert_eq!(b.collections, vec!["Professor Challenger"]);
    }

    #[test]
    fn needs_follow_revisions() {
        let mut b = Book::default();
        assert_eq!(Needs::of(&b, false), Needs { opf: true, colophon: false });
        b.meta_rev = META_REV;
        assert!(!Needs::of(&b, false).any());
        assert!(Needs::of(&b, true).colophon);
        b.colophon_rev = COLOPHON_REV;
        assert!(!Needs::of(&b, true).any());
    }

    #[test]
    fn colophon_enrichment_merges_with_opf_artist() {
        let mut b = Book::default();
        apply_metadata(&mut b, &parse_opf_xml(CURRENT).unwrap());
        let e = Enrichment {
            opf: None,
            colophon: Some(Some(ColophonInfo {
                published: Some(crate::books::Published { year: 1912, label: "1912".into() }),
                art: Some(CoverArt { title: "Brontosaurus".into(), year: Some(1897), artist: "C. Knight".into(), artist_url: String::new() }),
            })),
        };
        apply_enrichment(&mut b, &e);
        let art = b.cover_art.as_ref().unwrap();
        assert_eq!((art.title.as_str(), art.year, art.artist.as_str()), ("Brontosaurus", Some(1897), "Charles R. Knight"));
        assert_eq!(b.published.as_ref().unwrap().year, 1912);
        assert_eq!(b.colophon_rev, COLOPHON_REV);

        // A repo without a colophon is still marked done
        let mut c = Book::default();
        apply_enrichment(&mut c, &Enrichment { opf: None, colophon: Some(None) });
        assert_eq!(c.colophon_rev, COLOPHON_REV);
        assert!(c.published.is_none());
    }
}
