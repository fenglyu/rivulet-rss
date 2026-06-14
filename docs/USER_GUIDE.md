# Rivulet RSS User Guide

## First Run

```bash
rivulet init
rivulet add https://blog.rust-lang.org/feed.xml
rivulet update
rivulet tui
```

Rivulet accepts only HTTP and HTTPS URLs.

## Reading From The CLI

```bash
rivulet list
rivulet list --items
rivulet list --unread
rivulet list --queued
rivulet search "rust release" --limit 20
```

Use only one item-state filter at a time.

## Importing OPML

```bash
rivulet import feeds.opml
```

The default parallel fetch count is 30. Use `--workers <N>` before or after the
subcommand to change it.

## TUI Views

The Latest tab displays recent items across feeds, with `NEW` marking items
inserted by the most recently completed refresh. The Reader tab contains a
collapsible feed rail, item list, and preview.

Common keys:

| Key | Action |
|---|---|
| `j` / `k` | Move down / up |
| `h` / `l`, left / right | Focus adjacent pane |
| `Enter` | Open feed or focus preview |
| `Alt+1` / `Alt+2` | Latest / Reader |
| `\` | Toggle feed rail |
| `r`, `s`, `L`, `S`, `x` | Read, star, queue, save, archive |
| `a`, `u`, `f`, `Q`, `v`, `X` | All, unread, starred, queued, saved, archived |
| `o` | Open selected HTTP(S) link and mark it read |
| `R` | Refresh |
| `q` | Quit |

All bindings are configurable. See [../SHORTCUTS.md](../SHORTCUTS.md).

## Article Scraping

Chrome or Chromium is required for these commands:

```bash
rivulet scrape --limit 10 --concurrency 3
rivulet scrape --feed https://example.com/feed.xml
rivulet auth add example --site https://example.com/login
rivulet auth check example --url https://example.com/account
rivulet scrape --auth-profile example --limit 10
```

`auth add` opens a visible browser. Rivulet does not collect passwords; browser
session data stays in the selected Chrome profile directory. Scraping enforces
the configured timeout, concurrency, and extracted-content limits.

## Background Refresh

```bash
rivulet daemon start --interval 1h
rivulet daemon status
rivulet daemon stop
```

Intervals accept seconds, minutes, hours, or days, such as `30m`, `6h`, and
`1d`. Zero is rejected. The daemon validates PID ownership before signaling a
process.

## Logging

Set `RUST_LOG` for CLI logging:

```bash
RUST_LOG=rivulet=debug rivulet update
```

The TUI writes to `tui.log` in the platform data directory. Override it with
`RIVULET_TUI_LOG`.

## Troubleshooting

- HTTP 304 means the remote feed has not changed.
- Scraping requires Chrome or Chromium, but ordinary feed reading does not.
- Invalid configuration values are reported at startup instead of silently
  disabling timeouts or worker pools.
