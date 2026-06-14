# Installation

## Prerequisites

- Rust 1.88 or newer from <https://rustup.rs>
- Chrome or Chromium only for article scraping and auth profiles

No separate SQLite installation is required.

## Install From Source

```bash
git clone https://github.com/fenglyu/rivulet-rss.git
cd rivulet-rss
cargo install --path . --locked
rivulet --version
```

The Cargo package is named `rivulet-rss`, but the command is `rivulet`.

## Uninstall

```bash
cargo uninstall rivulet-rss
```

## Platform Paths

| Platform | Configuration | Database and logs |
|---|---|---|
| macOS | `~/Library/Application Support/rivulet/config.toml` | `~/Library/Application Support/rivulet/` |
| Linux | `~/.config/rivulet/config.toml` | `~/.local/share/rivulet/` |
| Windows | `%APPDATA%\rivulet\config.toml` | `%APPDATA%\rivulet\` |

Run `rivulet init` to create the configuration file. Removing the data
directory deletes local subscriptions, article data, state, and auth profile
metadata. Chrome profile data is stored under the same data directory unless a
custom path was selected.
