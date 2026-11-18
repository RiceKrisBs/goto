use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

// Directories the crawl never descends into: the built-in noise, plus any names
// the user appends via $GOTO_EXTRA_PRUNE. The crawl walks hidden entries,
// so `.git` here prevents it from being walked.
const DEFAULT_PRUNE: &[&str] = &["node_modules", ".terraform", ".git"];

// What an index is built for: changing either one means a different repo list.
pub struct Scope {
    pub root: PathBuf,
    pub prune: HashSet<String>,
}

impl Scope {
    // Sorted so reordering $GOTO_EXTRA_PRUNE doesn't invalidate the cache. Names
    // can't contain a comma, since `prune_set` splits on it.
    pub fn prune_key(&self) -> String {
        let mut names: Vec<&str> = self.prune.iter().map(String::as_str).collect();
        names.sort_unstable();
        names.join(",")
    }
}

// Absolute because the cache stores the root: a relative one would match any
// directory that happens to have the same relative path.
pub fn resolve_root() -> Option<PathBuf> {
    let root = resolve_root_from(env::var_os("GOTO_ROOT"), env::var_os("HOME"))?;
    std::path::absolute(root).ok()
}

// An empty value is treated as unset: joining onto "" yields a relative path.
fn resolve_root_from(goto_root: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    if let Some(val) = goto_root
        && !val.is_empty()
    {
        return Some(expand_tilde_with(PathBuf::from(val), home.as_deref()));
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

pub fn prune_from_env() -> HashSet<String> {
    prune_set(env::var_os("GOTO_EXTRA_PRUNE"))
}

pub fn prune_set(extra: Option<OsString>) -> HashSet<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(set.contains("node_modules"));
        assert_eq!(set.len(), 5);
    }

    #[test]
    fn prune_trims_whitespace_and_drops_empties() {
        let set = prune_set(Some(OsString::from(" vendor , , dist ,")));
        assert!(set.contains("vendor"));
        assert!(set.contains("dist"));
        assert!(!set.contains(""));
        assert_eq!(set.len(), 5);
    }

    #[test]
    fn prune_empty_env_adds_nothing() {
        assert_eq!(prune_set(Some(OsString::new())).len(), 3);
    }

    #[test]
    fn prune_key_is_sorted_and_order_independent() {
        let a = prune_set(Some(OsString::from("vendor,dist")));
        let b = prune_set(Some(OsString::from(" dist,vendor")));
        let scope = |prune| Scope {
            root: PathBuf::from("/r"),
            prune,
        };
        assert_eq!(
            scope(a).prune_key(),
            ".git,.terraform,dist,node_modules,vendor"
        );
        assert_eq!(
            scope(b).prune_key(),
            ".git,.terraform,dist,node_modules,vendor"
        );
    }
}
