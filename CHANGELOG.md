# Changelog

All notable changes to Rivulet RSS are documented here.

## [Unreleased]

## [0.1.0-rc.1] - 2026-06-14

### Added

- Latest and Reader TUI views with persisted refresh batches
- Search and read, starred, queued, saved, and archived states
- Optional Chrome article scraping and user-managed auth profiles
- Background refresh daemon
- GitHub CI, security policy, contribution guide, and release templates

### Security

- Allow only HTTP and HTTPS URLs
- Remove Chrome's `--no-sandbox` argument
- Reject zero worker, scraper concurrency, timeout, and daemon interval values
- Bound feed downloads and extracted article content
- Enforce scraper operation timeouts
- Verify daemon process identity before sending stop signals

### Changed

- Rename the Cargo package to `rivulet-rss`; keep the `rivulet` binary name
- Set the minimum supported Rust version to 1.88
- Bundle SQLite for consistent Linux, macOS, and Windows installation
- Restrict the Cargo package to release-relevant files

[Unreleased]: https://github.com/fenglyu/rivulet-rss/compare/v0.1.0-rc.1...HEAD
[0.1.0-rc.1]: https://github.com/fenglyu/rivulet-rss/releases/tag/v0.1.0-rc.1
