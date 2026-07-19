# PR Dashboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user reach a repo's PRs from cmd-k without retyping `owner/repo`, by remembering recent repos, pinning favourite repos and PRs, and adding `cmd-1..9` tab switching.

**Architecture:** A new isolated `store.rs` persists a small JSON file of pinned/recent repos and pinned PRs. The existing cmd-k palette gains a `RepoHome` entry step (pinned + recent repos + folder) that flows into the existing PR-list step, which now sorts pinned PRs to the top and offers a per-row pin star. Pure helpers (`home_rows`, `order_prs`, and all of `store.rs`) are unit-tested; the gpui rendering is verified by compile + the existing suite + manual smoke.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component 0.5.1, serde/serde_json, `dirs` crate, `gh` CLI.

## Global Constraints

- Edition 2021 (so `std::env::set_var` etc. are safe; no edition-2024 assumptions).
- `crates/app` package name is `lgtm`; build/test with `cargo build -p lgtm` / `cargo test -p lgtm`.
- All persistence is best-effort: any I/O or parse failure degrades to defaults and never panics or blocks startup.
- Follow the existing codebase style: `std::fs` best-effort I/O, `theme::` colors, `SharedString`, fixed-height palette rows (`PALETTE_ROW_HEIGHT`).
- `gh::PrSummary.number` and `gh::PrLocator.number` are `u64` — pinned PR numbers are `u64`.
- Do not touch the `gh`, `git`, `diff-core`, or `syntax` crates.
- Keep all work on branch `fix/highlighting-sidebar-wrapping`.

---

### Task 1: Persistence module `store.rs`

**Files:**
- Create: `crates/app/src/store.rs`
- Modify: `crates/app/src/main.rs` (add `mod store;` near the other top-level `mod` / `use` lines, e.g. beside `mod lsp_client;` / `mod theme;`)
- Modify: `crates/app/Cargo.toml` (add `dirs = "6"`, and `serde` with derive if not already a direct dep)

**Interfaces:**
- Produces:
  - `store::Store` with public fields `pinned_repos: Vec<String>`, `recent_repos: Vec<String>`, `pinned_prs: std::collections::BTreeMap<String, Vec<u64>>`.
  - `Store::load() -> Store`, `Store::save(&self)`, `Store::load_from(&Path) -> Store`, `Store::save_to(&self, &Path)`.
  - `Store::is_pinned_repo(&self, &str) -> bool`, `toggle_pinned_repo(&mut self, &str)`, `note_recent_repo(&mut self, &str)`.
  - `Store::is_pinned_pr(&self, &str, u64) -> bool`, `pinned_prs_for(&self, &str) -> &[u64]`, `toggle_pinned_pr(&mut self, &str, u64)`.

- [ ] **Step 1: Add dependencies**

In `crates/app/Cargo.toml` `[dependencies]`, add:

```toml
dirs = "6"
serde = { workspace = true }
serde_json = "1"
```

(If `serde`/`serde_json` are already listed as direct deps, leave them; the root workspace already provides `serde` with `derive`.) Run `cargo build -p lgtm` to confirm it resolves. Expected: builds (no code using them yet).

- [ ] **Step 2: Write `store.rs` with the failing tests first**

Create `crates/app/src/store.rs`:

```rust
//! The app's only durable state: pinned/recent repos and pinned PRs. Everything
//! else is rebuilt each launch. Best-effort — any I/O or parse failure degrades
//! to defaults and never panics or blocks startup.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many recent repos to remember.
const RECENT_CAP: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    /// Favourite repos ("owner/repo"), in user (insertion) order.
    #[serde(default)]
    pub pinned_repos: Vec<String>,
    /// Most-recently-opened repos, newest first, deduped, capped at RECENT_CAP.
    #[serde(default)]
    pub recent_repos: Vec<String>,
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
```

- [ ] **Step 3: Register the module**

