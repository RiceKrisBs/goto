use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MatchKind {
    Exact,
    Substring,
    Fuzzy,
}

#[derive(Debug)]
pub struct Matches<'a> {
    pub kind: MatchKind,
    pub repos: Vec<&'a PathBuf>,
}

// Exact name match, else substring, else close spelling. Each tier runs only if
// the ones before it found nothing, so a fuzzy score is never computed when an
// exact or substring match exists.
pub fn match_repos<'a>(
    repos: &'a [PathBuf],
    query: &str,
    keep: impl Fn(&Path) -> bool,
) -> Matches<'a> {
    let exact = best(repos, &keep, |k| (k == query).then_some(()));
    if !exact.is_empty() {
        return Matches {
            kind: MatchKind::Exact,
            repos: exact,
        };
    }
    let substring = best(repos, &keep, |k| k.contains(query).then_some(()));
    if !substring.is_empty() {
        return Matches {
            kind: MatchKind::Substring,
            repos: substring,
        };
    }
    let q: Vec<char> = query.chars().collect();
    let max = max_typos(q.len());
    let fuzzy = if max == 0 {
        Vec::new()
    } else {
        best(repos, &keep, |k| fuzzy_score(&q, k, max))
    };
    Matches {
        kind: MatchKind::Fuzzy,
        repos: fuzzy,
    }
}

// The repos sharing the lowest score, sorted by path. `keep` runs only on repos
// that scored, so a rejected best match falls through to the next best score
// and a miss doesn't stat every repo.
fn best<'a, S: Ord + Copy>(
    repos: &'a [PathBuf],
    keep: &impl Fn(&Path) -> bool,
    score: impl Fn(&str) -> Option<S>,
) -> Vec<&'a PathBuf> {
    let scored: Vec<(S, &PathBuf)> = repos
        .iter()
        .filter_map(|p| Some((score(&match_key(p)?)?, p)))
        .filter(|(_, p)| keep(p))
        .collect();
    let Some(min) = scored.iter().map(|(s, _)| *s).min() else {
        return Vec::new();
    };
    let mut found: Vec<&PathBuf> = scored
        .into_iter()
        .filter(|(s, _)| *s == min)
        .map(|(_, p)| p)
        .collect();
    found.sort();
    found
}

// Field order is the ranking: fewest typos first, then the name whose start is
// closest to the query, so `dashi` picks `dashboard` over `admin-dash`.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
struct FuzzyScore {
    typos: usize,
    from_name_start: usize,
}

fn fuzzy_score(query: &[char], key: &str, max_typos: usize) -> Option<FuzzyScore> {
    let name: Vec<char> = key.chars().collect();
    let typos = edit_distance(query, &name, Start::Word);
    (typos <= max_typos).then(|| FuzzyScore {
        typos,
        from_name_start: edit_distance(query, &name, Start::Name),
    })
}

// Short queries get no typo allowance: at one typo, `api` would hit half of
// any repo list.
fn max_typos(query_len: usize) -> usize {
    match query_len {
        0..4 => 0,
        4..8 => 1,
        _ => 2,
    }
}

#[derive(Clone, Copy)]
enum Start {
    Name,
    Word,
}

