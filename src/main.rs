use std::env;
use std::io::{self, IsTerminal};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode, Stdio};
use std::time::Duration;

mod cache;
mod crawl;
mod matching;
mod scope;
#[cfg(test)]
mod test_util;

use cache::{
    RefreshLock, cache_older_than, cache_path, lock_refresh, read_cache_or_crawl, try_lock_refresh,
    write_cache_at,
};
use crawl::crawl;
use matching::{MatchKind, match_key, match_repos};
use scope::{Scope, prune_from_env, resolve_root};

const HELP: &str = "\
gt — jump to any git repo under your source root by its directory name.

Usage:
  gt <name>       Jump to the repo whose dir name matches <name> (exact match,
                  else substring, else close spelling; an fzf picker opens if
                  several match).
  gt -            Jump back to the previous repo (toggles with your last jump).
  gt --list       List every known repo, with paths.
  gt --reindex    Rebuild the repo index now.
  gt --version    Print the version (also: -v).
  gt --help       Show this help (also: -h).

Tab-complete repo names with <TAB> (zsh). The search root defaults to ~/src;
override it with $GOTO_ROOT. The crawl skips node_modules, .terraform, and .git;
append more directory names with $GOTO_EXTRA_PRUNE (comma-separated).";

const SOURCE_HINT: &str = "\
gt-bin is the backend for the `gt` shell function. To define `gt`, add this to
your ~/.zshrc and open a new shell:
  source \"$HOMEBREW_PREFIX/share/goto/goto.zsh\"";

// Cooldown after a refresh lands: skip spawning another background crawl if the
// cache was rewritten within this window. Keyed on the cache mtime; overlapping
// crawls are prevented separately, by the refresh lock.
const REFRESH_DEBOUNCE: Duration = Duration::from_secs(3);

const USAGE: &str = "usage: gt <name>   (see: gt --help)";

#[derive(Debug, PartialEq)]
enum Command {
    Version,
    Help,
    Rooted(RootedCommand),
}

// Only these get a search root, so a command that needs one can't be run without it.
#[derive(Debug, PartialEq)]
enum RootedCommand {
    Reindex,
    // Internal: the background re-crawl `jump` spawns. Unlike --reindex, it
    // skips instead of waiting when another crawl is running.
    Refresh,
    List,
    Complete,
    Jump(String),
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    let command = match parse_command(&args) {
        Ok(command) => command,
        Err(msg) => {
            eprintln!("{msg}");
            // The `gt` function captures stdout, so a terminal means gt-bin was run directly.
            if args.is_empty() && io::stdout().is_terminal() {
                eprintln!("{SOURCE_HINT}");
            }
            return ExitCode::FAILURE;
        }
    };

