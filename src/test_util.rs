use std::env;
use std::fs;
use std::path::PathBuf;

pub fn repos(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

pub fn paths(found: &[&PathBuf]) -> Vec<String> {
    found.iter().map(|p| p.display().to_string()).collect()
}

// A fresh directory per test, so the filesystem cases can run in parallel.
pub fn temp_base(name: &str) -> PathBuf {
    let base = env::temp_dir().join(format!("goto-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base
}