// Fewest single-letter edits (insert, delete, substitute, swap two adjacent
// letters) that turn `query` into a stretch of `name` beginning at `start`:
// the first letter of the name, or the first letter of any word in it (after
// `-`, `_` or `.`). Mid-word starts are ruled out so `goto` can't match the
// `oto` in `photos`.
fn edit_distance(query: &[char], name: &[char], start: Start) -> usize {
    // Large enough to never win, small enough not to overflow when added to.
    const BLOCKED: usize = usize::MAX / 2;
    let n = name.len();
    // prev[j]: cost of matching the query so far against a stretch of `name`
    // ending just before name[j].
    let mut prev: Vec<usize> = (0..=n)
        .map(|j| match start {
            Start::Name => j,
            Start::Word if j == 0 || matches!(name[j - 1], '-' | '_' | '.') => 0,
            Start::Word => BLOCKED,
        })
        .collect();
    let mut prev2 = Vec::new();
    for i in 1..=query.len() {
        let mut row = vec![i; n + 1];
        for j in 1..=n {
            let sub = usize::from(query[i - 1] != name[j - 1]);
            let mut d = (prev[j] + 1).min(row[j - 1] + 1).min(prev[j - 1] + sub);
            if i > 1 && j > 1 && query[i - 1] == name[j - 2] && query[i - 2] == name[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            row[j] = d;
        }
        prev2 = std::mem::replace(&mut prev, row);
    }
    prev.into_iter()
        .min()
        .expect("the row has an entry per name position, plus one")
}

// The leaf name lowercased, for matching and sorting; see `display_name` for output.
pub fn match_key(p: &Path) -> Option<String> {
    p.file_name().map(|n| n.to_string_lossy().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{paths, repos, temp_base};
    use std::fs;

    fn matched<'a>(r: &'a [PathBuf], query: &str) -> Vec<&'a PathBuf> {
        match_repos(r, query, |_| true).repos
    }

    // ---- match_repos ----

    #[test]
    fn deleted_repos_are_not_jump_targets() {
        let base = temp_base("existing");
        fs::create_dir_all(base.join("a/neovim")).unwrap();
        let r = vec![base.join("a/neovim"), base.join("b/neovim")];
        assert_eq!(
            match_repos(&r, "neovim", Path::exists).repos,
            [&base.join("a/neovim")]
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn deleted_exact_match_falls_back_to_substring() {
        let base = temp_base("fallback");
        fs::create_dir_all(base.join("b/neovim-qt")).unwrap();
        let r = vec![base.join("a/neovim"), base.join("b/neovim-qt")];
        assert_eq!(
            match_repos(&r, "neovim", Path::exists).repos,
            [&base.join("b/neovim-qt")]
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn exact_name_match() {
        let r = repos(&["/src/a/neovim", "/src/b/other"]);
        assert_eq!(paths(&matched(&r, "neovim")), ["/src/a/neovim"]);
    }

    #[test]
    fn match_is_case_insensitive() {
        let r = repos(&["/src/a/Neovim"]);
        assert_eq!(paths(&matched(&r, "neovim")), ["/src/a/Neovim"]);
    }

    #[test]
    fn falls_back_to_substring_when_no_exact_match() {
        let r = repos(&["/src/acme/pgadmin", "/src/x/unrelated"]);
        assert_eq!(paths(&matched(&r, "adm")), ["/src/acme/pgadmin"]);
    }

    #[test]
    fn exact_match_wins_over_substring() {
        let r = repos(&["/src/a/skills", "/src/b/skills-extra"]);
        assert_eq!(paths(&matched(&r, "skills")), ["/src/a/skills"]);
    }

    #[test]
    fn ambiguous_exact_matches_return_all_sorted() {
        let r = repos(&["/src/kris/skills", "/src/ai/skills"]);
        assert_eq!(
            paths(&matched(&r, "skills")),
            ["/src/ai/skills", "/src/kris/skills"]
        );
    }

    #[test]
    fn no_match_returns_empty() {
        let r = repos(&["/src/a/neovim"]);
        assert!(matched(&r, "zzz").is_empty());
    }

    #[test]
    fn matches_deeply_nested_repo() {
        let r = repos(&["/src/acme/terraform/modules/aws-vpc"]);
        assert_eq!(
            paths(&matched(&r, "aws-vpc")),
            ["/src/acme/terraform/modules/aws-vpc"]
        );
    }

    #[test]
    fn match_reports_which_tier_answered() {
        let r = repos(&["/src/a/ripgrep", "/src/b/ripgrep-all"]);
        assert_eq!(match_repos(&r, "ripgrep", |_| true).kind, MatchKind::Exact);
        assert_eq!(match_repos(&r, "all", |_| true).kind, MatchKind::Substring);
        assert_eq!(match_repos(&r, "ripgerp", |_| true).kind, MatchKind::Fuzzy);
    }

    // ---- fuzzy matching ----

    #[test]
    fn substring_match_wins_over_fuzzy() {
        // `tails` is a typo of `rails`, but it's also a substring of `tailscale`.
        let r = repos(&["/src/a/tailscale", "/src/b/rails"]);
        assert_eq!(paths(&matched(&r, "tails")), ["/src/a/tailscale"]);
    }

    #[test]
    fn fuzzy_matches_common_typos() {
        let r = repos(&["/src/a/dashboard", "/src/b/ansible"]);
        for typo in [
            "dashbboard",
            "adshboard",
            "dashbaord",
            "dashbord",
            "dashboqrd",
        ] {
            assert_eq!(paths(&matched(&r, typo)), ["/src/a/dashboard"], "{typo}");
        }
    }

    #[test]
    fn fuzzy_prefers_names_that_start_like_the_query() {
        let r = repos(&["/src/a/admin-dash", "/src/b/dashboard"]);
        assert_eq!(paths(&matched(&r, "dashi")), ["/src/b/dashboard"]);
    }

    #[test]
    fn fuzzy_ties_return_all_sorted() {
        let r = repos(&["/src/b/ripgrep-all", "/src/a/ripgrep"]);
        assert_eq!(
            paths(&matched(&r, "ripgerp")),
            ["/src/a/ripgrep", "/src/b/ripgrep-all"]
        );
    }

    #[test]
    fn short_queries_get_no_fuzzy_match() {
        let r = repos(&["/src/a/ripgrep"]);
        assert!(matched(&r, "rpg").is_empty());
    }

    #[test]
    fn typo_allowance_grows_with_query_length() {
        let r = repos(&["/src/a/ansible", "/src/b/dashboard"]);
        // Two typos each: 7 chars allows one, 8 chars allows two.
        assert!(matched(&r, "ansbiel").is_empty());
        assert_eq!(paths(&matched(&r, "dshbaord")), ["/src/b/dashboard"]);
    }

    #[test]
    fn rejected_fuzzy_match_falls_back_to_the_next_closest() {
        let r = repos(&["/src/a/dashboard", "/src/b/admin-dash"]);
        let m = match_repos(&r, "dashi", |p| !p.ends_with("dashboard"));
        assert_eq!(m.kind, MatchKind::Fuzzy);
        assert_eq!(paths(&m.repos), ["/src/b/admin-dash"]);
    }

    #[test]
    fn fuzzy_does_not_match_mid_word() {
        // `oto` is inside `photos` and `ite` inside `sqlite`, but neither
        // starts a word, so these stay misses.
        let r = repos(&["/src/a/photos", "/src/b/sqlite"]);
        assert!(matched(&r, "goto").is_empty());
        assert!(matched(&r, "vite").is_empty());
    }

    #[test]
    fn fuzzy_matches_a_later_word_in_the_name() {
        let r = repos(&["/src/a/rust-analyzer"]);
        assert_eq!(paths(&matched(&r, "analzyer")), ["/src/a/rust-analyzer"]);
    }

    #[test]
    fn edit_distance_counts_typos_from_a_start_point() {
        let d = |q: &str, name: &str, start| {
            let q: Vec<char> = q.chars().collect();
            let name: Vec<char> = name.chars().collect();
            edit_distance(&q, &name, start)
        };
        assert_eq!(d("dash", "admin-dash", Start::Word), 0);
        assert_eq!(d("dash", "admin-dash", Start::Name), 3);
        assert_eq!(d("dashi", "dashboard", Start::Name), 1);
        assert_eq!(d("adshboard", "dashboard", Start::Word), 1);
        assert_eq!(d("dashbboard", "dashboard", Start::Word), 1);
        for sep in ["-", "_", "."] {
            assert_eq!(
                d("dash", &format!("admin{sep}dash"), Start::Word),
                0,
                "{sep}"
            );
        }
        assert_eq!(d("oto", "photos", Start::Word), 2);
    }

    // ---- match_key ----

    #[test]
    fn match_key_lowercases_leaf() {
        assert_eq!(match_key(Path::new("/a/B/Neovim")), Some("neovim".into()));
    }
}