In `crates/app/src/main.rs`, add `mod store;` next to the existing module declarations (search for `mod theme;` / `mod lsp_client;`). Add `use store::Store;` near the other `use` lines.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p lgtm store::`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/store.rs crates/app/src/main.rs crates/app/Cargo.toml Cargo.lock
git commit -m "feat: persist pinned/recent repos and pinned PRs (store.rs)"
```

---

### Task 2: Pure palette helpers `home_rows` and `order_prs`

**Files:**
- Modify: `crates/app/src/main.rs` (add two free functions + an enum near the palette helpers, e.g. just after `filter_base_refs` ~ line 3062; add tests in the existing `mod tests`)

**Interfaces:**
- Consumes: `gh::PrSummary` (has `.number: u64`), `store::Store` fields.
- Produces:
  - `enum HomeRow { Repo { slug: String, pinned: bool }, Folder }` (derive `Debug, Clone, PartialEq`).
  - `fn home_rows(pinned: &[String], recent: &[String], query: &str) -> Vec<HomeRow>`.
  - `fn order_prs(all: &[gh::PrSummary], filtered: &[usize], pinned: &[u64]) -> Vec<usize>`.

- [ ] **Step 1: Write the failing tests** (in `mod tests` in main.rs)

```rust
#[test]
fn home_rows_pinned_first_then_recent_then_folder() {
    let pinned = vec!["a/api".to_string(), "a/web".to_string()];
    let recent = vec!["a/web".to_string(), "b/infra".to_string()];
    let rows = home_rows(&pinned, &recent, "");
    assert_eq!(
        rows,
        vec![
            HomeRow::Repo { slug: "a/api".into(), pinned: true },
            HomeRow::Repo { slug: "a/web".into(), pinned: true },
            HomeRow::Repo { slug: "b/infra".into(), pinned: false }, // a/web deduped
            HomeRow::Folder,
        ]
    );
}

#[test]
fn home_rows_filters_by_query_and_drops_folder() {
    let pinned = vec!["a/api".to_string()];
    let recent = vec!["b/infra".to_string()];
    let rows = home_rows(&pinned, &recent, "infra");
    assert_eq!(rows, vec![HomeRow::Repo { slug: "b/infra".into(), pinned: false }]);
}

#[test]
fn order_prs_moves_pinned_to_top_preserving_order() {
    let prs = vec![
        pr_summary(10),
        pr_summary(20),
        pr_summary(30),
    ];
    let filtered = vec![0, 1, 2];
    let ordered = order_prs(&prs, &filtered, &[30, 10]);
    // pinned (in filtered order: 10 then 30), then the rest (20)
    assert_eq!(ordered, vec![0, 2, 1]);
}
```

Add this test helper inside `mod tests` (uses the existing `gh` import; fill any other required `PrSummary` fields to compile — check `crates/gh/src/lib.rs` for the full struct and `Author`):

```rust
fn pr_summary(number: u64) -> gh::PrSummary {
    gh::PrSummary {
        number,
        title: format!("PR {number}"),
        author: gh::Author { login: "me".into(), name: None },
        state: "OPEN".into(),
        is_draft: false,
        head_ref_name: "branch".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    }
}
```

