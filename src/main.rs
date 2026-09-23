use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{self, ExitCode, Stdio};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use ignore::{WalkBuilder, WalkState};

// Directories the crawl never descends into: the built-in noise, plus any names
// the user appends via $GOTO_EXTRA_PRUNE. The crawl walks hidden entries,
// so `.git` here prevents it from being walked.
const DEFAULT_PRUNE: &[&str] = &["node_modules", ".terraform", ".git"];

const HELP: &str = "\
gt — jump to any git repo under your source root by its directory name.

Usage:
  gt <name>       Jump to the repo whose dir name matches <name> (exact match,
                  else substring; an fzf picker opens if several match).
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
// cache was rewritten within this window. (This is a post-refresh cooldown keyed
// on the cache mtime, not a concurrency guard — a burst of calls against an old
// cache can still spawn overlapping crawls, which is cheap enough not to matter.)
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

    run(command, &root)
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    let Some((first, rest)) = args.split_first() else {
        return Err(USAGE.to_string());
    };
    let command = match first.as_str() {
        "--version" | "-v" => Command::Version,
        "--help" | "-h" => Command::Help,
        "--reindex" => Command::Rooted(RootedCommand::Reindex),
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

fn run(command: RootedCommand, root: &Path) -> ExitCode {
    match command {
        RootedCommand::Reindex => {
            let n = crawl_and_cache(root).len();
            eprintln!("gt: indexed {n} repos under {}", root.display());
        }
        RootedCommand::List => {
            let (repos, _) = read_cache_or_crawl(root);
            for line in format_list(&sorted_repos(&repos)) {
                println!("{line}");
            }
        }
        // Off the background-refresh path since it runs on every <TAB>; a cold
        // cache still pays for a full crawl.
        RootedCommand::Complete => {
            let (repos, _) = read_cache_or_crawl(root);
            for name in completion_names(&repos) {
                println!("{name}");
            }
        }
        RootedCommand::Jump(query) => return jump(root, &query),
    }
    ExitCode::SUCCESS
}

fn jump(root: &Path, query: &str) -> ExitCode {
    let (repos, warm) = read_cache_or_crawl(root);
    // The cache can name a repo that has since been deleted; the shell must not
    // be handed a path to `cd` into that's gone.
    let matches = match_repos(&repos, query, Path::exists);

    // Refresh the cache in the background so a newly cloned/removed repo is
    // picked up next time. Only from the warm path (the cold path just wrote a
    // fresh cache), and only if the cache isn't already very fresh.
    //
    // Ordering invariant: must stay ahead of the no-match return below, or a
    // newly cloned repo is never indexed — the first `gt` for it always misses.
    if warm && cache_older_than(REFRESH_DEBOUNCE) {
        spawn_background_reindex(root);
    }

    if matches.is_empty() {
        eprintln!("gt: no repo matching '{query}'");
        return ExitCode::FAILURE;
    }

    for p in matches {
        println!("{}", p.display());
    }
    ExitCode::SUCCESS
}

// Exact name match, else substring. `keep` runs per tier and only on name
// matches, so a rejected exact match falls through to the substring tier.
fn match_repos<'a>(
    repos: &'a [PathBuf],
    query: &str,
    keep: impl Fn(&Path) -> bool,
) -> Vec<&'a PathBuf> {
    let tier = |hit: &dyn Fn(&str) -> bool| {
        let mut found: Vec<&PathBuf> = repos
            .iter()
            .filter(|p| match_key(p).is_some_and(|k| hit(&k)) && keep(p))
            .collect();
        found.sort();
        found
    };
    let exact = tier(&|k| k == query);
    if exact.is_empty() {
        tier(&|k| k.contains(query))
    } else {
        exact
    }
}

// Render the `--list` table: two columns, repo name then full path.
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

// Absolute because the cache stores the root: a relative one would match any
// directory that happens to have the same relative path.
fn resolve_root() -> Option<PathBuf> {
    let root = resolve_root_from(env::var_os("GOTO_ROOT"), env::var_os("HOME"))?;
    std::path::absolute(root).ok()
}

