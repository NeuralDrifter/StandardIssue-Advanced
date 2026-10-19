// Michael Burgus — https://github.com/NeuralDrifter
// Word lookups for the reader from Wiktionary's REST API, cached in dictionary.json

use crate::fsutil::{load_json, save_json};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

const API: &str = "https://en.wiktionary.org/api/rest_v1/page/definition/";
const SENSES_PER_PART: usize = 3;
const MAX_WORD_CHARS: usize = 40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sense {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Part {
    /// "Noun", "Verb", …
    pub part: String,
    pub senses: Vec<Sense>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub word: String,
    pub parts: Vec<Part>,
    /// For an inflected form ("walked"), the word it comes from ("walk").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

// --- Wiktionary response ---

#[derive(Deserialize)]
struct RawPart {
    #[serde(rename = "partOfSpeech", default)]
    part: String,
    #[serde(default)]
    definitions: Vec<RawDefinition>,
}

#[derive(Deserialize)]
struct RawDefinition {
    #[serde(default)]
    definition: String,
    #[serde(default)]
    examples: Vec<String>,
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)<[^>]*>").unwrap())
}

fn form_of_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?s)form-of-definition-link.*?<a [^>]*title="([^"]+)""#).unwrap()
    })
}

/// Plain text from a Wiktionary HTML fragment.
pub fn strip_html(html: &str) -> String {
    let text = tag_re().replace_all(html, "");
    let text = decode_entities(&text);
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// English entry from a Wiktionary definition response, or None when it has no English senses.
pub fn parse_response(word: &str, json: &str) -> Result<Option<Entry>, String> {
    let all: BTreeMap<String, Vec<RawPart>> =
        serde_json::from_str(json).map_err(|e| format!("dictionary response: {e}"))?;
    let Some(english) = all.get("en") else {
        return Ok(None);
    };
    let mut root = None;
    let mut parts = Vec::new();
    for raw in english {
        let senses: Vec<Sense> = raw
            .definitions
            .iter()
            .filter_map(|d| {
                if root.is_none() {
                    root = form_of_re().captures(&d.definition).map(|c| decode_entities(&c[1]));
                }
                let text = strip_html(&d.definition);
                (!text.is_empty()).then(|| Sense {
                    text,
                    example: d.examples.iter().map(|e| strip_html(e)).find(|e| !e.is_empty()),
                })
            })
            .take(SENSES_PER_PART)
            .collect();
        if !senses.is_empty() {
            parts.push(Part { part: raw.part.clone(), senses });
        }
    }
    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(Entry { word: word.to_string(), parts, root: root.filter(|r| r != word) }))
}

/// The word as it should be looked up: outer punctuation trimmed, curly apostrophes made straight.
/// None when it is not a single word.
pub fn normalize(raw: &str) -> Option<String> {
    let word: String = raw
        .trim_matches(|c: char| !c.is_alphanumeric())
        .chars()
        .map(|c| if c == '\u{2019}' || c == '\u{2018}' { '\'' } else { c })
        .collect();
    let ok = !word.is_empty()
        && word.chars().count() <= MAX_WORD_CHARS
        && word.chars().all(|c| c.is_alphanumeric() || c == '\'' || c == '-');
    ok.then_some(word)
}

// --- Cache and lookup ---

/// Looked-up words; a word Wiktionary does not have is stored as None so it is not asked again.
pub struct Dictionary {
    path: PathBuf,
    cache: Mutex<BTreeMap<String, Option<Entry>>>,
}

#[derive(Debug, PartialEq)]
pub enum Lookup {
    Found(Entry),
    NotFound,
}

impl Dictionary {
    pub fn open(path: PathBuf) -> (Self, Option<String>) {
        let (cache, warning) = load_json(&path);
        (Self { path, cache: Mutex::new(cache) }, warning)
    }

    pub fn cached(&self, key: &str) -> Option<Option<Entry>> {
        self.cache.lock().unwrap().get(key).cloned()
    }

    pub fn store(&self, key: &str, entry: Option<Entry>) -> Result<(), String> {
        let mut cache = self.cache.lock().unwrap();
        cache.insert(key.to_string(), entry);
        save_json(&self.path, &*cache)
    }

    /// Look a word up, from the cache when it has been seen before. Tries the word as written,
    /// then in lower case (Wiktionary titles are case-sensitive; sentence-initial words are capitalised).
    pub async fn lookup(&self, client: &reqwest::Client, raw: &str) -> Result<Lookup, String> {
        let word = normalize(raw).ok_or("not a single word")?;
        let key = word.to_lowercase();
        if let Some(hit) = self.cached(&key) {
            return Ok(hit.map_or(Lookup::NotFound, Lookup::Found));
        }
        let mut tries = vec![word.clone()];
        if key != word {
            tries.push(key.clone());
        }
        let mut found = None;
        for w in tries {
            if let Some(entry) = fetch(client, &w).await? {
                found = Some(entry);
                break;
            }
        }
        self.store(&key, found.clone())?;
        Ok(found.map_or(Lookup::NotFound, Lookup::Found))
    }
}