(If `gh::Author`'s fields differ, adjust to match `crates/gh/src/lib.rs`. If `PrSummary`/`Author` are not constructible from outside `gh` — i.e. fields aren't `pub` — read the struct; the explore report shows fields are `pub`.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p lgtm home_rows_ order_prs_`
Expected: FAIL (functions/enum not defined).

- [ ] **Step 3: Implement the helpers** (near `filter_base_refs`)

```rust
/// A row in the repo-home palette step.
#[derive(Debug, Clone, PartialEq)]
enum HomeRow {
    /// A repo "owner/repo"; `pinned` marks favourites.
    Repo { slug: String, pinned: bool },
    /// The "open local folder" affordance, always last.
    Folder,
}

/// Rows for the repo-home step: pinned repos first (in pin order), then recent
/// repos not already pinned, filtered by a case-insensitive substring query,
/// with the Folder row last (kept only when it matches a non-empty query).
fn home_rows(pinned: &[String], recent: &[String], query: &str) -> Vec<HomeRow> {
    let q = query.trim().to_lowercase();
    let matches = |s: &str| q.is_empty() || s.to_lowercase().contains(&q);
    let mut rows = Vec::new();
    for slug in pinned {
        if matches(slug) {
            rows.push(HomeRow::Repo { slug: slug.clone(), pinned: true });
        }
    }
    for slug in recent {
        if pinned.iter().any(|p| p == slug) {
            continue;
        }
        if matches(slug) {
            rows.push(HomeRow::Repo { slug: slug.clone(), pinned: false });
        }
    }
    if q.is_empty() || "open local folder".contains(&q) {
        rows.push(HomeRow::Folder);
    }
    rows
}

/// Display order for a repo's PRs: pinned PRs first, then the rest, both
/// restricted to `filtered` (the fuzzy result) and preserving its order.
/// Returns indices into `all`.
fn order_prs(all: &[gh::PrSummary], filtered: &[usize], pinned: &[u64]) -> Vec<usize> {
    let mut pinned_ix = Vec::new();
    let mut rest_ix = Vec::new();
    for &ix in filtered {
        if pinned.contains(&all[ix].number) {
            pinned_ix.push(ix);
        } else {
            rest_ix.push(ix);
        }
    }
    pinned_ix.extend(rest_ix);
    pinned_ix
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p lgtm home_rows_ order_prs_`
Expected: 3 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/main.rs
git commit -m "feat: pure palette helpers home_rows and order_prs"
```

---

### Task 3: Wire `Store` into `ReviewApp`

**Files:**
- Modify: `crates/app/src/main.rs` — `struct ReviewApp` (fields), `ReviewApp::new` (init), and add a small helper.

**Interfaces:**
- Consumes: `store::Store` (Task 1).
- Produces: `ReviewApp.store: Store`, `ReviewApp.active_repo: Option<String>`, and `fn save_store(&self)` calling `self.store.save()`.

- [ ] **Step 1: Add fields to `struct ReviewApp`**

Find `struct ReviewApp {` and add (near `active: usize`):

```rust
    /// Durable pinned/recent repos + pinned PRs (the only persisted state).
    store: Store,
    /// Repo slug ("owner/repo") the palette PR-list is currently showing, so
    /// pin toggles and re-ordering know which repo they apply to.
    active_repo: Option<String>,
```

- [ ] **Step 2: Initialise in `ReviewApp::new`**

In the `let mut this = Self { ... }` literal inside `ReviewApp::new`, add:

```rust
            store: Store::load(),
            active_repo: None,
```

- [ ] **Step 3: Add the save helper** (in `impl ReviewApp`, near `open_palette`)

```rust
    fn save_store(&self) {
        self.store.save();
    }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p lgtm`
Expected: builds (fields unused-warnings are acceptable at this step; later tasks use them). If the compiler errors on unused, that's fine to ignore — do NOT add `#[allow]`.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/main.rs
git commit -m "feat: load persisted store into ReviewApp with active_repo"
```

---

### Task 4: Repo-home palette step

**Files:**
- Modify: `crates/app/src/main.rs` — `enum PaletteStep`, `open_palette`, `palette_back`, `palette_move`, `palette_query_changed`, `palette_confirm`, `render_palette`, plus new methods `palette_home_confirm`, `palette_home_activate`, `palette_toggle_pin_repo`.

**Interfaces:**
- Consumes: `home_rows` (Task 2), `Store` (Task 1/3), `active_repo`, existing `palette_fetch_prs`, `prompt_open_folder`, `parse_repo_slug`, `set_palette_input`, `palette_scroll`.
- Produces: `PaletteStep::RepoHome { selected: usize }` as the cmd-k entry point.

- [ ] **Step 1: Add the enum variant**

In `enum PaletteStep`, add as the first variant:

```rust
    /// Step 0 (cmd-k entry): pinned + recent repos, or type a new owner/repo.
    RepoHome { selected: usize },
```

- [ ] **Step 2: Make cmd-k open the home step**

In `open_palette`, replace the `Sources { selected: 0 }` assignment:

```rust
    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.palette = Some(PaletteStep::RepoHome { selected: 0 });
        self.palette_gen += 1;
        self.set_palette_input("", "type a repo (owner/repo) or filter…", window, cx);
        cx.notify();
    }
```

- [ ] **Step 3: Handle Esc and step-back**

In `palette_back`, change the close arm so `RepoHome` closes the palette, and make `RepoInput`/`PrList` fall back to `RepoHome` instead of `Sources`:

- In the first match arm, replace `Some(PaletteStep::Sources { .. })` with `Some(PaletteStep::RepoHome { .. })`.
- In the `RepoInput` arm, set `self.palette = Some(PaletteStep::RepoHome { selected: 0 })` and `set_palette_input("", "type a repo (owner/repo) or filter…", …)`.
- In the `PrList` arm, set `self.palette = Some(PaletteStep::RepoHome { selected: 0 })` (instead of `RepoInput`) and `set_palette_input(&repo, "type a repo (owner/repo) or filter…", …)`.

(The `Sources` variant and `palette_activate_source`/`filtered_sources`/`PALETTE_SOURCES`/`SOURCE_PR`/`SOURCE_FOLDER`/`RepoInput` become dead once nothing constructs them. Leave `RepoInput` and its handling in place — it is still reachable via nothing now, but removing it is out of scope; if the compiler warns about unreachable `Sources`, remove the `Sources` variant and its now-dead arms in `palette_move`/`palette_query_changed`/`palette_confirm`/`palette_activate_source`/`render_palette` and delete `filtered_sources`, `PALETTE_SOURCES`, `SOURCE_PR`, `SOURCE_FOLDER`, `palette_activate_source`. Do this cleanup only if it compiles cleanly; otherwise keep them.)

- [ ] **Step 4: Navigation — `palette_move`**

Add an arm to `palette_move` (before the `_ => return`):

```rust
            Some(PaletteStep::RepoHome { selected }) => {
                let len = home_rows(&self.store.pinned_repos, &self.store.recent_repos, &query).len();
                if len > 0 {
                    *selected = (*selected as isize + delta).clamp(0, len as isize - 1) as usize;
                }
            }
```

- [ ] **Step 5: Query changes — `palette_query_changed`**

Add an arm (before `_ => return`):

```rust
            Some(PaletteStep::RepoHome { selected }) => {
                let len = home_rows(&self.store.pinned_repos, &self.store.recent_repos, &query).len();
                *selected = (*selected).min(len.saturating_sub(1));
            }
```

- [ ] **Step 6: Confirm — `palette_confirm`**

Add an arm (before the final `_ => {}`):

```rust
            Some(PaletteStep::RepoHome { .. }) => self.palette_home_confirm(window, cx),
```

- [ ] **Step 7: New methods** (in `impl ReviewApp`, near `palette_activate_source`)

```rust
    /// Enter on the repo-home step: a fully-typed `owner/repo` always wins;
    /// otherwise act on the selected row (open a repo or the folder dialog).
    fn palette_home_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.palette_input.read(cx).value().trim().to_string();
        if let Ok((owner, repo)) = parse_repo_slug(&query) {
            self.palette_fetch_prs(owner, repo, window, cx);
            return;
        }
        let Some(PaletteStep::RepoHome { selected }) = &self.palette else {
            return;
        };
        let selected = *selected;
        let rows = home_rows(&self.store.pinned_repos, &self.store.recent_repos, &query);
        match rows.into_iter().nth(selected) {
            Some(HomeRow::Repo { slug, .. }) => self.palette_home_activate(&slug, window, cx),
            Some(HomeRow::Folder) => {
                self.close_palette(window, cx);
                self.prompt_open_folder(cx);
            }
            None => {}
        }
    }

    /// Open a repo slug's PR list from the home step.
    fn palette_home_activate(&mut self, slug: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok((owner, repo)) = parse_repo_slug(slug) {
            self.palette_fetch_prs(owner, repo, window, cx);
        }
    }

    /// Toggle a repo's pin from the home step, persist, and stay on the step.
    fn palette_toggle_pin_repo(&mut self, slug: &str, cx: &mut Context<Self>) {
        self.store.toggle_pinned_repo(slug);
        self.save_store();
        cx.notify();
    }
