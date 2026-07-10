use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;

use ignore::{WalkBuilder, WalkState};

const PRUNE: &[&str] = &["node_modules", ".terraform", ".git"];

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let query = match args.first() {
        Some(q) if !q.is_empty() => q.to_lowercase(),
        _ => {
            eprintln!("usage: gt-bin <name>");
            return ExitCode::FAILURE;
        }
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

    let repos = discover_repos(&root);

    // Exact basename match (case-insensitive), then substring fallback.
    let mut exact: Vec<&PathBuf> = repos
        .iter()
        .filter(|p| basename(p).map(|b| b == query).unwrap_or(false))
        .collect();
    exact.sort();

    let matches: Vec<&PathBuf> = if !exact.is_empty() {
        exact
    } else {
        let mut fuzzy: Vec<&PathBuf> = repos
            .iter()
            .filter(|p| basename(p).map(|b| b.contains(&query)).unwrap_or(false))
            .collect();
        fuzzy.sort();
        fuzzy
    };

    if matches.is_empty() {
        eprintln!("gt: no repo matching '{query}'");
        return ExitCode::FAILURE;
    }

    for p in matches {
        println!("{}", p.display());
    }
    ExitCode::SUCCESS
}

// Search root: $GOTO_ROOT if set (with a leading `~` expanded), else ~/src.
fn resolve_root() -> Option<PathBuf> {
    if let Some(val) = env::var_os("GOTO_ROOT") {
        if !val.is_empty() {
            return Some(expand_tilde(PathBuf::from(val)));
        }
    }
    env::var_os("HOME").map(|home| PathBuf::from(home).join("src"))
}

fn expand_tilde(path: PathBuf) -> PathBuf {
    let Some(home) = env::var_os("HOME") else {
        return path;
    };
    match path.strip_prefix("~") {
        Ok(rest) => PathBuf::from(home).join(rest),
        Err(_) => path,
    }
}

fn basename(p: &PathBuf) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase())
}

fn discover_repos(root: &PathBuf) -> Vec<PathBuf> {
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
