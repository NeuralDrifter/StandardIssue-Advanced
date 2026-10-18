// Michael Burgus — https://github.com/NeuralDrifter
// Cover image fetching from Standard Ebooks GitHub repos

use crate::books::{repo_name, BookStore};
use base64::Engine;
use image::imageops::FilterType;
use image::GenericImageView;
use regex::Regex;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::Semaphore;

fn jpeg_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"xlink:href="data:image/jpeg;base64,([^"]+)""#).unwrap())
}

const ORG: &str = "standardebooks";
const THUMB_W: u32 = 280;
const THUMB_H: u32 = 420;

pub struct CoverResult {
    pub fetched: usize,
    pub cached: usize,
    pub errors: usize,
}

pub async fn fetch_all_covers(
    store: &BookStore,
    client: &reqwest::Client,
    covers_dir: &Path,
    concurrency: usize,
    progress: impl Fn(usize, usize, String, String),
    cancel: Arc<AtomicBool>,
) -> CoverResult {
    std::fs::create_dir_all(covers_dir).ok();

    // Filter to books needing covers
    struct Job {
        repo_name: String,
        title: String,
    }

    let mut to_fetch = Vec::new();
    let mut cached = 0usize;
    let books = store.all();
    for b in &books {
        let rn = repo_name(b);
        if rn.is_empty() {
            continue;
        }
        let cover_path = covers_dir.join(format!("{rn}.jpg"));
        if cover_path.exists() {
            cached += 1;
            continue;
        }
        to_fetch.push(Job {
            repo_name: rn,
            title: b.title.clone(),
        });
    }

    let total = to_fetch.len();
    let sem = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut set = tokio::task::JoinSet::new();
    for job in to_fetch {
        let (sem, client, cancel) = (sem.clone(), client.clone(), cancel.clone());
        let covers_dir = covers_dir.to_path_buf();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.ok()?;
            if cancel.load(Ordering::SeqCst) {
                return None;
            }
            let r = fetch_and_save_cover(&client, &job.repo_name, &covers_dir).await;
            Some((job.title, r))
        });
    }

    let (mut done, mut fetched, mut errors) = (0, 0, 0);
    while let Some(joined) = set.join_next().await {
        let Ok(Some((title, r))) = joined else { continue };
        done += 1;
        match r {
            Ok(()) => {
                fetched += 1;
                progress(done, total, title, "fetched".into());
            }
            Err(e) => {
                errors += 1;
                progress(done, total, title, format!("error: {e}"));
            }
        }
    }

    CoverResult {
        fetched,
        cached,
        errors,
    }
}

pub async fn fetch_and_save_cover(
    client: &reqwest::Client,
    repo: &str,
    covers_dir: &Path,
) -> Result<(), String> {
    let url = format!(
        "https://raw.githubusercontent.com/{ORG}/{repo}/master/src/epub/images/cover.svg"
    );

    let resp = client.get(&url).send().await.map_err(|e| format!("{e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let body = resp.bytes().await.map_err(|e| format!("{e}"))?;

    // CPU-heavy work off the async runtime so it doesn't starve network I/O
    let cover_path = covers_dir.join(format!("{repo}.jpg"));
    tokio::task::spawn_blocking(move || {
        let jpeg_data = extract_jpeg_from_svg(&body)?;
        let thumb = resize_jpeg(&jpeg_data)?;
        std::fs::write(&cover_path, &thumb).map_err(|e| format!("write cover: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("task panic: {e}"))?
}

fn extract_jpeg_from_svg(svg: &[u8]) -> Result<Vec<u8>, String> {
    let svg_str = std::str::from_utf8(svg).map_err(|e| format!("SVG not UTF-8: {e}"))?;

    let caps = jpeg_regex()
        .captures(svg_str)
        .ok_or("no JPEG data URI in SVG")?;

    let b64 = &caps[1];
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("base64 decode: {e}"))
}

fn resize_jpeg(data: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
        .map_err(|e| format!("decode image: {e}"))?;

    let (src_w, src_h) = img.dimensions();

    let ratio = src_w as f64 / src_h as f64;
    let target_ratio = THUMB_W as f64 / THUMB_H as f64;

    let (dst_w, dst_h) = if ratio > target_ratio {
        // Width-constrained
        (THUMB_W, (THUMB_W as f64 / ratio) as u32)
    } else {
        // Height-constrained
        ((THUMB_H as f64 * ratio) as u32, THUMB_H)
    };

    let (dst_w, dst_h) = if dst_w >= src_w && dst_h >= src_h {
        (src_w, src_h) // Already smaller, no resize
    } else {
        (dst_w, dst_h)
    };

    let resized = img.resize_exact(dst_w, dst_h, FilterType::Triangle);

    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85);
    resized
        .write_with_encoder(encoder)
        .map_err(|e| format!("encode jpeg: {e}"))?;

    Ok(buf.into_inner())
}
