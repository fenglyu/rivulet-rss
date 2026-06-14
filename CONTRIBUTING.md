# Contributing

## Development Setup

Install Rust 1.88 or newer and Chrome or Chromium if you are testing article
scraping. Then run:

```bash
cargo test --all-targets --locked
```

## Before Opening a Pull Request

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

Keep changes focused, add tests for behavior changes, and update the README,
user guide, shortcuts, or changelog when user-facing behavior changes.

Use GitHub issues for bugs and concrete feature proposals. Security reports
must follow [SECURITY.md](SECURITY.md).
