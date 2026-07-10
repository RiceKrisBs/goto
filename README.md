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

Requires the [Rust toolchain](https://rustup.rs/) and [`fzf`](https://github.com/junegunn/fzf)
(for choosing between multiple matches).

1. Install the binary to `~/.cargo/bin`:

   ```sh
   cargo install --path .
   ```

   Make sure `~/.cargo/bin` is on your `PATH` (rustup normally adds it).

2. Add the `gt` shell function to your `~/.zshrc`:

   ```zsh
   # gt <name> — jump to a repo under ~/src by its dir name.
   gt() {
     local out target
     out="$(gt-bin "$@")" || return 1
     if [[ "$(print -r -- "$out" | wc -l)" -gt 1 ]]; then
       target="$(print -r -- "$out" | fzf --select-1 --exit-0 --height=40% --reverse)" || return 1
     else
       target="$out"
     fi
     [[ -n "$target" ]] && cd "$target"
   }
   ```

3. Open a new shell (or `source ~/.zshrc`) and try `gt <name>`.

## Updating

`cargo install` copies the binary; it doesn't track the source. After changing
the code, re-run `cargo install --path .` to pick up the new build.

## Configuration

The search root defaults to `~/src`. Override it with `GOTO_ROOT` (a leading `~`
is expanded):

```sh
export GOTO_ROOT=~/code
```

## Notes

- Discovery is done live on every invocation (no cache). It prunes `node_modules`,
  `.terraform`, and `.git` internals, and takes a few hundred milliseconds — fast
  enough to be imperceptible for an interactive `cd`.