    let command = match command {
        Command::Version => {
            println!("gt {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Command::Help => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Command::Rooted(command) => command,
    };

    let root = match resolve_root() {
        Some(root) => root,
        None => {
            eprintln!("gt: set GOTO_ROOT or HOME");
            return ExitCode::FAILURE;
        }
    };
    if !root.is_dir() {
        eprintln!("gt: {} is not a directory", root.display());
        return ExitCode::FAILURE;
    }

    let scope = Scope {
        root,
        prune: prune_from_env(),
    };
    run(command, &scope)
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    let Some((first, rest)) = args.split_first() else {
        return Err(USAGE.to_string());
    };
    let command = match first.as_str() {
        "--version" | "-v" => Command::Version,
        "--help" | "-h" => Command::Help,
        "--reindex" => Command::Rooted(RootedCommand::Reindex),
        "--refresh" => Command::Rooted(RootedCommand::Refresh),
        "--list" => Command::Rooted(RootedCommand::List),
        "--complete" => Command::Rooted(RootedCommand::Complete),
        "" => return Err(USAGE.to_string()),
        a if a.starts_with('-') => {
            return Err(format!("gt: unknown option '{a}' (see: gt --help)"));
        }
        q => Command::Rooted(RootedCommand::Jump(q.to_lowercase())),
    };
    if let Some(extra) = rest.first() {
        return Err(format!(
            "gt: unexpected argument '{extra}' (see: gt --help)"
        ));
    }
    Ok(command)
}

fn run(command: RootedCommand, scope: &Scope) -> ExitCode {
    match command {
        RootedCommand::Reindex => {
            let _lock = lock_refresh();
            return reindex(scope);
        }
        RootedCommand::Refresh => {
            let _lock = match try_lock_refresh() {
                RefreshLock::Busy => return ExitCode::SUCCESS,
                lock => lock,
            };
            return reindex(scope);
        }
        RootedCommand::List => {
            let (repos, _) = read_cache_or_crawl(scope);
            for line in format_list(&sorted_repos(&repos)) {
                println!("{line}");
            }
        }
        // Off the background-refresh path since it runs on every <TAB>; a cold
        // cache still pays for a full crawl.
        RootedCommand::Complete => {
            let (repos, _) = read_cache_or_crawl(scope);
            for name in completion_names(&repos) {
                println!("{name}");
            }
        }
        RootedCommand::Jump(query) => return jump(scope, &query),
    }
    ExitCode::SUCCESS
}

// Crawl and write the index. Callers take the refresh lock first.
fn reindex(scope: &Scope) -> ExitCode {
    let repos = crawl(scope);
    let Some(path) = cache_path() else {
        eprintln!("gt: nowhere to write the index (set XDG_CACHE_HOME or HOME)");
        return ExitCode::FAILURE;
    };
    if !write_cache_at(&path, scope, &repos) {
        eprintln!("gt: could not write the index to {}", path.display());
        return ExitCode::FAILURE;
    }
    let n = repos.len();
    let noun = if n == 1 { "repo" } else { "repos" };
    eprintln!("gt: indexed {n} {noun} under {}", scope.root.display());
    ExitCode::SUCCESS
}

fn jump(scope: &Scope, query: &str) -> ExitCode {
    let (repos, warm) = read_cache_or_crawl(scope);
    // The cache can name a repo that has since been deleted; the shell must not
    // be handed a path to `cd` into that's gone.
    let matches = match_repos(&repos, query, Path::exists);

    // Refresh the cache in the background so a newly cloned/removed repo is
    // picked up next time. Only from the warm path (the cold path just wrote a
    // fresh cache), and only if the cache isn't already very fresh.
    //
    // Ordering invariant: must stay ahead of the no-match return below, or a
    // newly cloned repo is never indexed — the first `gt` for it always misses.
    if warm && cache_older_than(REFRESH_DEBOUNCE) && !refresh_running() {
        spawn_background_reindex(&scope.root);
    }

    if matches.repos.is_empty() {
        eprintln!("gt: no repo matching '{query}'");
        return ExitCode::FAILURE;
    }
    if matches.kind == MatchKind::Fuzzy {
        match matches.repos.as_slice() {
            [only] => eprintln!(
                "gt: no repo matching '{query}'; using {}",
                display_name(only)
            ),
            _ => eprintln!("gt: no repo matching '{query}'; showing close matches"),
        }
    }

    for p in matches.repos {
        println!("{}", p.display());
    }
    ExitCode::SUCCESS
}

fn format_list(repos: &[&PathBuf]) -> Vec<String> {
    let rows: Vec<(String, String)> = repos
        .iter()
        .map(|p| (display_name(p), p.display().to_string()))
        .collect();
    let width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    rows.iter()
        .map(|(name, path)| format!("{name:<width$}  {path}"))
        .collect()
}

// Tab-completion candidates: repo leaf names (case preserved), sorted and
// deduplicated. Two repos sharing a name (e.g. `skills` in two namespaces)
// collapse to one candidate — the same string can't disambiguate them, so the
// runtime fzf picker handles the final choice.
fn completion_names(repos: &[PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = repos.iter().map(|p| display_name(p)).collect();
    names.sort_by_key(|n| n.to_lowercase());
    // Same key the sort used; plain `dedup()` compares the case-preserved strings.
    names.dedup_by_key(|n| n.to_lowercase());
    names
}

// Display name for a repo row: the directory name (case preserved), falling
// back to the full path for the rootless edge case.
fn display_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

// All known repos, sorted alphabetically by repo name (the leaf), with the full
// path as a tiebreaker so same-named repos stay in a stable order.
fn sorted_repos(repos: &[PathBuf]) -> Vec<&PathBuf> {
    let mut sorted: Vec<&PathBuf> = repos.iter().collect();
    sorted.sort_by(|a, b| match_key(a).cmp(&match_key(b)).then_with(|| a.cmp(b)));
    sorted
}

// True while another process holds the refresh lock. Checked before spawning so
// a burst of calls doesn't fork a refresh per call; the spawned refresh takes
// the lock itself, so losing a race here still never runs two crawls at once.
fn refresh_running() -> bool {
    matches!(try_lock_refresh(), RefreshLock::Busy)
}

// Re-exec ourselves with --refresh, fully detached: no inherited stdio (so the
// shell's $(...) doesn't block on our pipe) and a fresh process group (so job
// control / SIGHUP doesn't reach it). We never wait on it.
fn spawn_background_reindex(root: &Path) {
    let Ok(exe) = env::current_exe() else { return };
    let _ = process::Command::new(exe)
        .arg("--refresh")
        .env("GOTO_ROOT", root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{paths, repos};

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    // ---- parse_command ----

    #[test]
    fn flags_parse_to_their_commands() {
        use RootedCommand::*;
        for (flag, want) in [
            ("--version", Command::Version),
            ("-v", Command::Version),
            ("--help", Command::Help),
            ("-h", Command::Help),
            ("--reindex", Command::Rooted(Reindex)),
            ("--refresh", Command::Rooted(Refresh)),
            ("--list", Command::Rooted(List)),
            ("--complete", Command::Rooted(Complete)),
        ] {
            assert_eq!(parse_command(&args(&[flag])), Ok(want), "{flag}");
        }
    }

    #[test]
    fn a_name_is_a_lowercased_jump() {
        assert_eq!(
            parse_command(&args(&["Neovim"])),
            Ok(Command::Rooted(RootedCommand::Jump("neovim".into())))
        );
    }

    #[test]
    fn extra_argument_is_rejected() {
        for a in [args(&["foo", "bar"]), args(&["--list", "bar"])] {
            let err = parse_command(&a).unwrap_err();
            assert!(
                err.contains("unexpected argument 'bar'"),
                "{a:?} gave {err:?}"
            );
        }
    }

    #[test]
    fn unknown_option_is_rejected() {
        let err = parse_command(&args(&["--lst"])).unwrap_err();
        assert!(err.contains("unknown option '--lst'"), "got {err:?}");
    }

    #[test]
    fn missing_or_empty_query_is_usage() {
        for a in [args(&[]), args(&[""]), args(&["", "bar"])] {
            assert_eq!(parse_command(&a), Err(USAGE.to_string()), "{a:?}");
        }
    }

    // ---- sorted_repos ----

    #[test]
    fn sorted_repos_orders_by_leaf_name() {
        // Sorted by repo name, not path: "alpha" precedes "zulu" even though its
        // parent dir ("z") sorts after zulu's ("a").
        let r = repos(&["/src/a/zulu", "/src/z/alpha"]);
        assert_eq!(paths(&sorted_repos(&r)), ["/src/z/alpha", "/src/a/zulu"]);
    }

    #[test]
    fn sorted_repos_breaks_name_ties_by_path() {
        let r = repos(&["/src/kris/skills", "/src/ai/skills"]);
        assert_eq!(
            paths(&sorted_repos(&r)),
            ["/src/ai/skills", "/src/kris/skills"]
        );
    }

    // ---- format_list ----

    #[test]
    fn list_pads_names_so_paths_align() {
        let r = repos(&["/src/acme/ansible", "/src/a/neovim"]);
        let sorted = sorted_repos(&r);
        assert_eq!(
            format_list(&sorted),
            ["ansible  /src/acme/ansible", "neovim   /src/a/neovim",]
        );
    }

    #[test]
    fn list_preserves_name_case() {
        let r = repos(&["/src/a/Neovim"]);
        assert_eq!(format_list(&sorted_repos(&r)), ["Neovim  /src/a/Neovim"]);
    }

    #[test]
    fn list_of_no_repos_is_empty() {
        assert!(format_list(&[]).is_empty());
    }

    // ---- completion_names ----

    #[test]
    fn completion_names_are_sorted_leaf_names() {
        let r = repos(&["/src/z/neovim", "/src/a/ansible"]);
        assert_eq!(completion_names(&r), ["ansible", "neovim"]);
    }

    #[test]
    fn completion_names_dedup_same_named_repos() {
        // `skills` in two namespaces collapses to a single candidate.
        let r = repos(&["/src/kris/skills", "/src/ai/skills"]);
        assert_eq!(completion_names(&r), ["skills"]);
    }

    #[test]
    fn completion_names_dedup_ignores_case() {
        // Three repos, two distinct casings: still one candidate. The third entry
        // also catches byte-identical duplicates landing non-adjacent.
        let r = repos(&["/src/a/skills", "/src/b/Skills", "/src/c/skills"]);
        assert_eq!(completion_names(&r), ["skills"]);
    }

    #[test]
    fn completion_names_preserve_case() {
        let r = repos(&["/src/a/Neovim"]);
        assert_eq!(completion_names(&r), ["Neovim"]);
    }
}
