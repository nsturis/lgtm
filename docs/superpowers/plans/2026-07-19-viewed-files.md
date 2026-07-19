# Plan — Mark-as-viewed + richer file tree

> REQUIRED SUB-SKILL for execution: subagent-driven-development. Steps use `- [ ]`.

**Base:** `42d652a`. **Files:** `crates/app/src/store.rs`, `crates/app/src/main.rs`.
**Verify:** `cargo test -p lgtm`, `cargo clippy -p lgtm` (no new warnings), `cargo build -p lgtm`.
Spec: `docs/superpowers/specs/2026-07-19-viewed-files-design.md`.

**Goal:** Track which files the reviewer has viewed (GitHub-style): manual toggle,
persisted per-review, auto-reset when a file's diff changes; viewed files collapse
to their header in the diff and are skipped by `]`/`[`; the tree marks them ✓.

## Task 1 — store: persist viewed files

- [ ] In `crates/app/src/store.rs`, add to `Store`:
```rust
    /// Reviewed files: review id → (path → content signature). A file is viewed
    /// only while its current signature matches, so a changed file auto-resets.
    #[serde(default)]
    pub viewed: std::collections::BTreeMap<String, std::collections::BTreeMap<String, u64>>,
```
- [ ] Add methods:
```rust
    pub fn is_viewed(&self, review: &str, path: &str, sig: u64) -> bool {
        self.viewed.get(review).and_then(|m| m.get(path)).is_some_and(|s| *s == sig)
    }
    /// Mark/unmark `path` viewed at signature `sig`; drops empty maps.
    pub fn set_viewed(&mut self, review: &str, path: &str, sig: u64, viewed: bool) {
        if viewed {
            self.viewed.entry(review.to_string()).or_default().insert(path.to_string(), sig);
        } else if let Some(m) = self.viewed.get_mut(review) {
            m.remove(path);
            if m.is_empty() { self.viewed.remove(review); }
        }
    }
```
- [ ] Tests (temp path, like existing store tests): set→is_viewed true; changed sig→false; unset→removed & empty map dropped; round-trips through disk.
- [ ] `cargo test -p lgtm store::` passes. Commit.

## Task 2 — signature + review id + resolve on load

- [ ] In `main.rs` add pure helpers near `build_rows`:
```rust
    /// Stable signature of one file's diff: hashes path + each hunk's line ranges
    /// and row texts. Changes iff the file's diff changes.
    fn file_signature(file: &FileDiff) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        file.display_path().hash(&mut h);
        for hunk in &file.hunks {
            (hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count).hash(&mut h);
            for row in &hunk.rows {
                match row {
                    DiffRow::Context { text, .. } | DiffRow::Added { text, .. }
                    | DiffRow::Removed { text, .. } => text.hash(&mut h),
                }
            }
        }
        h.finish()
    }
```
  (Read the real `FileDiff`/`DiffRow` shapes in `crates/diff-core/src/lib.rs` and match field names.)
- [ ] Add a review-id helper: for `Source::Pr(loc)` → `loc.repo_slug() + "#" + number`; for
  `Source::Local(src)` → `"<repo_root display>@<base_label>"`. Put it where `Source` is in scope.
- [ ] `ItemData` gains: `viewed: HashSet<usize>` and `review_id: String`. Populate in the
  `ItemData { .. }` constructor: compute `review_id`, then for each `(ix, file)` in
  `diff.files`, if `store.is_viewed(&review_id, file.display_path(), file_signature(file))`
  insert `ix`. `ItemData` construction doesn't have `store` — pass the resolved `viewed`
  set + `review_id` in from the caller (`spawn_fetch`'s install path has `app.store`), OR
  add a post-construction `data.resolve_viewed(&store)` the caller invokes. Prefer the
  latter (a method on ItemData taking `&Store`), called right after the item is installed
  and on refresh.
- [ ] `cargo build -p lgtm`. Commit.

## Task 3 — collapse viewed files in the diff

- [ ] In `build_rows`, thread the viewed set in. `build_rows` is a free fn with ~28 callers
  (mostly tests) — DO NOT change its public signature. Instead: `build_rows` reads viewed
  via a new parameter added to the PRODUCTION wrapper only (there is a `build_rows_cached`
  used by the 6 production sites from an earlier perf change — add a `viewed: &HashSet<usize>`
  param there and pass it down, defaulting to empty for the test-facing `build_rows`). Read
  how `build_rows`/`build_rows_cached`/`build_rows_impl` are structured first.
- [ ] When a file's `file_ix` is in `viewed`: push its `Row::FileHeader` (mark it viewed —
  add a `viewed: bool` field to `Row::FileHeader`, default false) and then `continue` —
  skip its hunks, gaps, and comment rows. (Binary/again files: still just the header.)
- [ ] `render_row` `FileHeader` arm: when `viewed`, render a ✓ + dim style, and its
  `on_click` toggles viewed off (re-expand) via a new `toggle_viewed(file_ix)` method.
- [ ] Add `fn toggle_viewed(&mut self, file_ix, cx)`: flip membership in
  `active_data().viewed`, persist via `store.set_viewed(review_id, path, sig, now_viewed)` +
  `save_store`, then `rebuild_rows_anchored()` + rebuild tree + notify.
- [ ] Test: `build_rows` with a viewed file emits only its header (no hunk rows). Commit.

## Task 4 — `shift-v` toggle + skip viewed in `]`/`[`

- [ ] Add action `ToggleViewed`, keybinding `KeyBinding::new("shift-v", ToggleViewed, Some("ReviewApp"))`,
  and a handler that toggles the file currently at the top of the viewport. Reuse the
  current-file computation the sidebar already does (search where `render_sidebar` derives
  `current_file` from the scroll offset / `file_rows`); extract it to a
  `fn active_file_ix(&self) -> Option<usize>` if not already available.
- [ ] `NextFile`/`PrevFile` handlers (search `jump_next`/`file_rows`): filter the target
  file rows to those whose file is NOT viewed. If the filtered list is empty (all viewed),
  fall back to the full list (don't get stuck / panic).
- [ ] Test the next-unviewed selection logic as a pure function (given file rows + viewed
  set + current position → next target), incl. the all-viewed fallback.
- [ ] `cargo build -p lgtm` + `cargo test -p lgtm`. Commit.

## Task 5 — richer tree row (✓ + counts + comment dot)

- [ ] `render_tree_row` (File arm): add a leading ✓ hit-target that toggles viewed for that
  file (calls `toggle_viewed`), shown filled/dim when viewed; dim the whole row when viewed.
  Add trailing `+N −M` (from `file.additions`/`deletions`) and a comment dot when the file
  has comments (from `CommentIndex.counts`, already available to the tree render). These
  extra visuals are trim-able — keep the ✓ as the must-have.
- [ ] `cargo build -p lgtm`; `cargo clippy -p lgtm` no new warnings; `cargo test -p lgtm`.
  Commit.

## Scope / boundaries
- `store.rs` + `main.rs` only. Don't change `wrap_rows`/soft-wrap, the perf caching
  contract (base_rows), or hit-testing beyond nav.
- Viewed collapse must keep fixed row heights (header row is one row) so minimap/scroll math
  stays valid.

## Done criteria
- Toggling `shift-v` / tree ✓ collapses the file to its header, persists across restart, and
  auto-resets when that file's diff changes (tested via signature).
- `]`/`[` skip viewed files; all-viewed doesn't hang.
- `cargo test -p lgtm` green; `cargo clippy -p lgtm` no new warnings.
