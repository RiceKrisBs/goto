use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::crawl::crawl;
use crate::scope::Scope;

pub fn cache_path() -> Option<PathBuf> {
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

// Cache format: line 1 is the root the cache was built for, line 2 the prune
// set it was crawled with, remaining lines are repo paths.
fn serialize_cache(scope: &Scope, repos: &[PathBuf]) -> String {
    let mut body = String::new();
    body.push_str(&scope.root.display().to_string());
    body.push('\n');
    body.push_str(&scope.prune_key());
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

// Parse cached contents, returning the repo list only if the recorded root and
// prune set match. Any mismatch (or empty input) → None. Only the header lines
// are validated.
fn parse_cache(contents: &str, scope: &Scope) -> Option<Vec<PathBuf>> {
    let mut lines = contents.lines();
    let stored_root = lines.next()?;
    if Path::new(stored_root) != scope.root {
        return None;
    }
    if lines.next()? != scope.prune_key() {
        return None;
    }
    Some(lines.filter(|l| !l.is_empty()).map(PathBuf::from).collect())
}

fn read_cache(scope: &Scope) -> Option<Vec<PathBuf>> {
    read_cache_at(&cache_path()?, scope)
}

fn read_cache_at(path: &Path, scope: &Scope) -> Option<Vec<PathBuf>> {
    let contents = fs::read_to_string(path).ok()?;
    parse_cache(&contents, scope)
}

fn write_cache(scope: &Scope, repos: &[PathBuf]) {
    if let Some(path) = cache_path() {
        write_cache_at(&path, scope, repos);
    }
}

pub fn write_cache_at(path: &Path, scope: &Scope, repos: &[PathBuf]) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Some(name) = path.file_name() else {
        return false;
    };
    if fs::create_dir_all(parent).is_err() {
        return false;
    }

    let body = serialize_cache(scope, repos);

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
    installed
}

pub fn cache_older_than(age: Duration) -> bool {
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

fn crawl_and_cache(scope: &Scope) -> Vec<PathBuf> {
    let repos = crawl(scope);
    write_cache(scope, &repos);
    repos
}

// False means cold: the crawl just wrote a fresh cache, so the caller must not
// then refresh.
pub fn read_cache_or_crawl(scope: &Scope) -> (Vec<PathBuf>, bool) {
    if let Some(repos) = read_cache(scope) {
        return (repos, true);
    }
    // Wait out any crawl already running, then check again: it may have just
    // written the index we need. A burst of cold calls crawls once, not once each.
    let _lock = lock_refresh();
    match read_cache(scope) {
        Some(repos) => (repos, false),
        None => (crawl_and_cache(scope), false),
    }
}

// Held for the length of a crawl, so crawls never overlap. It's an OS file
// lock, so the OS releases it when the holder exits, even if it crashes.
pub enum RefreshLock {
    Held(#[expect(dead_code, reason = "held, not read: dropping it releases the lock")] fs::File),
    Busy,
    // No cache directory to put the lock in: go ahead unlocked, since there's
    // no index to protect either.
    Unavailable,
}

fn lock_path() -> Option<PathBuf> {
    Some(lock_path_for(&cache_path()?))
}

fn lock_path_for(index: &Path) -> PathBuf {
    index.with_file_name("index.lock")
}

fn open_lock(path: &Path) -> Option<fs::File> {
    fs::create_dir_all(path.parent()?).ok()?;
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()
}

pub fn try_lock_refresh() -> RefreshLock {
    match lock_path() {
        Some(path) => try_lock_refresh_at(&path),
        None => RefreshLock::Unavailable,
    }
}

fn try_lock_refresh_at(path: &Path) -> RefreshLock {
    let Some(file) = open_lock(path) else {
        return RefreshLock::Unavailable;
    };
    match file.try_lock() {
        Ok(()) => RefreshLock::Held(file),
        Err(fs::TryLockError::WouldBlock) => RefreshLock::Busy,
        Err(fs::TryLockError::Error(_)) => RefreshLock::Unavailable,
    }
}

// Blocks until any running crawl finishes. None means no lock could be taken;
// the caller goes ahead unlocked, as it would have before locking existed.
pub fn lock_refresh() -> Option<fs::File> {
    let file = open_lock(&lock_path()?)?;
    file.lock().ok()?;
    Some(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::prune_set;
    use crate::test_util::{repos, temp_base};

    // ---- cache serialization ----

    fn scope(root: &str, extra_prune: &str) -> Scope {
        Scope {
            root: PathBuf::from(root),
            prune: prune_set(Some(OsString::from(extra_prune))),
        }
    }

    const HOME_SRC: &str = "/home/kris/src";

    #[test]
    fn cache_round_trips() {
        let s = scope(HOME_SRC, "vendor");
        let r = repos(&["/home/kris/src/a/neovim", "/home/kris/src/b/pgadmin"]);
        assert_eq!(parse_cache(&serialize_cache(&s, &r), &s), Some(r));
    }

    #[test]
    fn cache_rejected_when_root_differs() {
        let serialized = serialize_cache(&scope(HOME_SRC, ""), &repos(&["/x/y"]));
        assert_eq!(parse_cache(&serialized, &scope("/other/root", "")), None);
    }

    #[test]
    fn cache_rejected_when_prune_differs() {
        let serialized = serialize_cache(&scope(HOME_SRC, ""), &repos(&["/x/y"]));
        assert_eq!(
            parse_cache(&serialized, &scope(HOME_SRC, "worktrees")),
            None
        );
    }

    #[test]
    fn cache_survives_reordered_extra_prune() {
        let r = repos(&["/x/y"]);
        let serialized = serialize_cache(&scope(HOME_SRC, "vendor,dist"), &r);
        assert_eq!(
            parse_cache(&serialized, &scope(HOME_SRC, " dist,vendor")),
            Some(r)
        );
    }

    #[test]
    fn cache_without_prune_line_is_rejected() {
        // The old format: root line, then repo paths.
        let s = scope(HOME_SRC, "");
        let old = "/home/kris/src\n/home/kris/src/a/ripgrep\n";
        assert_eq!(parse_cache(old, &s), None);
        assert_eq!(parse_cache("/home/kris/src\n", &s), None);
    }

    #[test]
    fn empty_cache_parses_to_no_repos() {
        let s = scope(HOME_SRC, "");
        assert_eq!(parse_cache(&serialize_cache(&s, &[]), &s), Some(vec![]));
    }

    #[test]
    fn empty_contents_is_none() {
        assert_eq!(parse_cache("", &scope(HOME_SRC, "")), None);
    }

    #[test]
    fn paths_containing_a_newline_are_not_cached() {
        // The line-delimited format can't round-trip them: `bad\nname` would read
        // back as `bad` plus a bogus relative `name`.
        let s = scope(HOME_SRC, "");
        let r = repos(&["/home/kris/src/bad\nname", "/home/kris/src/ok"]);
        assert_eq!(
            parse_cache(&serialize_cache(&s, &r), &s),
            Some(repos(&["/home/kris/src/ok"]))
        );
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
        let s = scope(HOME_SRC, "");
        let r = repos(&["/home/kris/src/a/neovim", "/home/kris/src/b/pgadmin"]);

        assert!(write_cache_at(&path, &s, &r));
        assert!(
            path.exists(),
            "write_cache_at should create missing parents"
        );
        assert_eq!(read_cache_at(&path, &s), Some(r));
        assert_eq!(read_cache_at(&path, &scope("/other/root", "")), None);
        assert_eq!(read_cache_at(&path, &scope(HOME_SRC, "vendor")), None);

        let leftovers: Vec<String> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "left {leftovers:?} behind");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn unwritable_cache_reports_failure() {
        let base = temp_base("cache-unwritable");
        fs::write(base.join("file"), "").unwrap();
        let path = base.join("file/goto/index");
        assert!(!write_cache_at(&path, &scope("/r", ""), &repos(&["/r/a"])));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn reading_a_missing_cache_file_is_none() {
        let base = temp_base("cache-missing");
        assert_eq!(read_cache_at(&base.join("absent"), &scope("/r", "")), None);
        let _ = fs::remove_dir_all(&base);
    }
    #[test]
    fn lock_path_sits_next_to_the_index() {
        assert_eq!(
            lock_path_for(Path::new("/c/goto/index")),
            PathBuf::from("/c/goto/index.lock")
        );
    }

    #[test]
    fn refresh_lock_is_busy_while_held_and_free_once_dropped() {
        let base = temp_base("refresh-lock");
        let path = base.join("goto/index.lock");
        let held = try_lock_refresh_at(&path);
        assert!(
            matches!(held, RefreshLock::Held(_)),
            "first taker should hold it"
        );
        assert!(matches!(try_lock_refresh_at(&path), RefreshLock::Busy));
        drop(held);
        assert!(matches!(try_lock_refresh_at(&path), RefreshLock::Held(_)));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn refresh_lock_without_a_writable_dir_is_unavailable() {
        let base = temp_base("refresh-lock-unwritable");
        fs::write(base.join("file"), "").unwrap();
        assert!(matches!(
            try_lock_refresh_at(&base.join("file/goto/index.lock")),
            RefreshLock::Unavailable
        ));
        let _ = fs::remove_dir_all(&base);
    }
}
