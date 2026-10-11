# Migrating to Homebrew

For anyone who installed `gt` before it was on Homebrew: with `cargo install`,
from a clone of this repo, or with a copy of the shell file at `~/.goto.zsh`.

## 1. Remove the old install

```sh
cargo uninstall goto
```

If you have a copy of the shell file at `~/.goto.zsh`, delete it:

```sh
rm -f ~/.goto.zsh
```

Then delete the line in your `~/.zshrc` that sources the old file, for example
`source "$HOME/.goto.zsh"` or `source ~/path/to/goto/goto.zsh`.

Don't skip `cargo uninstall`. If `~/.cargo/bin` comes before Homebrew's `bin` on
your `PATH`, a leftover `gt-bin` there keeps running instead of the Homebrew one.

## 2. Install with Homebrew

```sh
brew install ricekrisbs/tap/goto-repo
```

Then add this to your `~/.zshrc`:

```zsh
source "$HOMEBREW_PREFIX/share/goto/goto.zsh"
```

## 3. Open a new shell and check

```sh
which -a gt-bin   # should list only $HOMEBREW_PREFIX/bin/gt-bin
gt --version
gt <some-repo>
```

Your repo index carries over; there's nothing to clear. If you cloned this repo
only to install `gt`, you can delete the clone.

From now on, upgrade with `brew upgrade goto-repo`.
