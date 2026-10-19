// Michael Burgus — https://github.com/NeuralDrifter
// Kobo KEPUB from an epub: every sentence of the text wrapped in a koboSpan, body in Kobo's page wrappers

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

/// Elements whose text is left alone.
const SKIP: &[&str] = &["pre", "code", "svg", "math", "script", "style", "head"];
/// Elements that start a new Kobo paragraph.
const BLOCKS: &[&str] = &[
    "p", "h1", "h2", "h3", "h4", "h5", "h6", "li", "dt", "dd", "td", "th", "blockquote", "figcaption",
    "caption", "div", "section", "article", "aside", "header", "footer", "body", "hgroup", "nav", "table",
];
/// Words that end in a full stop without ending the sentence.
const ABBREVIATIONS: &[&str] = &[
    "Mr", "Mrs", "Ms", "Messrs", "Dr", "St", "Mt", "Jr", "Sr", "Rev", "Capt", "Col", "Gen", "Lt", "Sgt",
    "Prof", "Hon", "No", "vs", "viz", "cf", "etc", "i.e", "e.g", "Co", "Bros",
];

#[derive(Debug, Default, PartialEq)]
pub struct Report {
    /// Content documents rewritten.
    pub documents: usize,
    /// Content documents that could not be parsed and were copied unchanged.
    pub skipped: usize,
    pub spans: usize,
}

/// `Title_Author (Standard Ebooks).epub` → `Title_Author (Standard Ebooks).kepub.epub`
pub fn kepub_path(epub: &Path) -> PathBuf {
    let stem = epub.file_stem().unwrap_or_default().to_string_lossy();
    epub.with_file_name(format!("{stem}.kepub.epub"))
}

