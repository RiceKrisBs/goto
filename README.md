# goto

Jump to any git repo under `~/src` by its directory name.

> **Have questions, feedback, bugs, feature requests, or just want to say hi?**
> Find me in Slack in [#goto-cli](https://fullscript.enterprise.slack.com/archives/C0BUFBA3S03).

```
gt nitro          # cd ~/src/git.fullscript.io/ai/nitro
gt hw-admin       # cd ~/src/git.fullscript.io/developers/hw-admin
gt secret-sender  # cd ~/src/github.com/Shopify/secret-sender
gt aws-redis      # cd ~/src/git.fullscript.io/devops/terraform/modules/aws-redis
```

Run `gt --help` (or `-h`) for the full list of commands.

## How it works

`goto` builds a binary called `gt-bin` that walks `~/src`, finds every git repo
(any directory containing a `.git` entry, at any depth), and matches your query
against the repo's directory name:

- **Exact match** wins (`gt nitro` → the dir named exactly `nitro`).
- If there's no exact match, it falls back to a **substring match** (`gt adm` → `hw-admin`).
- If **multiple** repos match, it prints them all and an `fzf` picker lets you choose.
- If **nothing** matches, it prints a message and does nothing.

A child process can't change its parent shell's working directory, so `gt-bin`
only *prints* the target path — a small `gt` shell function does the actual `cd`.

## Install

From the repo root, run:

```sh
rx dev up
```

This installs [`fzf`](https://github.com/junegunn/fzf) (for choosing between
multiple matches) and the [Rust toolchain](https://rustup.rs/), builds `gt-bin`
into `~/.cargo/bin`, and copies the `gt` shell function to `~/.goto.zsh`.

`rx` can't modify your shell for you, so one manual step remains. Add this line
to your `~/.zshrc`, then open a new shell:

```zsh
source "$HOME/.goto.zsh"
```

Try `gt <name>`.

## Tab completion

`gt <TAB>` completes repo names from the same index the jump uses. Matching is
**substring-anywhere** and **case-insensitive**, mirroring how `gt` itself
resolves a name — so `gt redis<TAB>` offers `aws-redis`, just as `gt redis`
would jump to it.

Completion is registered automatically when you `source ~/.goto.zsh`, provided
zsh's completion system is loaded. Keep the `source` line *after* `compinit`
runs in your `~/.zshrc` (e.g. after the oh-my-zsh setup). If it's sourced too
early, jumping still works but `<TAB>` won't complete.

Two repos that share a name (e.g. `skills` in two namespaces) collapse to a
single candidate — the name alone can't tell them apart, so completing it and
pressing Enter hands off to the same `fzf` picker used for any ambiguous match.

## Updating

Once installed, upgrade in place from any directory with:

```sh
gt upgrade
```

This pulls the clone `gt` was built from (`--ff-only`), rebuilds `gt-bin`,
refreshes `~/.goto.zsh`, and re-sources it into your current shell — so the new
version is live immediately, no new shell needed. It reuses the same install
scripts as first-time setup (each step skipped when already satisfied), so it
needs `cargo` but **not** `rx`.

If there's nothing upstream and your installed copy already matches the source,
`gt upgrade` reports `already up to date` and does nothing else — no rebuild, no
re-source.

`gt upgrade` locates the clone via a path baked into the binary at build time.
If you've moved the clone since installing, it'll say so — re-run the install
from the clone's new location to re-stamp it.

It only runs when the clone is on `main` (a release is what you're upgrading to,
not whatever branch you're developing on). On any other branch it refuses and
tells you — switch to `main`, or rebuild that branch directly with
`cargo install --path <clone>`.

Prefer to do it by hand? From the clone, either of these does the same rebuild
and refresh (the second needs no `rx`):

```sh
git pull && rx dev up
git pull && ./rx_scripts/build.sh --satisfy && ./rx_scripts/shell-fn.sh --satisfy
```

The manual paths don't re-source for you, so if the `gt` shell function changed,
open a new shell or re-source it:

```sh
source "$HOME/.goto.zsh"
```

Check which build is on your `PATH` with:

```sh
gt --version   # or: gt -v
```

## Configuration

The search root defaults to `~/src`. Override it with `GOTO_ROOT` (a leading `~`
is expanded):

```sh
export GOTO_ROOT=~/code
```

## Caching

To keep jumps instant, `goto` caches the discovered repo list at
`${XDG_CACHE_HOME:-~/.cache}/goto/index`:

- The **first** call after the cache is empty (or after switching `GOTO_ROOT`)
  crawls live and writes the cache — a few hundred milliseconds.
- **Subsequent** calls read the cache (~2ms) and, in the background, kick off a
  detached re-crawl so newly cloned or removed repos are reflected next time.
  This means a brand-new repo is picked up on the *second* `gt` after cloning it.
- The cache records the root it was built for, so changing `GOTO_ROOT`
  invalidates it automatically.

Force an immediate rebuild any time with:

```sh
gt --reindex
```

List every repo `goto` is aware of, sorted alphabetically:

```sh
gt --list
```

The crawl prunes `node_modules`, `.terraform`, and `.git` internals.