```

- [ ] **Step 8: Set `active_repo` when a repo is opened**

In `palette_fetch_prs`, right after `self.palette = Some(PaletteStep::PrList { ... })`, add:

```rust
        self.active_repo = Some(format!("{owner}/{repo}"));
        self.store.note_recent_repo(&format!("{owner}/{repo}"));
        self.save_store();
```

Note: `owner` and `repo` are moved into the async block later in the function, so compute the slug BEFORE the async move, or clone: place these three lines immediately after setting `self.palette` and before `self.set_palette_input(...)`, using `format!("{owner}/{repo}")` while `owner`/`repo` are still owned locals (they are used again only inside the `cx.spawn` move closure below — clone there if the borrow checker complains: `let (owner, repo) = (owner.clone(), repo.clone());` is already effectively what happens via move; simplest is to build `let slug = format!("{owner}/{repo}");` at the top of the function and reuse it for the `PrList { repo: slug.clone() }`, `active_repo`, and `note_recent_repo`).

- [ ] **Step 9: Render the home step — `render_palette`**

In `render_palette`, add to the `header` match: `PaletteStep::RepoHome { .. } => None,`.

Add to the `body` match a `RepoHome` arm that renders the rows as a simple flex list (few rows; no virtualization needed), mirroring the `Sources` arm's styling. Each `Repo` row: a pin star (★ pinned / ☆ not) on the left that toggles the pin (with `cx.stop_propagation()`), then the slug; clicking the row opens it. The `Folder` row shows "Open local folder…".

```rust
            PaletteStep::RepoHome { selected } => {
                let selected = *selected;
                let rows = home_rows(&self.store.pinned_repos, &self.store.recent_repos, &query);
                let mut list = div().py_1().flex().flex_col();
                if rows.is_empty() {
                    list = list.child(
                        div().px_3().py_2().text_color(theme::overlay0())
                            .child(SharedString::from("no matches — type owner/repo and press enter")),
                    );
                }
                for (pos, row) in rows.into_iter().enumerate() {
                    let is_sel = pos == selected;
                    let base = div()
                        .id(("palette-home", pos))
                        .mx_1().px_2().h(px(PALETTE_ROW_HEIGHT)).rounded_md()
                        .flex().items_center().gap_2().cursor_pointer()
                        .when(is_sel, |r| r.bg(theme::surface0()))
                        .when(!is_sel, |r| r.hover(|s| s.bg(Hsla::from(theme::surface0()).opacity(0.5))));
                    let child = match row {
                        HomeRow::Repo { slug, pinned } => {
                            let slug_for_row = slug.clone();
                            let slug_for_star = slug.clone();
                            base
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.palette_home_activate(&slug_for_row, window, cx)
                                }))
                                .child(
                                    div()
                                        .id(("palette-home-star", pos))
                                        .flex_shrink_0().px_1().cursor_pointer()
                                        .text_color(if pinned { theme::peach() } else { theme::overlay0() })
                                        .child(SharedString::from(if pinned { "\u{2605}" } else { "\u{2606}" }))
                                        .on_click(cx.listener(move |this, _, _window, cx| {
                                            cx.stop_propagation();
                                            this.palette_toggle_pin_repo(&slug_for_star, cx);
                                        })),
                                )
                                .child(div().flex_1().min_w_0().truncate().text_color(theme::text())
                                    .child(SharedString::from(slug)))
                        }
                        HomeRow::Folder => base
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.close_palette(window, cx);
                                this.prompt_open_folder(cx);
                            }))
                            .child(div().flex_shrink_0().px_1().text_color(theme::overlay0())
                                .child(SharedString::from("\u{1F4C1}")))
                            .child(div().text_color(theme::text())
                                .child(SharedString::from("Open local folder\u{2026}"))),
                    };
                    list = list.child(child);
                }
                list.into_any_element()
            }
