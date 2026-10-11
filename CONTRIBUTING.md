# Contributing to goto

## Before you start

- **Small fixes** (bugs, typos, docs): open a pull request directly.
- **Anything bigger** (a new flag, a change in behaviour, a new dependency):
  open an issue first, so we can agree on the approach before you spend time on
  it.

Contributions are released under the project's [MIT license](LICENSE).

## What a pull request needs

- **Tests.** A bug fix comes with a test that fails without the fix. Rust tests
  live in `mod tests` at the bottom of `src/main.rs`; tests for the `gt` shell
  function live in `tests/goto_zsh_test.zsh`.
- **Green CI.** CI runs formatting, clippy with warnings as errors, both test
  suites on Linux and macOS, and a build on the minimum supported Rust version.
- **The minimum supported Rust version stays 1.85** unless there's a reason to
  raise it that's worth discussing in an issue. Don't raise it to silence a lint.
- **A CHANGELOG entry** for anything a user would notice, under an
  `## Unreleased` heading at the top of [`CHANGELOG.md`](CHANGELOG.md) (add the
  heading if it isn't there).
- **Few comments.** Comment only what the code can't say for itself.

## Development setup

You'll need a Rust toolchain v1.85 or newer ([rustup](https://rustup.rs)), zsh,
and [`fzf`](https://github.com/junegunn/fzf).

```sh
git clone https://github.com/RiceKrisBs/goto.git
cd goto
cargo build
```

## Running the checks

These are the same checks CI runs:

```sh
cargo test  # Rust tests
cargo build && GT_BIN=target/debug/gt-bin zsh -f tests/goto_zsh_test.zsh  # gt shell function, incl. against the real binary
cargo fmt --check && cargo clippy --all-targets -- -D warnings  # lints
```

Run the shell tests with `zsh -f`, as above, so your own `~/.zshrc` can't
interfere. Without `GT_BIN`, the cases that run the real binary are skipped.

## Trying your changes

To try a dev build without disturbing an installed copy, start a separate shell
that uses the build from your clone:

```sh
cargo build
PATH="$PWD/target/debug:$PATH" XDG_CACHE_HOME=/tmp/goto-dev zsh
```

Then, inside that shell:

```zsh
source ./goto.zsh   # this shell's gt is now the clone's version
gt <some-repo>      # exercises your build
exit                # back to your normal gt
```

- The `PATH` prefix makes this shell's `gt-bin` your build, and sourcing
  `./goto.zsh` picks up any edits to the shell function.
- `XDG_CACHE_HOME` keeps the dev build's repo index separate from your normal
  one, which matters if you change the cache format or the crawl.
- After changing Rust code, re-run `cargo build`; there's no need to restart
  the shell.

Don't `cargo install` a dev build while the Homebrew one is installed. That
leaves two `gt-bin`s on your `PATH`, and which one runs depends on the order.

## Releasing

For maintainers. Releases are published to the
[RiceKrisBs/homebrew-tap](https://github.com/RiceKrisBs/homebrew-tap) formula
`goto-repo`.

1. Rename `## Unreleased` in `CHANGELOG.md` to `## X.Y.Z — YYYY-MM-DD`, and set
   `version` in `Cargo.toml`. Run `cargo build` so `Cargo.lock` picks up the new
   version (CI checks it with `--locked`).
2. Commit as "Release X.Y.Z", then tag and push:

   ```sh
   git tag -a vX.Y.Z -m "goto X.Y.Z"
   git push origin main vX.Y.Z
   ```

   Tags can't be moved or deleted once pushed; a fix after release is a new
   version.
3. Within a day, the tap's autobump workflow opens a PR updating the formula's
   `url` and `sha256`. To skip the wait, open that PR by hand. A PR opened by
   autobump doesn't start CI on its own: close and reopen it.
4. When the PR's "brew test-bot" checks pass, run the tap's **brew pr-pull**
   workflow with the PR number. It publishes the bottles and lands the formula
   on `main`. Don't merge the PR with the merge button.
5. Check that `brew upgrade goto-repo` picks up the new version.
