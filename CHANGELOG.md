# Changelog

Notable changes to `goto`, newest first. Versions follow [semantic
versioning](https://semver.org/); dates are ISO 8601. On release, move the
`Unreleased` entries under a new version header.

## 0.4.0 — 2026-10-10

### Added

- Running `gt-bin` directly with no arguments prints the line to add to
  `~/.zshrc`, for when the `gt` function isn't defined.

### Changed

- Install and upgrade via Homebrew: `brew install ricekrisbs/tap/goto`, then
  `brew upgrade goto`.
- Unknown options are rejected (`gt: unknown option '--lst'`) instead of being
  treated as repo names.
- Extra arguments are rejected (`gt foo bar`, `gt --list foo`) instead of being
  ignored.
- A relative `GOTO_ROOT` is resolved against the current directory once, so the
  cache no longer mixes up two directories with the same relative path.
- `gt --reindex` exits 1 and says why when it can't write the index.
- `gt -` when you're already in the previous directory prints
  `gt: already in the previous directory` and exits 1, instead of silently doing
  nothing.
- Tab completion only offers repo names for the first argument.
- Jumps no longer start a `wc` process to count matches.

### Fixed

- If the exact-name match was deleted, `gt` reported no match even when a
  partial match still existed. It now falls back to the partial match.
- Jumping to the repo you're already in broke the next `gt -`.
- `gt -` failed under `setopt nounset`.
- `gt --complete` went down the jump path and could change directory.
- Tab completion listed the same name more than once when repos differed only in
  case.
- An empty `HOME`, `GOTO_ROOT` or `XDG_CACHE_HOME` was used as a relative path,
  crawling or writing the cache under the current directory. Empty values are
  now treated as unset.
- A repo path containing a newline was written to the cache as two entries.
- `gt` exited 0 without moving if `gt-bin` printed nothing.
- `gt --reindex` said "1 repos".

### Removed

- `gt upgrade` and the `--source` flag it relied on. Homebrew handles upgrades.
- `goto.zsh` no longer adds `~/.cargo/bin` to your `PATH`.

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