```

(If the `Sources` variant was left in place in Step 3, keep its arm too. `cx.stop_propagation()` is available inside `cx.listener` handlers in this gpui version — verify against an existing `.on_click(cx.listener(...))` usage; if the signature differs, read how other row click handlers are written and match them. The `\u{...}` escapes avoid non-ASCII source.)

- [ ] **Step 10: Build and smoke-test**

Run: `cargo build -p lgtm` → expect clean build.
Run: `cargo test -p lgtm` → expect all existing + new tests pass.
Manual (optional): `cargo run -p lgtm`, press cmd-k — the home step shows recents/pins; star toggles a repo; enter on a repo lists its PRs; Esc from PR list returns to home.

- [ ] **Step 11: Commit**

```bash
git add crates/app/src/main.rs
git commit -m "feat: cmd-k repo home with pinned + recent repos and folder"
```

---

### Task 5: Pin PRs to the top of the repo's list

**Files:**
- Modify: `crates/app/src/main.rs` — `palette_pr_row` (add pin star), `palette_fetch_prs` + `palette_query_changed` (order pinned to top), add `palette_toggle_pin_pr`, and update the `PrList` render call site to pass the pinned flag.

**Interfaces:**
- Consumes: `order_prs` (Task 2), `Store`, `active_repo`, `filter_prs`.
- Produces: `fn palette_toggle_pin_pr(&mut self, number: u64, cx: &mut Context<Self>)`; `palette_pr_row(pr, pos, selected, pinned, entity)`.

- [ ] **Step 1: Order pinned PRs to the top where `filtered` is computed**

In `palette_fetch_prs`, the result handler currently sets `filtered = filter_prs(&all, &query)`. Change it to order pinned first. Since the pinned list is per-repo, capture it before the async move:

At the top of `palette_fetch_prs` add `let pinned = self.store.pinned_prs_for(&format!("{owner}/{repo}")).to_vec();` and move `pinned` into the async closure; inside the `Ok(all) =>` arm replace:

```rust
                    Ok(all) => {
                        let filtered = order_prs(&all, &filter_prs(&all, &query), &pinned);
                        PrListState::Loaded { all, filtered, selected: 0 }
                    }
