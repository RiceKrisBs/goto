use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ignore::{WalkBuilder, WalkState};

use crate::scope::Scope;

pub fn crawl(scope: &Scope) -> Vec<PathBuf> {
    // Owned copy: the walker's filter must be 'static. A handful of short names.
    discover_repos(&scope.root, scope.prune.clone())
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
    use crate::scope::prune_set;
    use crate::test_util::temp_base;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::symlink;

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
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

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
}
