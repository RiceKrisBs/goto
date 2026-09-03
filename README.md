# goto

Jump to any git repo under `~/src` by its directory name.

> **Have questions, feedback, bugs, feature requests, or just want to say hi?**
> Find me in Slack in [#goto-cli](https://fullscript.enterprise.slack.com/archives/C0BUFBA3S03).

## Quickstart

### Install

From the repo root:

```sh
rx dev up
```

This installs [`fzf`](https://github.com/junegunn/fzf) and the
[Rust toolchain](https://rustup.rs/), builds the `gt-bin` binary into
`~/.cargo/bin`, and writes the `gt` shell function to `~/.goto.zsh`. `rx` can't
edit your shell for you, so finish by adding this to your `~/.zshrc` and opening a
new shell:

```zsh
source "$HOME/.goto.zsh"
```

### Configure the search root

`goto` searches `~/src` by default. If your repos live somewhere else, point it
there with `GOTO_ROOT` in your `~/.zshrc` (a leading `~` is expanded):

```sh
export GOTO_ROOT=~/dev
```

### Use

Jump to a repo by its directory name:

```sh
gt nitro          # cd ~/src/git.fullscript.io/ai/nitro
gt hw-admin       # cd ~/src/git.fullscript.io/developers/hw-admin
gt secret-sender  # cd ~/src/github.com/Shopify/secret-sender
gt aws-redis      # cd ~/src/git.fullscript.io/devops/terraform/modules/aws-redis
```

A partial name works too (`gt adm` → `hw-admin`), and tab completion is built in
(`gt hw-a<TAB>` → `hw-admin`). If several repos match, an `fzf` picker lets you
choose.

Run `gt --help` (or `-h`) for the full list of commands.

### Upgrade

From any directory:

```sh
gt upgrade
```

This pulls the latest `gt`, rebuilds it, and reloads it into your current shell.
If you're already up to date, it says so and does nothing.

## How it works

`goto` builds a binary called `gt-bin` that walks `~/src`, finds every git repo
(any directory containing a `.git` entry, at any depth), and matches your query
against the repo's directory name:

- **Exact match** wins (`gt nitro` → the dir named exactly `nitro`).
- If there's no exact match, it falls back to a **substring match** (`gt adm` → `hw-admin`).
- If **multiple** repos match, it prints them all and an `fzf` picker lets you choose.
- If **nothing** matches, it prints a message and does nothing.

A child process can't change its parent shell's working directory, so `gt-bin`
only _prints_ the target path — a small `gt` shell function does the actual `cd`.

## Tab completion

`gt <TAB>` completes repo names from the same index the jump uses. Matching is
**substring-anywhere** and **case-insensitive**, mirroring how `gt` itself
resolves a name — so `gt redis<TAB>` offers `aws-redis`, just as `gt redis`
would jump to it.

Completion is registered automatically when you `source ~/.goto.zsh` — whether
that happens before or after `compinit` runs. If `compinit` hasn't run yet, goto
defers registration to the first prompt.

It does need `compinit` to run _somewhere_ in your shell startup. Frameworks
like oh-my-zsh do this for you. If `gt <TAB>` doesn't complete (and neither does
any other command), your shell isn't initializing zsh's completion system at
all — add this to your `~/.zshrc` and open a new shell:

```zsh
autoload -Uz compinit && compinit
```

Two repos that share a name (e.g. `skills` in two namespaces) collapse to a
single candidate — the name alone can't tell them apart, so completing it and
pressing Enter hands off to the same `fzf` picker used for any ambiguous match.

## Upgrading in depth

`gt upgrade` brings your install up to date with the clone it was built from. It:

1. fast-forwards the clone (`git pull --ff-only`),
2. rebuilds `gt-bin` and refreshes `~/.goto.zsh` (each step skipped when already
   satisfied), then
3. re-sources `~/.goto.zsh` so the new function is live in your current shell —
   no new shell needed.

It reuses the same install scripts as first-time setup, called directly, so it
needs `cargo` but **not** `rx`. If there's nothing upstream and your installed
copy already matches the source, it reports `already up to date` and does
nothing else.

A couple of guardrails:

- **It finds the clone** via a path baked into the binary at build time. If
  you've moved the clone since installing, `gt upgrade` says so — re-run the
  install from its new location to re-stamp the path.
- **It only runs on `main`** (a release is what you're upgrading to, not
  whatever branch you're developing on). On any other branch it refuses —
  switch to `main`, or rebuild that branch directly with
  `cargo install --path <clone>`.

Prefer to upgrade by hand? From the clone, either of these does the same rebuild
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

## Caching

To keep jumps instant, `goto` caches the discovered repo list at
`${XDG_CACHE_HOME:-~/.cache}/goto/index`:

- The **first** call after the cache is empty (or after switching `GOTO_ROOT`)
  crawls live and writes the cache — a few hundred milliseconds.
- **Subsequent** calls read the cache (~2ms) and, in the background, kick off a
  detached re-crawl so newly cloned or removed repos are reflected next time.
  This means a brand-new repo is picked up on the _second_ `gt` after cloning it.
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
