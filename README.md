# goto

Jump to any git repo under `~/src` by its directory name.

```
gt nitro          # cd ~/src/git.fullscript.io/ai/nitro
gt hw-admin       # cd ~/src/git.fullscript.io/developers/hw-admin
gt secret-sender  # cd ~/src/github.com/Shopify/secret-sender
gt aws-redis      # cd ~/src/git.fullscript.io/devops/terraform/modules/aws-redis
```

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

## Updating

`cargo install` copies the binary; it doesn't track the source. After changing
the code, re-run `rx dev up` (or `cargo install --path .`) to pick up the new
build. Re-running is safe — each step is skipped if it's already satisfied.

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
