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
3. Get a formula PR open in the tap. Either:
   - **Wait for autobump.** Within a day, the tap's "brew bump" workflow opens a
     PR updating the formula's `url` and `sha256`. A PR it opens doesn't start
     CI on its own: close and reopen it to start the checks. (If no PR appears,
     check that "Allow GitHub Actions to create and approve pull requests" is on
     in the tap's Settings → Actions → General, and look at the "brew bump" run.)
   - **Or open it by hand,** in a clone of the tap:

     ```sh
     curl -sL https://github.com/RiceKrisBs/goto/archive/refs/tags/vX.Y.Z.tar.gz | shasum -a 256
     git switch -c goto-repo-X.Y.Z
     # in Formula/goto-repo.rb: set url to the vX.Y.Z tarball and sha256 to the
     # hash above. Leave the bottle block alone; publishing replaces it.
     git commit -am "goto-repo X.Y.Z"
     git push -u origin goto-repo-X.Y.Z
     ```

     Then open the PR on GitHub. A PR you open starts CI by itself.
4. Wait for the PR's "brew test-bot" checks to pass. They build bottles for
   Apple Silicon (`macos-15`) and Linux.
5. In the tap's Actions tab, run **brew pr-pull** with the PR number, and the
   PR's latest commit SHA as the expected head SHA. It uploads the bottles to a
   release in the tap, adds their hashes to the formula, lands both commits on
   `main`, and closes the PR. Don't merge the PR with the merge button.
6. Check that `brew upgrade goto-repo` picks up the new version.

If a step fails, the run's log is on the tap's Actions tab. A failing
test-bot job on the PR is fixed by pushing to the PR branch, which reruns the
checks; don't run brew pr-pull until they pass. If brew pr-pull itself fails,
the PR stays open and the run can be retried.

### Routine tap maintenance

- **Dependabot** opens PRs that bump the pinned GitHub Actions in the tap's
  workflows. Those don't touch a formula, so merge them with the normal merge
  button once their checks pass.
- **There is no Intel macOS bottle:** homebrew-core no longer ships Intel
  bottles for `rust` or `fzf`, so Intel Macs build `goto-repo` from source.