```

In `palette_query_changed`, the `PrList` arm currently does `*filtered = filter_prs(all, &query);`. Compute the pinned list first (before the `match &mut self.palette`, using `self.active_repo`):

```rust
        let pinned = self
            .active_repo
            .as_ref()
            .map(|r| self.store.pinned_prs_for(r).to_vec())
            .unwrap_or_default();
```

then in the arm: `*filtered = order_prs(all, &filter_prs(all, &query), &pinned);`.

- [ ] **Step 2: Add the PR pin toggle method** (near `palette_open_pr_row`)

```rust
    /// Toggle the pin on PR `number` in the active repo, persist, and re-order.
    fn palette_toggle_pin_pr(&mut self, number: u64, cx: &mut Context<Self>) {
        let Some(repo) = self.active_repo.clone() else {
            return;
        };
        self.store.toggle_pinned_pr(&repo, number);
        self.save_store();
        let query = self.palette_input.read(cx).value().to_string();
        let pinned = self.store.pinned_prs_for(&repo).to_vec();
        if let Some(PaletteStep::PrList {
            prs: PrListState::Loaded { all, filtered, selected },
            ..
        }) = &mut self.palette
        {
            *filtered = order_prs(all, &filter_prs(all, &query), &pinned);
            *selected = 0;
        }
        cx.notify();
    }