/// Split raw text (entities left as written) into sentences. Each piece keeps the
/// whitespace that follows it, so joining the pieces gives back the input.
pub fn split_sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (_, c) = chars[i];
        if matches!(c, '.' | '!' | '?' | '…') {
            // Closing quotes and brackets belong to the sentence they end
            let mut j = i + 1;
            while j < chars.len() && matches!(chars[j].1, '”' | '’' | '"' | '\'' | ')' | ']' | '»') {
                j += 1;
            }
            let space = j < chars.len() && matches!(chars[j].1, ' ' | '\n' | '\t' | '\r');
            if space && !(c == '.' && is_abbreviation(&text[start..chars[i].0])) {
                while j < chars.len() && matches!(chars[j].1, ' ' | '\n' | '\t' | '\r') {
                    j += 1;
                }
                let end = chars.get(j).map_or(text.len(), |&(b, _)| b);
                out.push(&text[start..end]);
                start = end;
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// True when the text before a full stop ends in an abbreviation or a single initial.
fn is_abbreviation(before: &str) -> bool {
    let word = before.rsplit(|c: char| c.is_whitespace() || c == '(' || c == '“' || c == '‘').next().unwrap_or("");
    let word = word.trim_start_matches(|c: char| !c.is_alphanumeric());
    let mut letters = word.chars();
    let initial = matches!((letters.next(), letters.next()), (Some(c), None) if c.is_uppercase());
    initial || ABBREVIATIONS.contains(&word)
}

/// Rewrite one XHTML document for Kobo. None when it cannot be parsed as XML.
pub fn convert_xhtml(src: &str) -> Option<(String, usize)> {
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(src, opts).ok()?;
    let body = doc.descendants().find(|n| n.has_tag_name("body"))?;
    if body.first_child().is_none() {
        return None;
    }

    // Kobo's page wrappers just inside <body>. Inserts are (byte position, text), kept in push
    // order at equal positions: the wrapper opens before any span, and closes after the last one.
    let open_end = start_tag_end(src, body.range().start)?;
    let close = src[..body.range().end].rfind("</body")?;
    let mut inserts: Vec<(usize, String)> =
        vec![(open_end, r#"<div id="book-columns"><div id="book-inner">"#.into())];
    let mut spans = 0;
    let mut para = 0usize;
    let mut last_block: Option<roxmltree::NodeId> = None;
    let mut seg = 0usize;

    for node in body.descendants().filter(|n| n.is_text()) {
        let raw = &src[node.range()];
        if raw.trim().is_empty() || raw.contains('<') {
            continue; // whitespace, or CDATA
        }
        let mut block = None;
        let mut skip = false;
        for a in node.ancestors().filter(|a| a.is_element()) {
            let name = a.tag_name().name();
            if SKIP.contains(&name) {
                skip = true;
                break;
            }
            if block.is_none() && BLOCKS.contains(&name) {
                block = Some(a.id());
            }
        }
        if skip {
            continue;
        }
        if block != last_block {
            last_block = block;
            para += 1;
            seg = 0;
        }
        let mut pos = node.range().start;
        for piece in split_sentences(raw) {
            seg += 1;
            spans += 1;
            inserts.push((pos, format!(r#"<span class="koboSpan" id="kobo.{para}.{seg}">"#)));
            inserts.push((pos + piece.len(), "</span>".into()));
            pos += piece.len();
        }
    }

    inserts.push((close, "</div></div>".into()));

    // Stable sort, so equal positions keep push order (a span closes before the next one opens)
    inserts.sort_by_key(|(p, _)| *p);
    let mut out = String::with_capacity(src.len() + inserts.iter().map(|(_, s)| s.len()).sum::<usize>());
    let mut at = 0;
    for (p, s) in &inserts {
        out.push_str(&src[at..*p]);
        out.push_str(s);
        at = *p;
    }
    out.push_str(&src[at..]);
    Some((out, spans))
}

/// Byte index just past the `>` of the start tag beginning at `from`.
fn start_tag_end(src: &str, from: usize) -> Option<usize> {
    let mut quote = None;
    for (i, c) in src[from..].char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '>') => return Some(from + i + 1),
            _ => {}
        }
    }
    None
}

fn is_content_doc(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm")
}

/// Write a KEPUB of `epub` to `out` (via a temporary file, so a failure leaves nothing behind).
pub fn convert(epub: &Path, out: &Path) -> Result<Report, String> {
    let file = std::fs::File::open(epub).map_err(|e| format!("open epub: {e}"))?;
    let mut zin = zip::ZipArchive::new(file).map_err(|e| format!("read epub: {e}"))?;
    let part = out.with_extension("part");
    let result = (|| {
        let mut zw = zip::ZipWriter::new(std::fs::File::create(&part).map_err(|e| format!("create kepub: {e}"))?);
        let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let deflated = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zw.start_file("mimetype", stored).map_err(|e| format!("zip mimetype: {e}"))?;
        zw.write_all(b"application/epub+zip").map_err(|e| format!("zip mimetype: {e}"))?;

        let mut report = Report::default();
        for i in 0..zin.len() {
            let mut entry = zin.by_index(i).map_err(|e| format!("read epub entry: {e}"))?;
            let name = entry.name().to_string();
            if name == "mimetype" || entry.is_dir() {
                continue;
            }
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(|e| format!("read {name}: {e}"))?;
            if is_content_doc(&name) {
                match std::str::from_utf8(&data).ok().and_then(convert_xhtml) {
                    Some((xhtml, spans)) => {
                        data = xhtml.into_bytes();
                        report.documents += 1;
                        report.spans += spans;
                    }
                    None => report.skipped += 1,
                }
            }
            zw.start_file(&name, deflated).map_err(|e| format!("zip {name}: {e}"))?;
            zw.write_all(&data).map_err(|e| format!("zip {name}: {e}"))?;
        }
        zw.finish().map_err(|e| format!("zip finish: {e}"))?;
        Ok(report)
    })();
    match result {
        Ok(report) => {
            std::fs::rename(&part, out).map_err(|e| format!("save kepub: {e}"))?;
            Ok(report)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_split_after_terminal_punctuation_only() {
        let t = "It was late. “Who is there?” he asked. Mr. Ablett said no… Then J. Smith came in.";
        assert_eq!(
            split_sentences(t),
            ["It was late. ", "“Who is there?” ", "he asked. ", "Mr. Ablett said no… ", "Then J. Smith came in."]
        );
        assert_eq!(split_sentences(t).concat(), t);
        assert_eq!(split_sentences("No stop here"), ["No stop here"]);
        assert_eq!(split_sentences("3.5 miles. Then"), ["3.5 miles. ", "Then"]);
        assert_eq!(split_sentences("Mrs.\u{a0}Stevens"), ["Mrs.\u{a0}Stevens"]);
    }

    const DOC: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
	<head><title>I. A Title</title></head>
	<body epub:type="bodymatter">
		<section id="c1">
			<p>One. Two &amp; three.</p>
			<p>Four <i>five.</i> Six.</p>
			<pre>Keep. This.</pre>
		</section>
	</body>
</html>"#;

    #[test]
    fn spans_wrap_sentences_and_body_is_wrapped() {
        let (out, spans) = convert_xhtml(DOC).unwrap();
        assert_eq!(spans, 5);
        assert!(out.contains(
            r#"<p><span class="koboSpan" id="kobo.1.1">One. </span><span class="koboSpan" id="kobo.1.2">Two &amp; three.</span></p>"#
        ));
        assert!(out.contains(
            r#"<p><span class="koboSpan" id="kobo.2.1">Four </span><i><span class="koboSpan" id="kobo.2.2">five.</span></i><span class="koboSpan" id="kobo.2.3"> Six.</span></p>"#
        ));
        assert!(out.contains("<pre>Keep. This.</pre>"));
        assert!(out.contains("<title>I. A Title</title>"));
        assert!(out.contains(r#"<body epub:type="bodymatter"><div id="book-columns"><div id="book-inner">"#));
        assert!(out.contains("</div></div></body>"));
        // Still well-formed, and the text reads the same
        let parsed = roxmltree::Document::parse(&out).unwrap();
        let text = |d: &roxmltree::Document| d.descendants().filter(|n| n.is_text()).map(|n| n.text().unwrap()).collect::<String>();
        assert_eq!(text(&parsed), text(&roxmltree::Document::parse(DOC).unwrap()));
    }

    #[test]
    fn unparseable_document_is_none() {
        assert_eq!(convert_xhtml("<html><body><p>broken</body></html>"), None);
    }

    #[test]
    fn convert_rewrites_text_and_copies_the_rest() {
        let t = tempfile::tempdir().unwrap();
        let epub = t.path().join("Book_Author (Standard Ebooks).epub");
        {
            let mut zw = zip::ZipWriter::new(std::fs::File::create(&epub).unwrap());
            let o = SimpleFileOptions::default();
            zw.start_file("mimetype", o).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("epub/text/c1.xhtml", o).unwrap();
            zw.write_all(DOC.as_bytes()).unwrap();
            zw.start_file("epub/text/bad.xhtml", o).unwrap();
            zw.write_all(b"<p>oops").unwrap();
            zw.start_file("epub/css/core.css", o).unwrap();
            zw.write_all(b"p { margin: 0 }").unwrap();
            zw.finish().unwrap();
        }
        let out = kepub_path(&epub);
        assert_eq!(out.file_name().unwrap(), "Book_Author (Standard Ebooks).kepub.epub");
        let report = convert(&epub, &out).unwrap();
        assert_eq!(report, Report { documents: 1, skipped: 1, spans: 5 });
        assert!(!out.with_extension("part").exists());

        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        {
            let first = z.by_index(0).unwrap();
            assert_eq!(first.name(), "mimetype");
            assert_eq!(first.compression(), zip::CompressionMethod::Stored);
        }
        let read = |z: &mut zip::ZipArchive<std::fs::File>, n: &str| {
            let mut s = String::new();
            z.by_name(n).unwrap().read_to_string(&mut s).unwrap();
            s
        };
        assert!(read(&mut z, "epub/text/c1.xhtml").contains("koboSpan"));
        assert_eq!(read(&mut z, "epub/text/bad.xhtml"), "<p>oops");
        assert_eq!(read(&mut z, "epub/css/core.css"), "p { margin: 0 }");
    }

    /// Converts every epub in the real library and checks each output still parses.
    /// Run with: cargo test --lib kepub::tests::real_library -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_library() {
        let dir = std::env::var("USERPROFILE").map(PathBuf::from).unwrap().join("Documents/StandardIssue Library/epubs");
        let t = tempfile::tempdir().unwrap();
        let mut books = 0;
        for e in walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
            let p = e.path();
            if !p.to_string_lossy().ends_with(").epub") || books >= 60 {
                continue;
            }
            books += 1;
            let out = t.path().join("x.kepub.epub");
            let r = convert(p, &out).unwrap();
            assert_eq!(r.skipped, 0, "{}", p.display());
            assert!(r.spans > 0);
            let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
            for i in 0..z.len() {
                let mut f = z.by_index(i).unwrap();
                if is_content_doc(f.name()) {
                    let mut s = String::new();
                    f.read_to_string(&mut s).unwrap();
                    let name = f.name().to_string();
                    roxmltree::Document::parse_with_options(&s, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
                        .unwrap_or_else(|e| panic!("{} {name}: {e}", p.display()));
                }
            }
        }
        println!("{books} books converted");
    }
}
