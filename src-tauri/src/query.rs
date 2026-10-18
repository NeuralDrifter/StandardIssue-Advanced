// Michael Burgus — https://github.com/NeuralDrifter
// Library query: search, filters with facet counts, sorting and pagination

use crate::books::{era_of, repo_name, Book, ERAS};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Query {
    pub search: String,
    pub author: String,
    pub genre: String,
    pub era: String,
    /// all | built | not_built | pipeline | queued | building | failed | reading
    pub status: String,
    /// title | author | year | words | recent_build | newest
    pub sort: String,
    pub page: usize,
    pub per_page: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            search: String::new(),
            author: String::new(),
            genre: String::new(),
            era: String::new(),
            status: "all".into(),
            sort: "title".into(),
            page: 1,
            per_page: 24,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Built,
    NotBuilt,
    Queued,
    Building,
    Failed,
}

pub const STATUS_FILTERS: [&str; 8] = ["all", "built", "not_built", "pipeline", "queued", "building", "failed", "reading"];

fn status_matches(filter: &str, s: Status, reading: bool) -> bool {
    match filter {
        "built" => s == Status::Built,
        "not_built" => s == Status::NotBuilt,
        "pipeline" => matches!(s, Status::Queued | Status::Building),
        "queued" => s == Status::Queued,
        "building" => s == Status::Building,
        "failed" => s == Status::Failed,
        "reading" => reading,
        _ => true,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FacetCount {
    pub value: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Facets {
    pub authors: Vec<FacetCount>,
    pub genres: Vec<FacetCount>,
    pub eras: Vec<FacetCount>,
    pub statuses: Vec<FacetCount>,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    /// Indices into the book slice for the requested page.
    pub indices: Vec<usize>,
    pub total: usize,
    pub page: usize,
    pub pages: usize,
    pub per_page: usize,
    pub facets: Facets,
}

/// Per-book view used by the query; built by the caller from live state.
pub struct BookFacts {
    pub status: Status,
    pub reading: bool,
    pub built_at: u64,
}

pub fn run(books: &[Book], q: &Query, facts: impl Fn(&Book) -> BookFacts) -> QueryResult {
    let needle = q.search.trim().to_lowercase();
    let facts: Vec<BookFacts> = books.iter().map(&facts).collect();

    // Which filters each book passes: [search, author, genre, era, status]
    let passes: Vec<[bool; 5]> = books
        .iter()
        .zip(&facts)
        .map(|(b, f)| {
            [
                needle.is_empty() || matches_search(b, &needle),
                q.author.is_empty() || b.author == q.author,
                q.genre.is_empty() || b.categories.iter().any(|g| *g == q.genre),
                q.era.is_empty() || era_of(b) == q.era,
                status_matches(&q.status, f.status, f.reading),
            ]
        })
        .collect();
    let all_but = |p: &[bool; 5], skip: usize| p.iter().enumerate().all(|(i, ok)| i == skip || *ok);

    let mut authors = BTreeMap::<String, usize>::new();
    let mut genres = BTreeMap::<String, usize>::new();
    let mut eras = BTreeMap::<&str, usize>::new();
    let mut statuses = BTreeMap::<&str, usize>::new();
    for ((b, f), p) in books.iter().zip(&facts).zip(&passes) {
        if all_but(p, 1) {
            *authors.entry(b.author.clone()).or_default() += 1;
        }
        if all_but(p, 2) {
            for g in &b.categories {
                *genres.entry(g.clone()).or_default() += 1;
            }
        }
        if all_but(p, 3) {
            *eras.entry(era_of(b)).or_default() += 1;
        }
        if all_but(p, 4) {
            for s in STATUS_FILTERS {
                if status_matches(s, f.status, f.reading) {
                    *statuses.entry(s).or_default() += 1;
                }
            }
        }
    }
    let to_vec = |m: BTreeMap<String, usize>| m.into_iter().map(|(value, count)| FacetCount { value, count }).collect();
    let facets = Facets {
        authors: to_vec(authors),
        genres: to_vec(genres),
        eras: ERAS
            .iter()
            .filter_map(|e| eras.get(e).map(|&count| FacetCount { value: e.to_string(), count }))
            .collect(),
        statuses: STATUS_FILTERS
            .iter()
            .map(|s| FacetCount { value: s.to_string(), count: statuses.get(s).copied().unwrap_or(0) })
            .collect(),
    };

    let mut hits: Vec<usize> = (0..books.len()).filter(|&i| passes[i].iter().all(|ok| *ok)).collect();
    sort(&mut hits, books, &facts, &q.sort);

    let per_page = q.per_page.clamp(1, 500);
    let total = hits.len();
    let pages = total.div_ceil(per_page).max(1);
    let page = q.page.clamp(1, pages);
    let start = (page - 1) * per_page;
    let indices = hits.into_iter().skip(start).take(per_page).collect();

    QueryResult { indices, total, page, pages, per_page, facets }
}

fn matches_search(b: &Book, needle: &str) -> bool {
    let has = |s: &str| s.to_lowercase().contains(needle);
    has(&b.title)
        || has(&b.author)
        || has(&repo_name(b))
        || b.subjects.iter().any(|s| has(s))
        || b.categories.iter().any(|s| has(s))
}

/// Title without a leading article, for alphabetical order.
fn title_key(t: &str) -> String {
    let lower = t.to_lowercase();
    for article in ["the ", "a ", "an "] {
        if let Some(rest) = lower.strip_prefix(article) {
            return rest.to_string();
        }
    }
    lower
}

fn sort(hits: &mut [usize], books: &[Book], facts: &[BookFacts], key: &str) {
    let by_title = |a: &usize, b: &usize| title_key(&books[*a].title).cmp(&title_key(&books[*b].title));
    match key {
        "author" => hits.sort_by(|a, b| {
            books[*a].author.to_lowercase().cmp(&books[*b].author.to_lowercase()).then_with(|| by_title(a, b))
        }),
        "year" => hits.sort_by(|a, b| {
            let ya = books[*a].published.as_ref().map(|p| p.year);
            let yb = books[*b].published.as_ref().map(|p| p.year);
            match (ya, yb) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => Ordering::Less,
                (None, Some(_)) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            }
            .then_with(|| by_title(a, b))
        }),
        "words" => hits.sort_by(|a, b| books[*b].word_count.cmp(&books[*a].word_count).then_with(|| by_title(a, b))),
        "recent_build" => hits.sort_by(|a, b| facts[*b].built_at.cmp(&facts[*a].built_at).then_with(|| by_title(a, b))),
        "newest" => hits.sort_by(|a, b| books[*b].se_published.cmp(&books[*a].se_published).then_with(|| by_title(a, b))),
        _ => hits.sort_by(by_title),
    }
}

/// Repos that are currently being read (have progress, not finished).
pub fn reading_set<'a>(progress: impl Iterator<Item = (&'a String, f64)>) -> HashSet<String> {
    progress.filter(|(_, f)| *f > 0.0 && *f < 0.995).map(|(r, _)| r.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::books::Published;

    fn b(repo: &str, title: &str, author: &str, genre: &str, year: Option<i32>, words: i64) -> Book {
        Book {
            title: title.into(),
            author: author.into(),
            repo_name: repo.into(),
            categories: if genre.is_empty() { vec![] } else { vec![genre.into()] },
            published: year.map(|y| Published { year: y, label: y.to_string() }),
            word_count: words,
            ..Default::default()
        }
    }

    fn library() -> Vec<Book> {
        vec![
            b("doyle_lost-world", "The Lost World", "Arthur Conan Doyle", "Adventure", Some(1912), 76000),
            b("stoker_dracula", "Dracula", "Bram Stoker", "Horror", Some(1897), 160000),
            b("akutagawa_short-fiction", "Short Fiction", "Akutagawa Ryūnosuke", "Fiction", Some(1915), 50000),
            b("obrien_x", "At Swim-Two-Birds", "Flann O'Brien", "Fiction", None, 70000),
            b("doyle_hound", "The Hound of the Baskervilles", "Arthur Conan Doyle", "Mystery", Some(1902), 59000),
        ]
    }

    fn facts<'a>(built: &'a [&'a str]) -> impl Fn(&Book) -> BookFacts + 'a {
        move |bk: &Book| BookFacts {
            status: if built.contains(&bk.repo_name.as_str()) { Status::Built } else { Status::NotBuilt },
            reading: bk.repo_name == "stoker_dracula",
            built_at: if bk.repo_name == "doyle_hound" { 10 } else { 0 },
        }
    }

    fn titles(books: &[Book], r: &QueryResult) -> Vec<String> {
        r.indices.iter().map(|&i| books[i].title.clone()).collect()
    }

    #[test]
    fn title_sort_ignores_articles() {
        let lib = library();
        let r = run(&lib, &Query::default(), facts(&[]));
        assert_eq!(
            titles(&lib, &r),
            ["At Swim-Two-Birds", "Dracula", "The Hound of the Baskervilles", "The Lost World", "Short Fiction"]
        );
    }

    #[test]
    fn search_is_case_insensitive_and_handles_unicode_and_quotes() {
        let lib = library();
        for (needle, want) in [("RYŪNOSUKE", "Short Fiction"), ("o'brien", "At Swim-Two-Birds"), ("horror", "Dracula")] {
            let q = Query { search: needle.into(), ..Default::default() };
            assert_eq!(titles(&lib, &run(&lib, &q, facts(&[]))), [want], "{needle}");
        }
        let q = Query { search: "(.*[".into(), ..Default::default() };
        assert_eq!(run(&lib, &q, facts(&[])).total, 0);
    }

    #[test]
    fn facets_count_with_other_filters_applied() {
        let lib = library();
        let q = Query { author: "Arthur Conan Doyle".into(), ..Default::default() };
        let r = run(&lib, &q, facts(&["doyle_hound"]));
        assert_eq!(r.total, 2);
        // Author facet ignores the author filter itself
        assert_eq!(r.facets.authors.len(), 4);
        // Genre facet respects the author filter
        let genres: Vec<_> = r.facets.genres.iter().map(|f| f.value.as_str()).collect();
        assert_eq!(genres, ["Adventure", "Mystery"]);
        let count = |v: &str| r.facets.statuses.iter().find(|f| f.value == v).unwrap().count;
        assert_eq!((count("all"), count("built"), count("not_built")), (2, 1, 1));
        // Eras come in display order with Undated last
        let all = run(&lib, &Query::default(), facts(&[]));
        let eras: Vec<_> = all.facets.eras.iter().map(|f| f.value.as_str()).collect();
        assert_eq!(eras, ["1850–1899", "1900–1929", "Undated"]);
    }

    #[test]
    fn status_era_and_reading_filters() {
        let lib = library();
        let built = Query { status: "built".into(), ..Default::default() };
        assert_eq!(run(&lib, &built, facts(&["stoker_dracula"])).total, 1);
        let reading = Query { status: "reading".into(), ..Default::default() };
        assert_eq!(titles(&lib, &run(&lib, &reading, facts(&[]))), ["Dracula"]);
        let era = Query { era: "Undated".into(), ..Default::default() };
        assert_eq!(titles(&lib, &run(&lib, &era, facts(&[]))), ["At Swim-Two-Birds"]);
    }

    #[test]
    fn sorts_by_year_words_and_recent_build() {
        let lib = library();
        let q = |s: &str| Query { sort: s.into(), ..Default::default() };
        assert_eq!(titles(&lib, &run(&lib, &q("year"), facts(&[])))[0], "Dracula");
        assert_eq!(titles(&lib, &run(&lib, &q("year"), facts(&[])))[4], "At Swim-Two-Birds");
        assert_eq!(titles(&lib, &run(&lib, &q("words"), facts(&[])))[0], "Dracula");
        assert_eq!(titles(&lib, &run(&lib, &q("recent_build"), facts(&[])))[0], "The Hound of the Baskervilles");
    }

    #[test]
    fn pagination_clamps_out_of_range_pages() {
        let lib = library();
        let q = Query { per_page: 2, page: 99, ..Default::default() };
        let r = run(&lib, &q, facts(&[]));
        assert_eq!((r.page, r.pages, r.total, r.indices.len()), (3, 3, 5, 1));

        let empty = Query { search: "zzz".into(), page: 4, ..Default::default() };
        let r = run(&lib, &empty, facts(&[]));
        assert_eq!((r.page, r.pages, r.total), (1, 1, 0));
    }
}
