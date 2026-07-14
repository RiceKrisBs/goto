use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use ignore::{WalkBuilder, WalkState};

const PRUNE: &[&str] = &["node_modules", ".terraform", ".git"];

// Cooldown after a refresh lands: skip spawning another background crawl if the
// cache was rewritten within this window. (This is a post-refresh cooldown keyed
// on the cache mtime, not a concurrency guard — a burst of calls against an old
// cache can still spawn overlapping crawls, which is cheap enough not to matter.)
const REFRESH_DEBOUNCE: Duration = Duration::from_secs(3);

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

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

    // `--reindex` is a subcommand, not a repo name: it shares the first-arg slot
    // but forces a synchronous rebuild and exits. Used by the background refresh
    // and by `gt --reindex`.
    if args.first().map(|a| a == "--reindex").unwrap_or(false) {
        let n = crawl_and_cache(&root).len();
        eprintln!("gt: indexed {n} repos under {}", root.display());
        return ExitCode::SUCCESS;
    }

    // `--list` prints every repo the cli is aware of, sorted alphabetically, and
    // exits. Uses the cache when warm, crawling live otherwise.
    if args.first().map(|a| a == "--list").unwrap_or(false) {
        let repos = read_cache(&root).unwrap_or_else(|| crawl_and_cache(&root));
        for line in format_list(&sorted_repos(&repos)) {
            println!("{line}");
        }
        return ExitCode::SUCCESS;
    }

    let query = match args.first() {
        Some(q) if !q.is_empty() => q.to_lowercase(),
        _ => {
            eprintln!("usage: gt <name>");
            return ExitCode::FAILURE;
        }
    };

    // Warm path: use the cache if present and built for this root. Cold path:
    // crawl live and write the cache so the next call is fast.
    let (repos, warm) = match read_cache(&root) {
        Some(repos) => (repos, true),
        None => (crawl_and_cache(&root), false),
    };

    // Filter matches by existence: the cache can name a repo that has since been
    // deleted, and we must not hand the shell a path to `cd` into that's gone.
    // The background refresh drops it from the index for next time.
    let matches: Vec<&PathBuf> = match_repos(&repos, &query)
        .into_iter()
        .filter(|p| p.exists())
        .collect();

    // Refresh the cache in the background so a newly cloned/removed repo is
    // picked up next time. Only from the warm path (the cold path just wrote a
    // fresh cache), and only if the cache isn't already very fresh.
    if warm && cache_older_than(REFRESH_DEBOUNCE) {
        spawn_background_reindex(&root);
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

// Exact basename match (case-insensitive), then substring fallback.
fn match_repos<'a>(repos: &'a [PathBuf], query: &str) -> Vec<&'a PathBuf> {
    let mut exact: Vec<&PathBuf> = repos
        .iter()
        .filter(|p| basename(p).map(|b| b == query).unwrap_or(false))
        .collect();
    exact.sort();
    if !exact.is_empty() {
        return exact;
    }

    let mut fuzzy: Vec<&PathBuf> = repos
        .iter()
        .filter(|p| basename(p).map(|b| b.contains(query)).unwrap_or(false))
        .collect();
    fuzzy.sort();
    fuzzy
}

// Render the `--list` table: two columns, repo name then full path, with the
// name column padded to the widest name so the paths line up.
fn format_list(repos: &[&PathBuf]) -> Vec<String> {
    let rows: Vec<(String, String)> = repos
        .iter()
        .map(|p| (list_name(p), p.display().to_string()))
        .collect();
    let width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    rows.iter()
        .map(|(name, path)| format!("{name:<width$}  {path}"))
        .collect()
}

// Display name for a repo row: the directory name (case preserved), falling
// back to the full path for the rootless edge case.
fn list_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

// All known repos, sorted alphabetically by repo name (the leaf), with the full
// path as a tiebreaker so same-named repos stay in a stable order.
fn sorted_repos(repos: &[PathBuf]) -> Vec<&PathBuf> {
    let mut sorted: Vec<&PathBuf> = repos.iter().collect();
    sorted.sort_by(|a, b| basename(a).cmp(&basename(b)).then_with(|| a.cmp(b)));
    sorted
}

// Search root: $GOTO_ROOT if set (with a leading `~` expanded), else ~/src.
fn resolve_root() -> Option<PathBuf> {
    resolve_root_from(env::var_os("GOTO_ROOT"), env::var_os("HOME"))
}

fn resolve_root_from(goto_root: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    if let Some(val) = goto_root {
        if !val.is_empty() {
            return Some(expand_tilde_with(PathBuf::from(val), home.as_deref()));
        }
    }
    home.map(|home| PathBuf::from(home).join("src"))
}