// An empty value is treated as unset: joining onto "" yields a relative path.
fn resolve_root_from(goto_root: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    if let Some(val) = goto_root {
        if !val.is_empty() {
            return Some(expand_tilde_with(PathBuf::from(val), home.as_deref()));
        }
    }
    home.filter(|home| !home.is_empty())
        .map(|home| PathBuf::from(home).join("src"))
}

fn expand_tilde_with(path: PathBuf, home: Option<&OsStr>) -> PathBuf {
    let Some(home) = home.filter(|home| !home.is_empty()) else {
        return path;
    };
    match path.strip_prefix("~") {
        Ok(rest) => PathBuf::from(home).join(rest),
        Err(_) => path,
    }
}

// The leaf name lowercased, for matching and sorting; see `display_name` for output.
fn match_key(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase())
}

// ---- cache ----

fn cache_path() -> Option<PathBuf> {
    cache_path_from(env::var_os("XDG_CACHE_HOME"), env::var_os("HOME"))
}

// One cache file for every root; the recorded root line distinguishes them.
fn cache_path_from(xdg: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let dir = if let Some(xdg) = xdg.filter(|v| !v.is_empty()) {
        PathBuf::from(xdg)
    } else {
        PathBuf::from(home.filter(|v| !v.is_empty())?).join(".cache")
    };
    Some(dir.join("goto").join("index"))
}

// Cache format: line 1 is the root the cache was built for, remaining lines are
// repo paths. Splitting the (de)serialization out keeps it pure and testable.
fn serialize_cache(root: &Path, repos: &[PathBuf]) -> String {
    let mut body = String::new();
    body.push_str(&root.display().to_string());
    body.push('\n');
    for r in repos {
        let line = r.display().to_string();
        // Can't round-trip in a line-delimited format: one repo would read as two.
        if line.contains('\n') {
            continue;
        }
        body.push_str(&line);
        body.push('\n');
    }
    body
}

// Parse cached contents, returning the repo list only if the recorded root
// matches `root`. Any mismatch (or empty input) → None. Only the root line is
// validated.
fn parse_cache(contents: &str, root: &Path) -> Option<Vec<PathBuf>> {
    let mut lines = contents.lines();
    let stored_root = lines.next()?;
    if Path::new(stored_root) != root {
        return None;
    }
    Some(lines.filter(|l| !l.is_empty()).map(PathBuf::from).collect())
}

// Returns the cached repo list only if the cache exists and was built for `root`.
fn read_cache(root: &Path) -> Option<Vec<PathBuf>> {
    read_cache_at(&cache_path()?, root)
}

fn read_cache_at(path: &Path, root: &Path) -> Option<Vec<PathBuf>> {
    let contents = fs::read_to_string(path).ok()?;
    parse_cache(&contents, root)
}

fn write_cache(root: &Path, repos: &[PathBuf]) {
    if let Some(path) = cache_path() {
        write_cache_at(&path, root, repos);
    }
}

