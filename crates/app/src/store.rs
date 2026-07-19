//! The app's only durable state: pinned/recent repos and pinned PRs. Everything
//! else is rebuilt each launch. Best-effort — any I/O or parse failure degrades
//! to defaults and never panics or blocks startup.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many recent repos to remember.
const RECENT_CAP: usize = 8;
/// How many recent PRs to remember (shown directly in the cmd-k home for
/// one-click reopen).
const RECENT_PR_CAP: usize = 10;

/// A pull request the user opened, kept so it can be reopened in one click.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentPr {
    pub slug: String,
    pub number: u64,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    /// Favourite repos ("owner/repo"), in user (insertion) order.
    #[serde(default)]
    pub pinned_repos: Vec<String>,
    /// Most-recently-opened repos, newest first, deduped, capped at RECENT_CAP.
    #[serde(default)]
    pub recent_repos: Vec<String>,
    /// Most-recently-opened PRs, newest first, deduped by (slug, number),
    /// capped at RECENT_PR_CAP.
    #[serde(default)]
    pub recent_prs: Vec<RecentPr>,
    /// Pinned PR numbers per repo ("owner/repo" -> numbers).
    #[serde(default)]
    pub pinned_prs: BTreeMap<String, Vec<u64>>,
}

impl Store {
    /// Load from the default path; missing/unreadable/malformed → default.
    pub fn load() -> Self {
        match default_path() {
            Some(path) => Self::load_from(&path),
            None => Self::default(),
        }
    }

    /// Save to the default path, best-effort.
    pub fn save(&self) {
        if let Some(path) = default_path() {
            self.save_to(&path);
        }
    }

    pub fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Atomic write: temp file then rename. Creates the parent dir. Best-effort.
    pub fn save_to(&self, path: &Path) {
        let Ok(json) = serde_json::to_vec_pretty(self) else {
            return;
        };
        if let Some(dir) = path.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return;
            }
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    pub fn is_pinned_repo(&self, slug: &str) -> bool {
        self.pinned_repos.iter().any(|r| r == slug)
    }

    /// Add or remove `slug` from pinned repos.
    pub fn toggle_pinned_repo(&mut self, slug: &str) {
        if let Some(pos) = self.pinned_repos.iter().position(|r| r == slug) {
            self.pinned_repos.remove(pos);
        } else {
            self.pinned_repos.push(slug.to_string());
        }
    }

    /// Record `slug` as most-recently opened: move to front, dedupe, cap.
    pub fn note_recent_repo(&mut self, slug: &str) {
        self.recent_repos.retain(|r| r != slug);
        self.recent_repos.insert(0, slug.to_string());
        self.recent_repos.truncate(RECENT_CAP);
    }

    /// Record a just-opened PR: move to front, dedupe by (slug, number), cap.
    /// The title refreshes to the latest seen.
    pub fn note_recent_pr(&mut self, slug: &str, number: u64, title: &str) {
        self.recent_prs
            .retain(|p| !(p.slug == slug && p.number == number));
        self.recent_prs.insert(
            0,
            RecentPr {
                slug: slug.to_string(),
                number,
                title: title.to_string(),
            },
        );
        self.recent_prs.truncate(RECENT_PR_CAP);
    }

    pub fn is_pinned_pr(&self, slug: &str, number: u64) -> bool {
        self.pinned_prs.get(slug).is_some_and(|v| v.contains(&number))
    }

    pub fn pinned_prs_for(&self, slug: &str) -> &[u64] {
        self.pinned_prs.get(slug).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Add or remove a pinned PR number for `slug`; drops the entry when empty.
    pub fn toggle_pinned_pr(&mut self, slug: &str, number: u64) {
        let list = self.pinned_prs.entry(slug.to_string()).or_default();
        if let Some(pos) = list.iter().position(|n| *n == number) {
            list.remove(pos);
        } else {
            list.push(number);
        }
        if list.is_empty() {
            self.pinned_prs.remove(slug);
        }
    }
}

/// `~/Library/Application Support/lgtm/state.json` on macOS, via `dirs`.
fn default_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("lgtm").join("state.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lgtm-store-test-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("state.json")
    }

    #[test]
    fn round_trips_through_disk() {
        let path = temp_path("roundtrip");
        let mut s = Store::default();
        s.toggle_pinned_repo("acme/api");
        s.note_recent_repo("acme/web");
        s.toggle_pinned_pr("acme/api", 482);
        s.save_to(&path);
        let loaded = Store::load_from(&path);
        assert_eq!(s, loaded);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_and_corrupt_files_default() {
        let path = temp_path("missing");
        assert_eq!(Store::load_from(&path), Store::default());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        assert_eq!(Store::load_from(&path), Store::default());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn recent_is_move_to_front_deduped_and_capped() {
        let mut s = Store::default();
        for i in 0..10 {
            s.note_recent_repo(&format!("o/r{i}"));
        }
        assert_eq!(s.recent_repos.len(), RECENT_CAP);
        assert_eq!(s.recent_repos[0], "o/r9"); // newest first
        s.note_recent_repo("o/r5"); // re-open an older one
        assert_eq!(s.recent_repos[0], "o/r5");
        assert_eq!(s.recent_repos.iter().filter(|r| *r == "o/r5").count(), 1);
    }

    #[test]
    fn recent_prs_move_to_front_dedupe_and_cap() {
        let mut s = Store::default();
        for i in 0..12 {
            s.note_recent_pr("o/r", i, &format!("PR {i}"));
        }
        assert_eq!(s.recent_prs.len(), RECENT_PR_CAP);
        assert_eq!(s.recent_prs[0].number, 11); // newest first
        // Reopening an existing PR moves it to front with a refreshed title,
        // without duplicating.
        s.note_recent_pr("o/r", 5, "PR 5 (updated)");
        assert_eq!(s.recent_prs[0].number, 5);
        assert_eq!(s.recent_prs[0].title, "PR 5 (updated)");
        assert_eq!(
            s.recent_prs.iter().filter(|p| p.number == 5 && p.slug == "o/r").count(),
            1
        );
        // Same number in a different repo is a distinct entry.
        s.note_recent_pr("o/other", 5, "other");
        assert_eq!(s.recent_prs.iter().filter(|p| p.number == 5).count(), 2);
    }

    #[test]
    fn toggle_repo_and_pr_are_reversible() {
        let mut s = Store::default();
        s.toggle_pinned_repo("a/b");
        assert!(s.is_pinned_repo("a/b"));
        s.toggle_pinned_repo("a/b");
        assert!(!s.is_pinned_repo("a/b"));

        s.toggle_pinned_pr("a/b", 7);
        assert!(s.is_pinned_pr("a/b", 7));
        assert_eq!(s.pinned_prs_for("a/b"), &[7]);
        s.toggle_pinned_pr("a/b", 7);
        assert!(!s.is_pinned_pr("a/b", 7));
        assert!(s.pinned_prs.get("a/b").is_none()); // empty entry dropped
    }
}
