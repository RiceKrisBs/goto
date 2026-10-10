# goto

Jump to any git repo under `~/src` by its directory name.

Requires zsh, on macOS or Linux.

## Quickstart

### Install

```sh
brew install ricekrisbs/tap/goto
```

Homebrew can't edit your shell config, so finish by adding this to your
`~/.zshrc` and opening a new shell:

```zsh
source "$HOMEBREW_PREFIX/share/goto/goto.zsh"
```

`$HOMEBREW_PREFIX` is exported by `brew shellenv`. If it isn't set in your
shell, use the literal path that `brew --prefix` prints.

### Install from source

To install from source, you'll need a Rust toolchain v1.85 or newer
([rustup](https://rustup.rs)), plus zsh.

[`fzf`](https://github.com/junegunn/fzf) is optional, but needed for the picker
when several repos match.

```sh
git clone https://github.com/RiceKrisBs/goto.git
cd goto
cargo install --path . --locked   # builds gt-bin into ~/.cargo/bin
```

Make sure `~/.cargo/bin` is on your `PATH`, then source the shell function from
the clone in your `~/.zshrc` and open a new shell:

```zsh
source ~/path/to/goto/goto.zsh
```

Because that sources the file straight from the clone, edits to `goto.zsh` show
up in your next shell. After changing the Rust code, re-run
`cargo install --path . --locked` to update `gt-bin`.

Check it worked with `gt --version`.

### Development

```sh
cargo test                       # Rust tests
zsh -f tests/goto_zsh_test.zsh   # tests for the gt shell function
cargo fmt --check && cargo clippy --all-targets -- -D warnings   # what CI lints
```

### Configure the search root

`goto` searches `~/src` by default. If your repos live somewhere else, point it
there with `GOTO_ROOT` in your `~/.zshrc` (a leading `~` is expanded):

```sh
export GOTO_ROOT=~/dev
```

### Tune what the crawl skips

The crawl always skips `node_modules`, `.terraform`, and `.git`. To skip more
directories — matched by name, at any depth — list them comma-separated in
`GOTO_EXTRA_PRUNE` in your `~/.zshrc`:

```sh
export GOTO_EXTRA_PRUNE=vendor,dist,.venv
```

Surrounding whitespace is trimmed, so `vendor, dist, .venv` works too. This only
adds to the built-in list; the three defaults are always pruned.

### Use

Jump to a repo by its directory name, wherever it sits in the tree:

```sh
gt ripgrep    # cd ~/src/github.com/BurntSushi/ripgrep
gt dotfiles   # cd ~/src/github.com/you/dotfiles
gt aws-vpc    # cd ~/src/gitlab.com/acme/terraform/modules/aws-vpc
```

A partial name works too (`gt ripg` → `ripgrep`), and tab completion is built in
(`gt rip<TAB>` → `ripgrep`). If several repos match, an
[`fzf`](https://github.com/junegunn/fzf) picker lets you choose.

Bounce back to the repo you were just in — a two-item toggle, like `cd -`:

```sh
gt -          # back to the repo you jumped from
```

`gt -` returns you to the exact directory you left (subdirectory and all) before
your last `gt` jump, and toggles: run it again to come back. A manual `cd` in
between becomes the other end of the toggle. It's per-shell — a new terminal has
no previous repo yet.

Run `gt --help` (or `-h`) for the full list of commands.

## How it works

`goto` installs a binary called `gt-bin` that walks `~/src`, finds every git repo
(any directory containing a `.git` entry, at any depth), and matches your query
against the repo's directory name:

- **Exact match** wins (`gt ripgrep` → the dir named exactly `ripgrep`).
- If there's no exact match, it falls back to a **substring match** (`gt ripg` → `ripgrep`).
- If **multiple** repos match, it prints them all and an `fzf` picker lets you choose.
- If **nothing** matches, it prints a message and does nothing.

A child process can't change its parent shell's working directory, so `gt-bin`
only _prints_ the target path — a small `gt` shell function does the actual `cd`.

## Tab completion

`gt <TAB>` completes repo names from the same index the jump uses. Matching is
**substring-anywhere** and **case-insensitive**, mirroring how `gt` itself
resolves a name — so `gt grep<TAB>` offers `ripgrep`, just as `gt grep` would
jump to it.

Completion needs zsh's completion system initialized somewhere in your shell
startup. Frameworks like oh-my-zsh do this for you. If `gt <TAB>` doesn't
complete (and neither does any other command), add this to your `~/.zshrc` and
open a new shell:

```zsh
autoload -Uz compinit && compinit
```

Two repos that share a name (e.g. `dotfiles` in two namespaces) collapse to a
single candidate — the name alone can't tell them apart, so completing it and
pressing Enter hands off to the same `fzf` picker used for any ambiguous match.

## Upgrading

```sh
brew upgrade goto
```

If the `gt` shell function changed, open a new shell or re-source it:

```zsh
source "$HOMEBREW_PREFIX/share/goto/goto.zsh"
```

Check which build is on your `PATH` with:

```sh
gt --version   # or: gt -v
```

See [`CHANGELOG.md`](CHANGELOG.md) for what changed between versions.

## Caching

To keep jumps instant, `goto` caches the discovered repo list at
`${XDG_CACHE_HOME:-~/.cache}/goto/index`:

- The **first** call after the cache is empty (or after switching `GOTO_ROOT`)
  crawls live and writes the cache — a few hundred milliseconds.
- **Subsequent** calls read the cache (~2ms) and, in the background, kick off a
  detached re-crawl so newly cloned or removed repos are reflected next time.
  This means a brand-new repo is picked up on the _second_ `gt` after cloning it.
- The cache records the root it was built for, so changing `GOTO_ROOT`
  invalidates it automatically. There is only **one** cache file, shared by every
  root — so if you alternate between two `GOTO_ROOT` values, each call
  invalidates the other's cache and every call pays for a cold crawl.
- Caching needs somewhere to write. If `HOME` is unset and `XDG_CACHE_HOME`
  isn't set either, caching is silently off and **every** call crawls.
- `goto` trusts its own cache: the recorded root is checked, but the individual
  repo paths are not, so a cache entry can name any path on the filesystem.
  That's fine inside your own cache directory, but don't point
  `XDG_CACHE_HOME` at somewhere other local users can write — they could both
  read your repo layout and steer `gt`.

Force an immediate rebuild any time with:

```sh
gt --reindex
```

List every repo `goto` is aware of, sorted alphabetically:

```sh
gt --list
```

`gt --list` and tab completion show the cache as-is, so a repo you've deleted
keeps appearing there until the next re-crawl (any `gt <name>`, or
`gt --reindex`). Jumping is unaffected: `gt <name>` skips paths that no longer
exist.

The crawl prunes `node_modules`, `.terraform`, and `.git` internals, plus any
directory names you add via `GOTO_EXTRA_PRUNE` (see
[Tune what the crawl skips](#tune-what-the-crawl-skips)).

Directories the crawl can't read are skipped silently, and the exit status is
still 0. If a repo you expect is missing from `gt --list`, check that every
directory on the way to it is readable.

The crawl doesn't follow symlinks, so a repo reached through a symlinked
directory isn't found. A repo nested inside another (a vendored checkout, say)
is indexed alongside its parent.

## License

MIT — see [`LICENSE`](LICENSE).
