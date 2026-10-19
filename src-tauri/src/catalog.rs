// Michael Burgus — https://github.com/NeuralDrifter
// GitHub catalog sync — fetches all ebook repos from standardebooks org

use crate::books::{BookStore, Book};
use serde::Deserialize;

const ORG: &str = "standardebooks";
const EBOOK_MARKER: &str = "Standard Ebooks edition";

#[derive(Deserialize)]
struct GhRepo {
    name: String,
    #[serde(default)]
    description: Option<String>,
}

/// Fetch all ebook repos from the GitHub org, paginated. Also returns the
/// remaining API rate limit reported by GitHub.
async fn fetch_catalog(
    client: &reqwest::Client,
    token: &str,
    progress: &impl Fn(String),
) -> Result<(Vec<Book>, Option<u64>), String> {
    progress("Fetching repo list from GitHub...".into());

    let mut all_repos = Vec::new();
    let mut page = 1u32;
    let mut rate_remaining = None;

    loop {
        let url = format!(
            "https://api.github.com/orgs/{ORG}/repos?per_page=100&page={page}"
        );

        let mut req = client
            .get(&url)
            .header("Accept", "application/vnd.github.v3+json");
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| format!("GitHub API error: {e}"))?;

        let remaining = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        rate_remaining = remaining.or(rate_remaining);

        if !resp.status().is_success() {
            let hint = if remaining == Some(0) {
                " — rate limit reached; add a GitHub token in Settings or try again later"
            } else {
                ""
            };
            return Err(format!("GitHub API HTTP {}{hint}", resp.status()));
        }

        let batch: Vec<GhRepo> = resp
            .json()
            .await
            .map_err(|e| format!("JSON parse error: {e}"))?;

        if batch.is_empty() {
            break;
        }

        all_repos.extend(batch);
        progress(format!("Fetched {} repos...", all_repos.len()));
        page += 1;
    }

    let mut books = Vec::new();
    for repo in &all_repos {
        let desc = repo.description.as_deref().unwrap_or("");
        if !desc.contains(EBOOK_MARKER) {
            continue;
        }

        let (mut title, mut author) = parse_description(desc);

        // Fallback to slug parsing
        let parts: Vec<&str> = repo.name.splitn(2, '_').collect();
        if author.is_empty() && !parts.is_empty() {
            author = slug_to_title(parts[0]);
        }
        if title.is_empty() && parts.len() >= 2 {
            let title_slug = parts[1].split('_').next().unwrap_or("");
            title = slug_to_title(title_slug);
        }
        if title.is_empty() {
            title = repo.name.clone();
        }

        let path = repo.name.replace('_', "/");
        let book_url = format!("https://standardebooks.org/ebooks/{path}");

        books.push(Book {
            title,
            author,
            url: book_url,
            repo: format!("https://github.com/{ORG}/{}", repo.name),
            repo_name: repo.name.clone(),
            ..Default::default()
        });
    }

    progress(format!("Found {} books on GitHub.", books.len()));
    Ok((books, rate_remaining))
}

/// Merge fresh catalog with existing books, preserving enriched metadata.
pub async fn sync_catalog(
    store: &BookStore,
    client: &reqwest::Client,
    token: &str,
    progress: impl Fn(String),
) -> Result<serde_json::Value, String> {
    let (fresh, rate_remaining) = fetch_catalog(client, token, &progress).await?;
    let (new_count, removed_count, total) = store.merge_catalog(fresh);
    store.save()?;
    store.refresh_download_status();

    progress(format!(
        "Sync complete: {new_count} new, {removed_count} removed, {total} total"
    ));

    Ok(serde_json::json!({
        "new": new_count,
        "removed": removed_count,
        "total": total,
        "rate_remaining": rate_remaining,
    }))
}

fn parse_description(desc: &str) -> (String, String) {
    let idx = match desc.find("edition of ") {
        Some(i) => i,
        None => return (String::new(), String::new()),
    };
    let after = &desc[idx + "edition of ".len()..];

    if let Some(by_idx) = after.find(", by ") {
        let title = after[..by_idx].to_string();
        let rest = &after[by_idx + ", by ".len()..];

        for sep in &[". Translated by", ". Illustrated by", ". Edited by"] {
            if let Some(sep_idx) = rest.find(sep) {
                let author = rest[..sep_idx].to_string();
                return (title.trim().to_string(), author.trim().to_string());
            }
        }
        let author = rest.trim_end_matches('.').to_string();
        (title.trim().to_string(), author.trim().to_string())
    } else {
        let title = after.trim_end_matches('.').to_string();
        (title.trim().to_string(), String::new())
    }
}

fn slug_to_title(slug: &str) -> String {
    slug.split('-')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
