# Changelog

Notable changes to `goto`, newest first. Versions follow [semantic
versioning](https://semver.org/); dates are ISO 8601. On release, move the
`Unreleased` entries under a new version header.

## Unreleased

## 0.3.0 — 2026-09-05

### Added

- `gt -` jumps back to the previous repo. Similar to `cd -`, but
  scoped to `gt` jumps: it returns you to the exact directory you left
  and ignores any manual `cd`s in between.
- `GOTO_EXTRA_PRUNE` appends comma-separated directory names to the crawl's
  built-in prune list. The defaults (`node_modules`, `.terraform`, `.git`) are
  always pruned.

## 0.2.2 — 2026-09-02

### Fixed

- Register tab completion whether `~/.goto.zsh` is sourced before or after
  `compinit` runs.

## 0.2.1 — 2026-09-02

### Added

- `gt --help` (also `-h`).

## 0.2.0 — 2026-09-02

### Added

- Tab completion for repo names (zsh).
- `gt --version` (also `-v`).
- `gt upgrade` — update in place from the source clone.

## 0.1.0 — 2026-07-10

### Added

- Jump to any git repo under `~/src` (or `$GOTO_ROOT`) by its directory name,
  with an `fzf` picker for ambiguous matches.
- Async repo-list cache for instant jumps.
- `gt --list` to show known repos as a table.
- Installation via `rx dev up`.
