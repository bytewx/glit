# Contributing to glit

Thanks for considering contributing! This project is small, so the process is kept simple.

## Getting started

1. Fork the repo and clone your fork
2. Make sure you have a recent stable Rust toolchain (edition 2024, so Rust 1.85+)
3. Build and run:
   ```bash
   cargo build
   cargo run
   ```
   (run it inside any git repository so there's something to browse)

## Before opening a PR

Run these locally: the same checks run in CI and a failing check will block merging:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

If `cargo fmt` complains, just run `cargo fmt --all` to fix formatting automatically.

## Project structure

- `src/main.rs` — event loop, keybindings, wiring
- `src/git.rs` — everything that shells out to `git` and parses its output
- `src/state.rs` — `App` state, filtering, navigation
- `src/ui.rs` — rendering (ratatui widgets)
- `src/config.rs` — tunables (max commits, diff char limit)
- `src/error.rs` — `AppError`

## Submitting a change

- Keep PRs focused: one feature or fix at a time is easier to review
- Add or update tests in `src/git.rs` if you're touching parsing logic (there's decent coverage there already, including Unicode and edge cases — please keep it that way)
- Update `README.md` (Features / Controls / Changelog) if the change is user-facing
- Open the PR against `main`

## Reporting bugs

Open an issue with:
- What you expected to happen
- What actually happened
- Your OS and terminal emulator (TUI rendering issues are often terminal-specific)
- `git --version` and `glit --version` if relevant

## Releasing (maintainer notes)

Versions follow semver. To cut a release:
1. Bump `version` in `Cargo.toml` and `Cargo.lock`
2. Add a changelog entry to `README.md`
3. Commit, push to `main`
4. Tag: `git tag -a vX.Y.Z -m "vX.Y.Z"` and `git push origin vX.Y.Z`
5. The `Release` workflow builds binaries for Linux/macOS/Windows and attaches them with checksums
6. If publishing to crates.io: `cargo publish` (package name is `glit-tui`, binary stays `glit`)