/// One Wiktionary request. Ok(None) = no such English entry; Err = could not ask (offline, server error).
async fn fetch(client: &reqwest::Client, word: &str) -> Result<Option<Entry>, String> {
    let mut url = reqwest::Url::parse(API).map_err(|e| e.to_string())?;
    url.path_segments_mut().map_err(|_| "bad dictionary url")?.pop_if_empty().push(word);
    let res = client.get(url).send().await.map_err(|e| format!("dictionary unavailable: {e}"))?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !res.status().is_success() {
        return Err(format!("dictionary unavailable: HTTP {}", res.status()));
    }
    let body = res.text().await.map_err(|e| format!("dictionary unavailable: {e}"))?;
    parse_response(word, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MELANCHOLY: &str = include_str!("../tests/fixtures/wiktionary_melancholy.json");
    const WALKED: &str = include_str!("../tests/fixtures/wiktionary_walked.json");

    #[test]
    fn parses_english_senses_as_plain_text() {
        let e = parse_response("melancholy", MELANCHOLY).unwrap().unwrap();
        assert_eq!(e.word, "melancholy");
        assert_eq!(e.root, None);
        let parts: Vec<_> = e.parts.iter().map(|p| p.part.as_str()).collect();
        assert_eq!(parts, ["Noun", "Adjective"]);
        assert_eq!(
            e.parts[0].senses[1].text,
            "Great sadness or depression, especially of a thoughtful or introspective nature."
        );
        assert!(e.parts[0].senses[0].text.starts_with("Black bile, formerly"));
        assert!(e.parts.iter().flat_map(|p| &p.senses).all(|s| !s.text.contains('<')));
        assert!(e.parts[1].senses.iter().any(|s| s.example.is_some()));
    }

    #[test]
    fn inflected_form_points_at_its_root() {
        let e = parse_response("walked", WALKED).unwrap().unwrap();
        assert_eq!(e.root.as_deref(), Some("walk"));
        assert_eq!(e.parts[0].part, "Verb");
        assert_eq!(e.parts[0].senses[0].text, "simple past and past participle of walk");
    }

    #[test]
    fn no_english_entry_is_none() {
        assert_eq!(parse_response("x", r#"{"fr":[{"partOfSpeech":"Noun","definitions":[{"definition":"y"}]}]}"#).unwrap(), None);
        assert_eq!(parse_response("x", r#"{"en":[{"partOfSpeech":"Noun","definitions":[{"definition":"<span></span>"}]}]}"#).unwrap(), None);
        assert!(parse_response("x", "not json").is_err());
    }

    #[test]
    fn strips_tags_and_entities() {
        assert_eq!(strip_html("<i>a</i> &amp; b&#160;c &#x2014; &lt;d&gt; &bogus; & e"), "a & b c — <d> &bogus; & e");
    }

    #[test]
    fn normalizes_words() {
        assert_eq!(normalize("“Fortitude,”").as_deref(), Some("Fortitude"));
        assert_eq!(normalize("don’t").as_deref(), Some("don't"));
        assert_eq!(normalize("well-known.").as_deref(), Some("well-known"));
        assert_eq!(normalize("two words"), None);
        assert_eq!(normalize("…"), None);
    }

    #[test]
    fn cache_keeps_hits_and_misses() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("dictionary.json");
        let (d, w) = Dictionary::open(path.clone());
        assert!(w.is_none());
        assert_eq!(d.cached("melancholy"), None);
        let entry = parse_response("melancholy", MELANCHOLY).unwrap();
        d.store("melancholy", entry.clone()).unwrap();
        d.store("qzxv", None).unwrap();

        let (d2, _) = Dictionary::open(path);
        assert_eq!(d2.cached("melancholy"), Some(entry));
        assert_eq!(d2.cached("qzxv"), Some(None));
    }

    #[test]
    fn cached_words_need_no_network() {
        let t = tempfile::tempdir().unwrap();
        let (d, _) = Dictionary::open(t.path().join("dictionary.json"));
        d.store("qzxv", None).unwrap();
        // An unroutable client: a cache miss would fail
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_millis(1)).build().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert_eq!(rt.block_on(d.lookup(&client, "Qzxv.")).unwrap(), Lookup::NotFound);
        assert!(rt.block_on(d.lookup(&client, "two words")).is_err());
    }
}
