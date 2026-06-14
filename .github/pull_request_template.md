## Summary

Describe the user-visible or internal change.

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test --all-targets --locked`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`

## Documentation

- [ ] User-facing documentation and changelog updated, or not applicable
- [ ] No private URLs, credentials, cookies, logs, or personal data included
