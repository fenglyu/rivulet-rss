# Rivulet RSS

Rivulet RSS is a terminal-first, offline-first RSS and Atom reader written in
Rust. The Cargo package is `rivulet-rss`; the installed command is `rivulet`.

## Features

- Latest and Reader views in a keyboard-driven Ratatui interface
- Local SQLite storage for feeds, items, search, and reading state
- Read, starred, queued, saved, and archived item states
- Conditional feed requests with ETag and Last-Modified support
- OPML import, full-text search, and scheduled background refresh
- Optional Chrome-based article extraction and user-managed login profiles
- Configurable colors, keybindings, refresh windows, and scraper limits

Only HTTP and HTTPS URLs are accepted. Feed responses, browser extraction,
page load time, and concurrent work are bounded.

## Requirements

- Rust 1.88 or newer
- Chrome or Chromium only when using article scraping or auth profiles

SQLite is built from source through `rusqlite`'s bundled feature.

## Install

```bash
git clone https://github.com/fenglyu/rivulet-rss.git
cd rivulet-rss
cargo install --path . --locked
rivulet --version
```

The prerelease is not published to crates.io. See [docs/INSTALL.md](docs/INSTALL.md)
for platform paths and uninstall instructions.

## Quick Start

```bash
rivulet add https://blog.rust-lang.org/feed.xml
rivulet import feeds.opml
rivulet update
rivulet tui
```

The default fetch worker count is 30. Override it globally with
`--workers <N>`; zero is rejected.

## Commands

```text
rivulet init [--force]
rivulet add <URL>
rivulet remove <URL>
rivulet import <FILE.opml>
rivulet update
rivulet list [--items] [--unread|--starred|--queued|--saved|--archived]
rivulet search <QUERY> [--limit N] [item-state filter]
rivulet scrape [--feed URL] [--limit N] [--concurrency N] [--visible]
rivulet auth add <NAME> --site <URL>
rivulet auth check <NAME> [--url URL] [--visible]
rivulet auth list
rivulet daemon start [--interval 1h] [--foreground]
rivulet daemon stop
rivulet daemon status
rivulet tui
```

## TUI

The Latest tab shows recent items and marks entries inserted by the newest
refresh as `NEW`. The Reader tab uses a collapsible feed rail, item list, and
article preview.

| Key | Action |
|---|---|
| `j` / `k`, arrows | Move or change pane |
| `Enter` | Open the highlighted feed or focus preview |
| `Alt+1` / `[` | Latest tab |
| `Alt+2` / `]` | Reader tab |
| `\` | Toggle the Reader feed rail |
| `r`, `s`, `L`, `S`, `x` | Toggle read, star, queue, save, archive |
| `a`, `u`, `f`, `Q`, `v`, `X` | Select item view |
| `o` | Open an HTTP(S) item link |
| `R` | Refresh feeds |
| `m` | Toggle maximized preview |
| `q` | Quit |

See [SHORTCUTS.md](SHORTCUTS.md) for the full configurable key map.

## Configuration And Data

Run `rivulet init` to create the sample configuration.

| Platform | Configuration | Database |
|---|---|---|
| macOS | `~/Library/Application Support/rivulet/config.toml` | `~/Library/Application Support/rivulet/rivulet.db` |
| Linux | `~/.config/rivulet/config.toml` | `~/.local/share/rivulet/rivulet.db` |
| Windows | `%APPDATA%\rivulet\config.toml` | `%APPDATA%\rivulet\rivulet.db` |

## Development

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
cargo build --release --locked
cargo package
```

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and
[LICENSE](LICENSE).