fn expand_tilde_with(path: PathBuf, home: Option<&OsStr>) -> PathBuf {
    let Some(home) = home else {
        return path;
    };
    match path.strip_prefix("~") {
        Ok(rest) => PathBuf::from(home).join(rest),
        Err(_) => path,
    }
}

fn basename(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase())
}

// ---- cache ----

fn cache_path() -> Option<PathBuf> {
    let dir = if let Some(xdg) = env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(xdg)
    } else {
        PathBuf::from(env::var_os("HOME")?).join(".cache")
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
        body.push_str(&r.display().to_string());
        body.push('\n');
    }
    body
}

// Parse cached contents, returning the repo list only if the recorded root
// matches `root`. Any mismatch (or empty input) → None.
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
    let contents = fs::read_to_string(cache_path()?).ok()?;
    parse_cache(&contents, root)
}

fn write_cache(root: &Path, repos: &[PathBuf]) {
    let Some(path) = cache_path() else { return };
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }

    let body = serialize_cache(root, repos);

    // Write to a per-process temp file, then atomically rename into place so a
    // concurrent reader never sees a partial file.
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if let Ok(mut f) = fs::File::create(&tmp) {
        if f.write_all(body.as_bytes()).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
    let _ = fs::remove_file(&tmp);
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
    let _ = Command::new(exe)
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
    let repos = discover_repos(root);
    write_cache(root, &repos);
    repos
}

fn discover_repos(root: &Path) -> Vec<PathBuf> {
    let found = Mutex::new(Vec::new());

    WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .filter_entry(|entry| {
            // Prune noisy/irrelevant dirs; never descend into them.
            entry
                .file_name()
                .to_str()
                .map(|n| !PRUNE.contains(&n))
                .unwrap_or(true)
        })
        .build_parallel()
        .run(|| {
            let found = &found;
            Box::new(move |result| {
                if let Ok(entry) = result {
                    let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
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

    fn repos(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    fn names(matched: &[&PathBuf]) -> Vec<String> {
        matched.iter().map(|p| p.display().to_string()).collect()
    }

    // ---- match_repos ----

    #[test]
    fn exact_basename_match() {
        let r = repos(&["/src/a/nitro", "/src/b/other"]);
        assert_eq!(names(&match_repos(&r, "nitro")), ["/src/a/nitro"]);
    }

    #[test]
    fn match_is_case_insensitive() {
        let r = repos(&["/src/a/Nitro"]);
        assert_eq!(names(&match_repos(&r, "nitro")), ["/src/a/Nitro"]);
    }

    #[test]
    fn falls_back_to_substring_when_no_exact_match() {
        let r = repos(&["/src/developers/hw-admin", "/src/x/unrelated"]);
        assert_eq!(names(&match_repos(&r, "adm")), ["/src/developers/hw-admin"]);
    }

    #[test]
    fn exact_match_wins_over_substring() {
        // "skills" is both an exact name and a substring of "skills-extra";
        // only the exact match should be returned.
        let r = repos(&["/src/a/skills", "/src/b/skills-extra"]);
        assert_eq!(names(&match_repos(&r, "skills")), ["/src/a/skills"]);
    }

    #[test]
    fn ambiguous_exact_matches_return_all_sorted() {
        let r = repos(&["/src/kris/skills", "/src/ai/skills"]);
        assert_eq!(
            names(&match_repos(&r, "skills")),
            ["/src/ai/skills", "/src/kris/skills"]
        );
    }

    #[test]
    fn no_match_returns_empty() {
        let r = repos(&["/src/a/nitro"]);
        assert!(match_repos(&r, "zzz").is_empty());
    }

    #[test]
    fn matches_deeply_nested_repo() {
        let r = repos(&["/src/devops/terraform/modules/aws-redis"]);
        assert_eq!(
            names(&match_repos(&r, "aws-redis")),
            ["/src/devops/terraform/modules/aws-redis"]
        );
    }

    // ---- sorted_repos ----

    #[test]
    fn sorted_repos_orders_by_leaf_name() {
        // Sorted by repo name, not path: "alpha" precedes "zeta" even though its
        // parent dir ("z") sorts after zeta's ("a").
        let r = repos(&["/src/a/zeta", "/src/z/alpha"]);
        assert_eq!(names(&sorted_repos(&r)), ["/src/z/alpha", "/src/a/zeta"]);
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
            [
                "ansible  /src/devops/ansible",
                "nitro    /src/a/nitro",
            ]
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

    // ---- basename ----

    #[test]
    fn basename_lowercases_leaf() {
        assert_eq!(basename(Path::new("/a/B/Nitro")), Some("nitro".into()));
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
        assert_eq!(parse_cache(&serialized, Path::new("/home/kris/src")), Some(vec![]));
    }

    #[test]
    fn empty_contents_is_none() {
        assert_eq!(parse_cache("", Path::new("/home/kris/src")), None);
    }
}
