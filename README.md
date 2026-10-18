# StandardIssue (Advanced)

> The Advanced edition of [StandardIssue](https://github.com/NeuralDrifter/StandardIssue): everything in the simple edition plus a built-in EPUB reader.

A fast, native desktop app for managing the entire [Standard Ebooks](https://standardebooks.org) catalog. Built with **Tauri v2** and **Rust**: a plain HTML, CSS and JavaScript frontend (no frameworks, no bundlers) on top of a pure Rust backend.

## What It Does

StandardIssue syncs every ebook from the Standard Ebooks GitHub organization, enriches each entry with full metadata from the original OPF files, fetches cover art, and builds valid `.epub` files locally by shallow-cloning repos and zipping their `src/` directories to spec.

All 1,300+ books are organized automatically:

```
epubs/
├── Fiction/
│   ├── James Joyce/
│   │   └── Dubliners_James Joyce (Standard Ebooks).epub
│   ├── Agatha Christie/
│   │   └── Poirot/
│   │       └── The Mysterious Affair at Styles_Agatha Christie (Standard Ebooks).epub
│   └── ...
├── Science Fiction/
├── Philosophy/
├── Poetry/
└── ...
```

## Features

- **Catalog Sync** — Pulls the full repo list from the Standard Ebooks GitHub org via API, parses descriptions, and merges with existing data
- **Metadata Enrichment** — Fetches each book's `content.opf` from GitHub and extracts genres, word count, reading ease, descriptions, subjects, series info, collections, sources, and more; reads the colophon for the original publication date and the cover painting's credit
- **Cover Fetching** — Extracts embedded JPEG data from SVG cover files, resizes to thumbnails, and caches locally
- **Epub Building** — Shallow-clones each repo, zips `src/` into a valid EPUB (mimetype first, uncompressed, per spec), and files it by author/series (optionally by genre too)
- **Build One, Some or All** — One click builds a single book; tick covers to build a selection; Build All queues everything not yet built. Single and bulk builds can also fetch that book's metadata and cover art, so nobody has to enrich the whole catalog first
- **Live Build Progress** — Every card in the pipeline shows its own percentage (git clone progress, then packaging), queue position, and a per-book build log with the commit it was built from
- **Search & Filters** — Search title, author, genre or subject; filter by author, genre, era (from the original publication date) and build status, with live counts; sort by title, author, year, length or build date
- **Built-in Reader** — Read any downloaded book inside the app, with the library sidebar still at hand: Obsidian, Parchment, Cream and Pure OLED atmospheres; original, serif, sans or mono type; font size in pixels, line length, line spacing, justification and drop caps; single, two-page or scroll layout; running heads; contents with a chapter filter and estimated minutes per chapter; full-text search; bookmarks; a scrubber with chapter ticks; reading-time estimates from your own measured pace; fullscreen; endnote jumps with a back button; resumes exactly where you left off
- **Continue Reading** — Sidebar shortcut to your most recent book, plus progress bars on started covers
- **Open in System Reader** — Still available for any downloaded epub
- **Cancel Operations** — All long-running operations support cancellation
- **Concurrent Downloads** — Parallel fetching with configurable semaphore limits

## Requirements

- [Rust](https://rustup.rs/) (stable)
- [Tauri CLI v2](https://v2.tauri.app/start/prerequisites/)
- `git` (for cloning Standard Ebooks repos during epub building)
- [Node.js](https://nodejs.org/) (builds the Tailwind CSS)
- Linux: `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf`

## Build & Run

```bash
# Install Tauri CLI if you haven't
cargo install tauri-cli --version "^2"

# Frontend tooling (Tailwind CSS), once
npm install

# Dev mode (Tailwind rebuilds the CSS automatically)
cd src-tauri
cargo tauri dev

# Release build
cargo tauri build
```

## Project Structure

```
src-tauri/
├── src/
│   ├── main.rs        # Tauri app setup, all IPC command handlers
│   ├── lib.rs         # Module declarations
│   ├── books.rs       # Book model, per-book updates, epub paths, eras
│   ├── query.rs       # Search, filters with facet counts, sorting, paging
│   ├── catalog.rs     # GitHub org sync — fetches all ebook repos
│   ├── enrich.rs      # OPF metadata parser — genres, word count, series
│   ├── colophon.rs    # Publication date and cover artwork credit
│   ├── covers.rs      # SVG cover extraction, JPEG resize, caching
│   ├── build.rs       # Build queue, git clone progress, EPUB packaging, logs
│   ├── library.rs     # Shared library folder, migration, genre re-filing
│   ├── settings.rs    # settings.json and activity.json
│   └── fsutil.rs      # Atomic JSON writes
├── styles/app.css     # Tailwind entry: design tokens and components
├── frontend/
│   ├── index.html     # App shell: title bar, sidebar, toolbar, shelf, inspector
│   ├── js/            # ES modules: api, state, actions and one per component
│   ├── js/reader/     # Reader: shell, contents, typesetting, search, bookmarks, pace
│   ├── dist/app.css   # Generated by Tailwind (not committed)
│   ├── vendor/fonts/  # Playfair Display, Manrope, Material Symbols (with licenses)
│   ├── sanitize.js    # Strips scripts from book content before display
│   └── vendor/foliate-js/  # Pinned EPUB renderer (see VERSION)
├── capabilities/
│   └── default.json   # Tauri permission scopes
├── Cargo.toml
└── tauri.conf.json
```

## How It Works

1. **Sync** pulls every repo from `github.com/standardebooks` and filters for actual ebook repos (those with "Standard Ebooks edition" in the description)
2. **Enrich** hits each repo's raw `content.opf` on GitHub and parses the XML for metadata — categories, word count, Flesch reading ease, series membership, Wikipedia links, and more
3. **Covers** fetches the SVG cover from each repo, extracts the base64-encoded JPEG embedded inside, resizes it to a 280×420 thumbnail, and saves it
4. **Build** shallow-clones each repo (`git clone --depth 1 --progress`, parsed for live progress), then zips the `src/` directory into a valid EPUB file — mimetype entry first and uncompressed per the EPUB spec, everything else deflated. The epub is written to a temporary file and moved into place, so a failed rebuild never damages the existing copy

All operations run concurrently with semaphore-bounded parallelism, emit progress events to the frontend via Tauri's event system, and support cancellation through an atomic flag.

## Data Storage

- **Books metadata** is stored as JSON in the Tauri app data directory (`~/.local/share/com.neuraldrifter.ebook-manager/books.json` on Linux)
- **Epubs** are saved in `Documents\StandardIssue Library\epubs`
- **Covers** are cached in `Documents\StandardIssue Library\covers`
- **Reading positions, bookmarks, measured reading pace and reader settings** are stored in `reading.json` next to `books.json`
- **App settings**, last-run summaries, build records and per-book build logs are in `settings.json`, `activity.json`, `builds.json` and `build-logs/` in the same folder

Older versions kept `epubs/` and `covers/` inside the project folder; they are moved to the shared library automatically on first launch.

### Simple and Advanced editions

StandardIssue comes in two editions — [Simple](https://github.com/NeuralDrifter/StandardIssue) and [Advanced](https://github.com/NeuralDrifter/StandardIssue-Advanced) — that share the same library and data. Only one can run at a time: if you open one while the other is running, it shows a waiting screen and carries on automatically once the other is closed.

## Tech Stack

| Layer    | Technology |
|----------|-----------|
| Backend  | Rust, Tokio, reqwest, roxmltree, zip, image |
| Frontend | HTML and ES modules, Tailwind CSS v4, Tauri IPC |
| Desktop  | Tauri v2 |

No Electron. No React. Plain ES modules styled with Tailwind; the EPUB renderer is a vendored copy of [foliate-js](https://github.com/johnfactotum/foliate-js) (MIT), pinned to the commit in `frontend/vendor/foliate-js/VERSION`.

## License

MIT

---

*Built by [NeuralDrifter](https://github.com/NeuralDrifter)*