```

- [ ] **Step 3: Add a pin star to `palette_pr_row`**

Change the signature to `fn palette_pr_row(pr: &gh::PrSummary, pos: usize, selected: bool, pinned: bool, entity: gpui::Entity<ReviewApp>)`. After the status-dot child (and before the `#number` child), insert a star that toggles the pin without opening the row:

```rust
        .child({
            let entity = entity.clone();
            let number = pr.number;
            div()
                .id(("palette-pr-star", pos))
                .flex_shrink_0()
                .px_1()
                .cursor_pointer()
                .text_color(if pinned { theme::peach() } else { theme::overlay0() })
                .child(SharedString::from(if pinned { "\u{2605}" } else { "\u{2606}" }))
                .on_click(move |_, _window, cx| {
                    cx.stop_propagation();
                    entity.update(cx, |this, cx| this.palette_toggle_pin_pr(number, cx));
                })
        })
```

- [ ] **Step 4: Pass the pinned flag at the render call site**

In `render_palette`, the `PrList` `Loaded` arm builds rows via `palette_pr_row(pr, pos, pos == *selected, entity.clone())`. It runs inside the `uniform_list` closure with `let this = entity.read(cx);`. Compute the pinned set there and pass it. Add, after destructuring `all/filtered/selected` and before the `range` mapping:

```rust
                                let pinned: &[u64] = this
                                    .active_repo
                                    .as_deref()
                                    .map(|r| this.store.pinned_prs_for(r))
                                    .unwrap_or(&[]);
```

and change the row call to:

```rust
                                    .map(|(pos, pr)| {
                                        let is_pinned = pinned.contains(&pr.number);
                                        palette_pr_row(pr, pos, pos == *selected, is_pinned, entity.clone())
                                    })
```

- [ ] **Step 5: Build and test**

Run: `cargo build -p lgtm` → clean build.
Run: `cargo test -p lgtm` → all pass.
Manual (optional): open a repo's PRs, click ☆ on a row → it becomes ★ and jumps to the top; reopen the repo later → still pinned.

- [ ] **Step 6: Commit**

```bash
git add crates/app/src/main.rs
git commit -m "feat: pin PRs to the top of a repo's palette list"
```

---

### Task 6: `cmd-1..9` quick tab switching

**Files:**
- Modify: `crates/app/src/main.rs` — `actions!` list, `KeyBinding` registrations, and the action handlers on the root element.

**Interfaces:**
- Consumes: existing `activate(ix, window, cx)` and `self.items`.
- Produces: actions `GoToItem1`..`GoToItem9` bound to `cmd-1`..`cmd-9`.

- [ ] **Step 1: Declare the actions**

In the `actions!(lgtm, [ ... ])` macro list, add `GoToItem1, GoToItem2, GoToItem3, GoToItem4, GoToItem5, GoToItem6, GoToItem7, GoToItem8, GoToItem9`.

- [ ] **Step 2: Register keybindings**

In the `KeyBinding::new(...)` block (search `KeyBinding::new("ctrl-tab"`), add nine bindings in the `"ReviewApp"` context (match the `None`/context argument used by neighbouring bindings like `NextItem`):

```rust
                KeyBinding::new("cmd-1", GoToItem1, Some("ReviewApp")),
                KeyBinding::new("cmd-2", GoToItem2, Some("ReviewApp")),
                KeyBinding::new("cmd-3", GoToItem3, Some("ReviewApp")),
                KeyBinding::new("cmd-4", GoToItem4, Some("ReviewApp")),
                KeyBinding::new("cmd-5", GoToItem5, Some("ReviewApp")),
                KeyBinding::new("cmd-6", GoToItem6, Some("ReviewApp")),
                KeyBinding::new("cmd-7", GoToItem7, Some("ReviewApp")),
                KeyBinding::new("cmd-8", GoToItem8, Some("ReviewApp")),
                KeyBinding::new("cmd-9", GoToItem9, Some("ReviewApp")),
```

