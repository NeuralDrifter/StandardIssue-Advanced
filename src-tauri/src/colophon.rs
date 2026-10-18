// Michael Burgus — https://github.com/NeuralDrifter
// Colophon parsing: original publication date and cover artwork credit

use crate::books::{CoverArt, Published};
use regex::Regex;
use roxmltree::{Document, Node, ParsingOptions};
use std::sync::OnceLock;

/// Bump when the parser learns something new, so books get re-enriched.
pub const COLOPHON_REV: u32 = 1;

const ORG: &str = "standardebooks";
const EPUB_NS: &str = "http://www.idpf.org/2007/ops";

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ColophonInfo {
    pub published: Option<Published>,
    pub art: Option<CoverArt>,
}

/// Fetch a book's colophon from GitHub. `Ok(None)` when the repo has no colophon.
pub async fn fetch_colophon(client: &reqwest::Client, repo: &str) -> Result<Option<String>, String> {
    let url = format!("https://raw.githubusercontent.com/{ORG}/{repo}/master/src/epub/text/colophon.xhtml");
    let resp = client.get(&url).send().await.map_err(|e| format!("{e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("colophon HTTP {}", resp.status()));
    }
    resp.text().await.map(Some).map_err(|e| format!("{e}"))
}

pub fn parse_colophon(xhtml: &str) -> Result<ColophonInfo, String> {
    let opts = ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = Document::parse_with_options(xhtml, opts).map_err(|e| format!("colophon XML: {e}"))?;

    let published = find_typed(&doc, "se:name.publication").and_then(|title| {
        let p = title.ancestors().find(|n| n.has_tag_name("p"))?;
        date_before_by(&text_after(p, title))
    });

    let art = find_typed(&doc, "se:name.visual-art").map(|title| {
        let p = title.ancestors().find(|n| n.has_tag_name("p")).unwrap_or(title);
        let after: Vec<Node> = p
            .descendants()
            .filter(|n| n.id().get_usize() > title.id().get_usize() && !n.ancestors().any(|a| a == title))
            .collect();
        let year = after
            .iter()
            .find(|n| n.has_tag_name("time"))
            .and_then(|t| text_of(*t).parse().ok());
        let link = after.iter().find(|n| n.has_tag_name("a"));
        CoverArt {
            title: text_of(title),
            year,
            artist: link.map(|a| text_of(*a)).unwrap_or_default(),
            artist_url: link.and_then(|a| a.attribute("href")).unwrap_or_default().to_string(),
        }
    });

    Ok(ColophonInfo { published, art })
}

/// First element whose epub:type contains `kind` (and isn't a typeface credit).
fn find_typed<'a>(doc: &'a Document, kind: &str) -> Option<Node<'a, 'a>> {
    doc.descendants().find(|n| {
        n.attribute((EPUB_NS, "type"))
            .is_some_and(|t| t.split_whitespace().any(|v| v.starts_with(kind) && !v.contains("typeface")))
    })
}

fn text_of(n: Node) -> String {
    normalize(&n.descendants().filter_map(|d| d.text().filter(|_| d.is_text())).collect::<String>())
}

/// Text inside `p` that comes after `title`, up to the translation sentence.
fn text_after(p: Node, title: Node) -> String {
    let text = p
        .descendants()
        .filter(|n| n.is_text() && n.id().get_usize() > title.id().get_usize() && !n.ancestors().any(|a| a == title))
        .filter_map(|n| n.text())
        .collect::<Vec<_>>()
        // Separate nodes so "<br/>by<br/><a>Aesop</a>" can't fuse into "byAesop"
        .join(" ");
    let text = normalize(&text);
    // The translation sentence ("It was translated … in 1916 by") never holds the original date
    static TRANSLATED: OnceLock<Regex> = OnceLock::new();
    let translated = TRANSLATED.get_or_init(|| Regex::new(r"\.\s+(It|They)\s").unwrap());
    let text = match translated.find(&text) {
        Some(m) => &text[..m.start()],
        None => &text,
    };
    text.to_string()
}

