# glit

Interactive git log viewer with fuzzy search and diff preview - right in your terminal.

Built with Rust + [ratatui](https://github.com/ratatui-org/ratatui).

## Demo

(screenshot will be added later :D)

## Features

- Fuzzy search through commit history in real time
- Instant diff preview with syntax highlighting
- Keyboard-only navigation
- Fast - loads 200 commits instantly

## Installation

```bash
cargo install --git https://github.com/bytewx/glit
```

## Usage

Run inside any git repository:

```bash
glit
```

## Controls

| Key | Action |
|-----|--------|
| Type anything | Fuzzy search |
| ↑ / ↓ | Navigate commits |
| PgUp / PgDn | Scroll diff |
| ESC | Quit |

## Built with

- [ratatui](https://github.com/ratatui-org/ratatui) — TUI framework
- [fuzzy-matcher](https://github.com/lotabout/fuzzy-matcher) — Fuzzy search