fn write_cache_at(path: &Path, root: &Path, repos: &[PathBuf]) {
    let Some(parent) = path.parent() else { return };
    let Some(name) = path.file_name() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }

    let body = serialize_cache(root, repos);

    // Write to a per-process temp file, then atomically rename into place so a
    // concurrent reader never sees a partial file.
    let tmp = path.with_file_name(format!(
        "{}.tmp.{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let installed = fs::File::create(&tmp)
        .is_ok_and(|mut f| f.write_all(body.as_bytes()).is_ok() && fs::rename(&tmp, path).is_ok());
    if !installed {
        let _ = fs::remove_file(&tmp);
    }
}

fn cache_older_than(age: Duration) -> bool {
    let Some(path) = cache_path() else {
        return true;
    };
    let Ok(modified) = fs::metadata(&path).and_then(|m| m.modified()) else {
        return true;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|d| d > age)
        .unwrap_or(true)
}

// Re-exec ourselves with --reindex, fully detached: no inherited stdio (so the
// shell's $(...) doesn't block on our pipe) and a fresh process group (so job
// control / SIGHUP doesn't reach it). We never wait on it.
fn spawn_background_reindex(root: &Path) {
    let Ok(exe) = env::current_exe() else { return };
    let _ = process::Command::new(exe)
        .arg("--reindex")
        .env("GOTO_ROOT", root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
}

// Crawl the tree and persist the result, returning the discovered repos.
fn crawl_and_cache(root: &Path) -> Vec<PathBuf> {
    let repos = discover_repos(root, prune_set(env::var_os("GOTO_EXTRA_PRUNE")));
    write_cache(root, &repos);
    repos
}

// Cached repos if the cache is warm and built for this root, else a live crawl
// that writes it. False means cold: the caller must not then refresh.
fn read_cache_or_crawl(root: &Path) -> (Vec<PathBuf>, bool) {
    match read_cache(root) {
        Some(repos) => (repos, true),
        None => (crawl_and_cache(root), false),
    }
}

// The set of directory names to prune: the built-in defaults plus any the user
// appends via $GOTO_EXTRA_PRUNE (comma-separated; whitespace trimmed, empties
// dropped).
fn prune_set(extra: Option<OsString>) -> HashSet<String> {
    let mut set: HashSet<String> = DEFAULT_PRUNE.iter().map(|s| s.to_string()).collect();
    if let Some(extra) = extra {
        for name in extra.to_string_lossy().split(',') {
            let name = name.trim();
            if !name.is_empty() {
                set.insert(name.to_string());
            }
        }
    }
    set
}

fn discover_repos(root: &Path, prune: HashSet<String>) -> Vec<PathBuf> {
    let found = Mutex::new(Vec::new());

    WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .filter_entry(move |entry| {
            // Prune noisy/irrelevant dirs; never descend into them. A name that
            // isn't valid UTF-8 can't be matched, so it's kept rather than pruned.
            entry
                .file_name()
                .to_str()
                .is_none_or(|n| !prune.contains(n))
        })
        .build_parallel()
        .run(|| {
            let found = &found;
            Box::new(move |result| {
                if let Ok(entry) = result {
                    let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
                    if is_dir {
                        let path = entry.path();
                        // A repo is any dir containing a `.git` entry (dir or file).
                        if path.join(".git").exists() {
                            found.lock().unwrap().push(path.to_path_buf());
                        }
                    }
                }
                WalkState::Continue
            })
        });

    found.into_inner().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn repos(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    // A fresh directory per test, so the filesystem cases can run in parallel.
    fn temp_base(name: &str) -> PathBuf {
        let base = env::temp_dir().join(format!("goto-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn build_probe_tree(base: &Path) -> PathBuf {
        let root = base.join("root");
        for p in [
            "normal-repo/.git",
            "node_modules/pruned-repo/.git",
            "vendor/vendored-repo/.git",
            "deep/a/b/c/d/e/f/g/deep-repo/.git",
            ".hidden-dir/hidden-repo/.git",
            "spaces in name/.git",
        ] {
            fs::create_dir_all(root.join(p)).unwrap();
        }

        // `.git` as a file, i.e. a worktree or submodule.
        fs::create_dir_all(root.join("worktree-repo")).unwrap();
        fs::write(root.join("worktree-repo/.git"), "gitdir: /elsewhere\n").unwrap();

        fs::create_dir_all(base.join("elsewhere/linked-repo/.git")).unwrap();
        symlink(base.join("elsewhere/linked-repo"), root.join("linked-repo")).unwrap();

        root
    }

    fn discovered_names(root: &Path, extra_prune: Option<&str>) -> Vec<String> {
        let prune = prune_set(extra_prune.map(OsString::from));
        let mut names: Vec<String> = discover_repos(root, prune)
            .iter()
            .map(|p| display_name(p))
            .collect();
        names.sort();
        names
    }

    fn names(found: &[&PathBuf]) -> Vec<String> {
        found.iter().map(|p| p.display().to_string()).collect()
    }

    fn matched<'a>(r: &'a [PathBuf], query: &str) -> Vec<&'a PathBuf> {
        match_repos(r, query, |_| true)
    }

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
            ("--list", Command::Rooted(List)),
            ("--complete", Command::Rooted(Complete)),
        ] {
            assert_eq!(parse_command(&args(&[flag])), Ok(want), "{flag}");
        }
    }

    #[test]
    fn a_name_is_a_lowercased_jump() {
        assert_eq!(
            parse_command(&args(&["Nitro"])),
            Ok(Command::Rooted(RootedCommand::Jump("nitro".into())))
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

    // ---- match_repos ----

    #[test]
    fn deleted_repos_are_not_jump_targets() {
        let base = temp_base("existing");
        fs::create_dir_all(base.join("a/nitro")).unwrap();
        let r = vec![base.join("a/nitro"), base.join("b/nitro")];
        assert_eq!(
            match_repos(&r, "nitro", Path::exists),
            [&base.join("a/nitro")]
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn deleted_exact_match_falls_back_to_substring() {
        let base = temp_base("fallback");
        fs::create_dir_all(base.join("b/nitro-tools")).unwrap();
        let r = vec![base.join("a/nitro"), base.join("b/nitro-tools")];
        assert_eq!(
            match_repos(&r, "nitro", Path::exists),
            [&base.join("b/nitro-tools")]
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn exact_basename_match() {
        let r = repos(&["/src/a/nitro", "/src/b/other"]);
        assert_eq!(names(&matched(&r, "nitro")), ["/src/a/nitro"]);
    }

    #[test]
    fn match_is_case_insensitive() {
        let r = repos(&["/src/a/Nitro"]);
        assert_eq!(names(&matched(&r, "nitro")), ["/src/a/Nitro"]);
    }

    #[test]
    fn falls_back_to_substring_when_no_exact_match() {
        let r = repos(&["/src/developers/hw-admin", "/src/x/unrelated"]);
        assert_eq!(names(&matched(&r, "adm")), ["/src/developers/hw-admin"]);
    }

    #[test]
    fn exact_match_wins_over_substring() {
        // "skills" is both an exact name and a substring of "skills-extra";
        // only the exact match should be returned.
        let r = repos(&["/src/a/skills", "/src/b/skills-extra"]);
        assert_eq!(names(&matched(&r, "skills")), ["/src/a/skills"]);
    }

    #[test]
    fn ambiguous_exact_matches_return_all_sorted() {
        let r = repos(&["/src/kris/skills", "/src/ai/skills"]);
        assert_eq!(
            names(&matched(&r, "skills")),
            ["/src/ai/skills", "/src/kris/skills"]
        );
    }

    #[test]
    fn no_match_returns_empty() {
        let r = repos(&["/src/a/nitro"]);
        assert!(matched(&r, "zzz").is_empty());
    }

    #[test]
    fn matches_deeply_nested_repo() {
        let r = repos(&["/src/devops/terraform/modules/aws-redis"]);
        assert_eq!(
            names(&matched(&r, "aws-redis")),
            ["/src/devops/terraform/modules/aws-redis"]
        );
    }

    // ---- sorted_repos ----

    #[test]
    fn sorted_repos_orders_by_leaf_name() {
        // Sorted by repo name, not path: "alpha" precedes "zulu" even though its
        // parent dir ("z") sorts after zulu's ("a").
        let r = repos(&["/src/a/zulu", "/src/z/alpha"]);
        assert_eq!(names(&sorted_repos(&r)), ["/src/z/alpha", "/src/a/zulu"]);
    }

    #[test]
    fn sorted_repos_breaks_name_ties_by_path() {
        let r = repos(&["/src/kris/skills", "/src/ai/skills"]);
        assert_eq!(
            names(&sorted_repos(&r)),
            ["/src/ai/skills", "/src/kris/skills"]
        );
    }

    // ---- format_list ----

    #[test]
    fn list_pads_names_so_paths_align() {
        let r = repos(&["/src/devops/ansible", "/src/a/nitro"]);
        let sorted = sorted_repos(&r);
        assert_eq!(
            format_list(&sorted),
            ["ansible  /src/devops/ansible", "nitro    /src/a/nitro",]
        );
    }

    #[test]
    fn list_preserves_name_case() {
        let r = repos(&["/src/a/Nitro"]);
        assert_eq!(format_list(&sorted_repos(&r)), ["Nitro  /src/a/Nitro"]);
    }

    #[test]
    fn list_of_no_repos_is_empty() {
        assert!(format_list(&[]).is_empty());
    }

    // ---- completion_names ----

    #[test]
    fn completion_names_are_sorted_leaf_names() {
        let r = repos(&["/src/z/nitro", "/src/a/ansible"]);
        assert_eq!(completion_names(&r), ["ansible", "nitro"]);
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
        let r = repos(&["/src/a/Nitro"]);
        assert_eq!(completion_names(&r), ["Nitro"]);
    }

    // ---- match_key ----

    #[test]
    fn match_key_lowercases_leaf() {
        assert_eq!(match_key(Path::new("/a/B/Nitro")), Some("nitro".into()));
    }

    // ---- expand_tilde_with ----

    #[test]
    fn tilde_expands_to_home() {
        let got = expand_tilde_with(PathBuf::from("~/code"), Some(OsStr::new("/home/kris")));
        assert_eq!(got, PathBuf::from("/home/kris/code"));
    }

    #[test]
    fn bare_tilde_expands_to_home() {
        let got = expand_tilde_with(PathBuf::from("~"), Some(OsStr::new("/home/kris")));
        assert_eq!(got, PathBuf::from("/home/kris"));
    }

    #[test]
    fn absolute_path_is_left_alone() {
        let got = expand_tilde_with(PathBuf::from("/etc/foo"), Some(OsStr::new("/home/kris")));
        assert_eq!(got, PathBuf::from("/etc/foo"));
    }

    #[test]
    fn tilde_without_home_is_left_alone() {
        let got = expand_tilde_with(PathBuf::from("~/code"), None);
        assert_eq!(got, PathBuf::from("~/code"));
    }

    // ---- resolve_root_from ----

    #[test]
    fn root_defaults_to_home_src() {
        let got = resolve_root_from(None, Some(OsString::from("/home/kris")));
        assert_eq!(got, Some(PathBuf::from("/home/kris/src")));
    }

    #[test]
    fn goto_root_overrides_default_and_expands_tilde() {
        let got = resolve_root_from(
            Some(OsString::from("~/code")),
            Some(OsString::from("/home/kris")),
        );
        assert_eq!(got, Some(PathBuf::from("/home/kris/code")));
    }

    #[test]
    fn empty_goto_root_falls_back_to_default() {
        let got = resolve_root_from(Some(OsString::new()), Some(OsString::from("/home/kris")));
        assert_eq!(got, Some(PathBuf::from("/home/kris/src")));
    }

    #[test]
    fn no_home_and_no_goto_root_is_none() {
        assert_eq!(resolve_root_from(None, None), None);
    }

    #[test]
    fn empty_home_is_treated_as_unset() {
        // Not Some("src"): joining onto "" would give a relative root, pointing
        // the crawl at whatever ./src happens to be in $PWD.
        assert_eq!(resolve_root_from(None, Some(OsString::new())), None);
        assert_eq!(
            resolve_root_from(Some(OsString::new()), Some(OsString::new())),
            None
        );
    }

    #[test]
    fn tilde_with_empty_home_is_left_alone() {
        let got = expand_tilde_with(PathBuf::from("~/code"), Some(OsStr::new("")));
        assert_eq!(got, PathBuf::from("~/code"));
    }

    // ---- prune_set ----

    #[test]
    fn prune_defaults_present_without_env() {
        let set = prune_set(None);
        assert!(set.contains("node_modules"));
        assert!(set.contains(".terraform"));
        assert!(set.contains(".git"));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn prune_appends_extra_dirs_to_defaults() {
        let set = prune_set(Some(OsString::from("vendor,dist")));
        assert!(set.contains("vendor"));
        assert!(set.contains("dist"));
        // Defaults are still pruned alongside the extras.
        assert!(set.contains("node_modules"));
        assert_eq!(set.len(), 5);
    }

    #[test]
    fn prune_trims_whitespace_and_drops_empties() {
        let set = prune_set(Some(OsString::from(" vendor , , dist ,")));
        assert!(set.contains("vendor"));
        assert!(set.contains("dist"));
        assert!(!set.contains(""));
        assert_eq!(set.len(), 5); // 3 defaults + vendor + dist
    }

    #[test]
    fn prune_empty_env_adds_nothing() {
        assert_eq!(prune_set(Some(OsString::new())).len(), 3);
    }

    // ---- cache serialization ----

    #[test]
    fn cache_round_trips() {
        let root = Path::new("/home/kris/src");
        let r = repos(&["/home/kris/src/a/nitro", "/home/kris/src/b/hw-admin"]);
        let serialized = serialize_cache(root, &r);
        assert_eq!(parse_cache(&serialized, root), Some(r));
    }

    #[test]
    fn cache_rejected_when_root_differs() {
        let serialized = serialize_cache(Path::new("/home/kris/src"), &repos(&["/x/y"]));
        assert_eq!(parse_cache(&serialized, Path::new("/other/root")), None);
    }

    #[test]
    fn empty_cache_parses_to_no_repos() {
        let serialized = serialize_cache(Path::new("/home/kris/src"), &[]);
        assert_eq!(
            parse_cache(&serialized, Path::new("/home/kris/src")),
            Some(vec![])
        );
    }

    #[test]
    fn empty_contents_is_none() {
        assert_eq!(parse_cache("", Path::new("/home/kris/src")), None);
    }

    #[test]
    fn paths_containing_a_newline_are_not_cached() {
        // The line-delimited format can't round-trip them: `bad\nname` would read
        // back as `bad` plus a bogus relative `name`.
        let root = Path::new("/home/kris/src");
        let r = repos(&["/home/kris/src/bad\nname", "/home/kris/src/ok"]);
        let serialized = serialize_cache(root, &r);
        assert_eq!(
            parse_cache(&serialized, root),
            Some(repos(&["/home/kris/src/ok"]))
        );
    }

    // ---- discover_repos ----

    #[test]
    fn discover_repos_finds_repos_and_prunes_noise() {
        let base = temp_base("discover");
        let root = build_probe_tree(&base);
        assert_eq!(
            discovered_names(&root, None),
            [
                "deep-repo",
                "hidden-repo",
                "normal-repo",
                "spaces in name",
                "vendored-repo",
                "worktree-repo",
            ]
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn discover_repos_skips_symlinked_repos() {
        let base = temp_base("symlink");
        let root = build_probe_tree(&base);
        let names = discovered_names(&root, None);
        assert!(root.join("linked-repo/.git").exists());
        assert!(!names.contains(&"linked-repo".to_string()), "got {names:?}");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn discover_repos_honours_extra_prune() {
        let base = temp_base("extra-prune");
        let root = build_probe_tree(&base);
        let names = discovered_names(&root, Some("vendor"));
        assert!(
            !names.contains(&"vendored-repo".to_string()),
            "got {names:?}"
        );
        assert!(names.contains(&"normal-repo".to_string()), "got {names:?}");
        let _ = fs::remove_dir_all(&base);
    }

    // ---- cache_path_from ----

    #[test]
    fn cache_path_prefers_xdg() {
        let got = cache_path_from(Some(OsString::from("/x")), Some(OsString::from("/h")));
        assert_eq!(got, Some(PathBuf::from("/x/goto/index")));
    }

    #[test]
    fn cache_path_falls_back_to_home_dot_cache() {
        let got = cache_path_from(None, Some(OsString::from("/h")));
        assert_eq!(got, Some(PathBuf::from("/h/.cache/goto/index")));
    }

    #[test]
    fn cache_path_treats_empty_values_as_unset() {
        let got = cache_path_from(Some(OsString::new()), Some(OsString::from("/h")));
        assert_eq!(got, Some(PathBuf::from("/h/.cache/goto/index")));
        assert_eq!(cache_path_from(None, Some(OsString::new())), None);
    }

    #[test]
    fn cache_path_without_xdg_or_home_is_none() {
        assert_eq!(cache_path_from(None, None), None);
    }

    // ---- cache file I/O ----

    #[test]
    fn cache_round_trips_through_the_filesystem() {
        let base = temp_base("cache-rt");
        let path = base.join("goto").join("index");
        let root = Path::new("/home/kris/src");
        let r = repos(&["/home/kris/src/a/nitro", "/home/kris/src/b/hw-admin"]);

        write_cache_at(&path, root, &r);
        assert!(
            path.exists(),
            "write_cache_at should create missing parents"
        );
        assert_eq!(read_cache_at(&path, root), Some(r));
        assert_eq!(read_cache_at(&path, Path::new("/other/root")), None);

        let leftovers: Vec<String> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "left {leftovers:?} behind");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn reading_a_missing_cache_file_is_none() {
        let base = temp_base("cache-missing");
        assert_eq!(read_cache_at(&base.join("absent"), Path::new("/r")), None);
        let _ = fs::remove_dir_all(&base);
    }
}