(Match the exact context argument form used by the existing `NextItem`/`ToggleSidebar` bindings — if they pass `None`, use `None`; if `Some("ReviewApp")`, use that. Read the surrounding lines.)

- [ ] **Step 3: Handle the actions**

Where the root element registers `.on_action(cx.listener(|this, _: &NextItem, ...))` (search `&NextItem`), add nine handlers. To avoid repetition, add one helper and nine thin handlers:

```rust
    fn goto_item(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        if n < self.items.len() {
            self.activate(n, window, cx);
        }
    }
```

and register:

```rust
            .on_action(cx.listener(|this, _: &GoToItem1, window, cx| this.goto_item(0, window, cx)))
            .on_action(cx.listener(|this, _: &GoToItem2, window, cx| this.goto_item(1, window, cx)))
            .on_action(cx.listener(|this, _: &GoToItem3, window, cx| this.goto_item(2, window, cx)))
            .on_action(cx.listener(|this, _: &GoToItem4, window, cx| this.goto_item(3, window, cx)))
            .on_action(cx.listener(|this, _: &GoToItem5, window, cx| this.goto_item(4, window, cx)))
            .on_action(cx.listener(|this, _: &GoToItem6, window, cx| this.goto_item(5, window, cx)))
            .on_action(cx.listener(|this, _: &GoToItem7, window, cx| this.goto_item(6, window, cx)))
            .on_action(cx.listener(|this, _: &GoToItem8, window, cx| this.goto_item(7, window, cx)))
            .on_action(cx.listener(|this, _: &GoToItem9, window, cx| this.goto_item(8, window, cx)))
```

(Confirm `activate`'s exact signature — the explore report shows `fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>)`. Match it.)

- [ ] **Step 4: Build and test**

Run: `cargo build -p lgtm` → clean build.
Run: `cargo test -p lgtm` → all pass.
Manual (optional): open 2-3 PRs, press cmd-2 → jumps to the 2nd tab.

- [ ] **Step 5: Commit**

```bash
git add crates/app/src/main.rs
git commit -m "feat: cmd-1..9 quick switch between open tabs"
```

---

## Self-Review

**Spec coverage:**
- Persistence (`store.json`, dirs, atomic write, best-effort) → Task 1. ✓
- Repo home (pinned + recent + folder, type owner/repo) → Task 4. ✓
- Pin repos → Task 4 (star + `toggle_pinned_repo`). ✓
- Pin PRs to top → Task 5 (`order_prs`, star, `toggle_pinned_pr`). ✓
- Recent MRU with cap → Task 1 (`note_recent_repo`, RECENT_CAP=8) + wired in Task 4 Step 8. ✓
- `active_repo` memory → Task 3 + set in Task 4 Step 8. ✓
- cmd-1..9 quick switching → Task 6. ✓
- Pure helpers unit-tested → Tasks 1 & 2. ✓
- gh crate untouched, isDraft-only status dot → respected (no gh changes). ✓
- Out of scope (cross-repo, CI columns, polling, widget rewrite) → none added. ✓

**Placeholder scan:** No TBD/TODO; every code step has literal code. Integration steps that must adapt to the live file (exact keybinding-context form, `cx.stop_propagation` signature, `Author` fields) are flagged explicitly with how to resolve them by reading neighbouring code — not left vague.

**Type consistency:** `Store` field/method names match across Tasks 1/3/4/5. PR numbers are `u64` everywhere (`PrSummary.number`, `pinned_prs: Vec<u64>`, `toggle_pinned_pr(_, u64)`, `order_prs(_, _, &[u64])`). `home_rows`/`order_prs`/`HomeRow` signatures match their call sites. `palette_pr_row`'s new 5-arg signature matches its single call site (Task 5 Step 4).