/// Date in the text before an author's "by" (or anywhere, when there is none). Tries each "by" in turn, since
/// some colophons say "compiled by stories published between 1823 and 1959 by".
fn date_before_by(text: &str) -> Option<Published> {
    static BY: OnceLock<Regex> = OnceLock::new();
    let by = BY.get_or_init(|| Regex::new(r"\bby\b").unwrap());
    by.find_iter(text)
        .find_map(|m| parse_date_phrase(&text[..m.start()]))
        // Anonymous works have no "by": "was written circa 1250."
        .or_else(|| parse_date_phrase(text))
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

const ORDINALS: [&str; 21] = [
    "", "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth", "tenth",
    "eleventh", "twelfth", "thirteenth", "fourteenth", "fifteenth", "sixteenth", "seventeenth",
    "eighteenth", "nineteenth", "twentieth",
];

/// "was published in 1912", "between 1855 and 1863", "in the 11th century CE",
/// "in the early third century BC", "circa 458 BC".
pub fn parse_date_phrase(s: &str) -> Option<Published> {
    static RES: OnceLock<[Regex; 3]> = OnceLock::new();
    let [between, century, year] = RES.get_or_init(|| {
        [
            Regex::new(r"between (\d{1,4})(?:\s*(BCE|BC))? and (\d{1,4})(?:\s*(BCE|BC|CE|AD))?").unwrap(),
            Regex::new(r"(?i)(?:(early|mid|late)[- ])?(\d{1,2}(?:st|nd|rd|th)|[a-z]+) century(?:\s+(BCE|BC|CE|AD))?").unwrap(),
            Regex::new(r"\b(\d{1,4})\b(?:\s*(BCE|BC|CE|AD))?").unwrap(),
        ]
    });
    let bce = |m: Option<regex::Match>| m.is_some_and(|m| m.as_str().starts_with('B'));

    if let Some(c) = between.captures(s) {
        let a: i32 = c[1].parse().ok()?;
        let b: i32 = c[3].parse().ok()?;
        let era_a = bce(c.get(2)) || (c.get(2).is_none() && bce(c.get(4)));
        let start = if era_a { -a } else { a };
        let suffix = c.get(4).map(|m| format!(" {}", m.as_str())).unwrap_or_default();
        return Some(Published { year: start, label: format!("{a}–{b}{suffix}") });
    }
    if let Some(c) = century.captures(s) {
        let word = c[2].to_lowercase();
        let n = word
            .trim_end_matches(|ch: char| ch.is_ascii_alphabetic())
            .parse::<i32>()
            .ok()
            .or_else(|| ORDINALS.iter().position(|o| *o == word).map(|i| i as i32))
            .filter(|n| *n > 0)?;
        let offset = match c.get(1).map(|m| m.as_str().to_lowercase()).as_deref() {
            Some("mid") => 33,
            Some("late") => 66,
            _ => 0,
        };
        let start = if bce(c.get(3)) { -(n * 100) + offset } else { (n - 1) * 100 + offset };
        return Some(Published { year: start, label: c[0].to_string() });
    }
    let c = year.captures(s)?;
    let y: i32 = c[1].parse().ok()?;
    Some(Published { year: if bce(c.get(2)) { -y } else { y }, label: c[0].to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><section>{body}</section></body></html>"#
        )
    }

    const LOST_WORLD: &str = r#"
<p><i epub:type="se:name.publication.book">The Lost World</i><br/>
was published in <time>1912</time> by<br/>
<a href="https://en.wikipedia.org/wiki/Arthur_Conan_Doyle">Arthur Conan Doyle</a>.</p>
<p>This ebook was produced for<br/><a href="https://standardebooks.org/">Standard Ebooks</a><br/>
and is based on a transcription produced in <time>2008</time> by<br/><b>Judith Boss</b>.</p>
<p>The cover page is adapted from<br/>
<i epub:type="se:name.visual-art.painting">Brontosaurus</i>,<br/>
a painting completed in <time>1897</time> by<br/>
<a href="https://en.wikipedia.org/wiki/Charles_R._Knight">Charles <abbr epub:type="z3998:given-name">R.</abbr> Knight</a>.<br/>
The cover and title pages feature the<br/>
<b epub:type="se:name.visual-art.typeface">League Spartan</b> and <b epub:type="se:name.visual-art.typeface">Sorts Mill Goudy</b><br/>
typefaces created in <time>2014</time> and <time>2009</time> by<br/>
<a href="https://www.theleagueofmoveabletype.com/">The League of Moveable Type</a>.</p>"#;

    #[test]
    fn single_year_and_painting_credit() {
        let info = parse_colophon(&wrap(LOST_WORLD)).unwrap();
        assert_eq!(info.published, Some(Published { year: 1912, label: "1912".into() }));
        let art = info.art.unwrap();
        assert_eq!(art.title, "Brontosaurus");
        assert_eq!(art.year, Some(1897));
        assert_eq!(art.artist, "Charles R. Knight");
        assert_eq!(art.artist_url, "https://en.wikipedia.org/wiki/Charles_R._Knight");
    }

    #[test]
    fn range_compiled_and_translation_year_ignored() {
        let range = wrap(r#"<p><i epub:type="se:name.publication.book">Russian Folktales</i><br/>
was published between <time>1855</time> and <time>1863</time> by<br/><b>A. N. Afanasyev</b>.<br/>
It was translated from Russian in <time>1916</time> by<br/><b>Leonard Magnus</b>.</p>"#);
        assert_eq!(
            parse_colophon(&range).unwrap().published,
            Some(Published { year: 1855, label: "1855–1863".into() })
        );

        let compiled = wrap(r#"<p><i epub:type="se:name.publication.book">Short Fiction</i><br/>
was compiled from short stories published in between <time>1915</time> and <time>1920</time> by<br/>
<a href="x">Akutagawa Ryūnosuke</a>.</p>"#);
        assert_eq!(parse_colophon(&compiled).unwrap().published.unwrap().year, 1915);
    }

    #[test]
    fn century_and_bc_dates() {
        let century = wrap(r#"<p><i epub:type="se:name.publication.book">The Luzumiyat</i><br/>
was published in the 11th century <abbr epub:type="se:era z3998:initialism">CE</abbr> by<br/><a href="x">al-Maʻarri</a>.</p>"#);
        assert_eq!(
            parse_colophon(&century).unwrap().published,
            Some(Published { year: 1000, label: "11th century CE".into() })
        );

        let bc = wrap(r#"<p><i epub:type="se:name.publication.play">Agamemnon</i><br/>
was completed circa 458 <abbr epub:type="se:era z3998:initialism">BC</abbr> by<br/><a href="x">Aeschylus</a>.<br/>
It was translated from Ancient Greek in <time>1920</time> by<br/><a href="y">Gilbert Murray</a>.</p>"#);
        assert_eq!(
            parse_colophon(&bc).unwrap().published,
            Some(Published { year: -458, label: "458 BC".into() })
        );
        assert_eq!(parse_date_phrase("in the 5th century BCE").unwrap().year, -500);
    }

    #[test]
    fn spelled_centuries_and_early_by() {
        let p = |s: &str| parse_date_phrase(s).map(|d| (d.year, d.label));
        assert_eq!(p("was completed in the early third century BC"), Some((-300, "early third century BC".into())));
        assert_eq!(p("was written in the late fifth century BC"), Some((-434, "late fifth century BC".into())));
        assert_eq!(p("was written in the third century AD"), Some((200, "third century AD".into())));
        assert_eq!(p("was written circa the sixth century BC"), Some((-600, "sixth century BC".into())));

        let shelley = wrap(r#"<p><i epub:type="se:name.publication.book">Short Fiction</i><br/>
is compiled by stories published between <time>1823</time> and <time>1959</time> by<br/>
<a href="x">Mary Shelley</a>.</p>"#);
        assert_eq!(parse_colophon(&shelley).unwrap().published.unwrap().label, "1823–1959");
    }

    #[test]
    fn anonymous_works_without_by() {
        let gudrun = wrap(r#"<p><i epub:type="se:name.publication.poem">Gudrun</i><br/>
was written circa 1250.<br/>
It was translated from Middle High German in <time>1889</time> by<br/><a href="x">Mary Pickering Nichols</a>.</p>"#);
        assert_eq!(parse_colophon(&gudrun).unwrap().published.unwrap().year, 1250);

        let ethics = wrap(r#"<p>The <i epub:type="se:name.publication.book">Nicomachean Ethics</i><br/>
is based on the lectures of<br/><a href="x">Aristotle</a><br/>
and was compiled circa 340 <abbr epub:type="se:era z3998:initialism">BC</abbr>.<br/>
It was translated from Greek in <time>1906</time> by<br/><a href="y">F. H. Peters</a>.</p>"#);
        assert_eq!(parse_colophon(&ethics).unwrap().published.unwrap().year, -340);
    }

    #[test]
    fn missing_date_and_art_give_none() {
        let none = wrap(r#"<p><i epub:type="se:name.publication.book">Fables</i><br/>
by<br/><a href="x">Aesop</a>.<br/>It was translated from Greek in <time>1912</time> by<br/><b>V. S. Vernon Jones</b>.</p>"#);
        let info = parse_colophon(&none).unwrap();
        assert_eq!(info, ColophonInfo::default());
    }

    #[test]
    fn painting_without_link_keeps_title() {
        let x = wrap(r#"<p>The cover page is adapted from<br/><i epub:type="se:name.visual-art.painting">Untitled</i>,<br/>
a painting completed in <time>1880</time> by an unknown artist.</p>"#);
        let art = parse_colophon(&x).unwrap().art.unwrap();
        assert_eq!((art.title.as_str(), art.year, art.artist.as_str()), ("Untitled", Some(1880), ""));
    }

    #[test]
    fn malformed_xml_is_an_error() {
        assert!(parse_colophon("<html><p>").is_err());
    }
}

/// Run against a real library: `SI_EPUBS=<epubs dir> cargo test --lib scan_library -- --ignored --nocapture`
#[cfg(test)]
mod scan {
    #[test]
    #[ignore]
    fn scan_library() {
        let Ok(dir) = std::env::var("SI_EPUBS") else { return };
        let (mut total, mut dated, mut art, mut errors) = (0, 0, 0, 0);
        let mut undated = Vec::new();
        let mut eras = std::collections::BTreeMap::new();
        for e in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
            if e.path().extension().is_none_or(|x| x != "epub") {
                continue;
            }
            total += 1;
            let mut z = zip::ZipArchive::new(std::fs::File::open(e.path()).unwrap()).unwrap();
            let mut xhtml = String::new();
            let Ok(mut f) = z.by_name("epub/text/colophon.xhtml") else { errors += 1; continue };
            std::io::Read::read_to_string(&mut f, &mut xhtml).unwrap();
            match super::parse_colophon(&xhtml) {
                Ok(info) => {
                    match &info.published {
                        Some(d) => {
                            dated += 1;
                            *eras.entry(crate::books::era_of_year(d.year)).or_insert(0) += 1;
                            if d.year < 1500 || d.year > 1960 {
                                println!("check: {} -> {} ({})", e.file_name().to_string_lossy(), d.year, d.label);
                            }
                        }
                        None => undated.push(e.file_name().to_string_lossy().to_string()),
                    }
                    if info.art.as_ref().is_some_and(|a| !a.title.is_empty()) { art += 1 }
                }
                Err(err) => { errors += 1; println!("ERR {}: {err}", e.path().display()) }
            }
        }
        println!("total {total}, dated {dated}, art {art}, errors {errors}");
        println!("eras {eras:?}");
        for u in undated.iter().take(40) { println!("undated: {u}") }
    }
}
