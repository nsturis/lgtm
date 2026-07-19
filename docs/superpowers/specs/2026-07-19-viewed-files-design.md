# Mark-as-viewed + richer file tree — design

**Date:** 2026-07-19 · **Base:** `7a2e175` · **Branch:** `custom`

## Problem
Reviewing a large PR, there's no way to track which files you've already looked
at. Every file stays fully expanded and `]`/`[` step through all of them, so you
re-scan work you've finished. GitHub's "viewed" checkbox solves this.

## Decisions (from brainstorming)
- **Manual toggle, persisted per-PR, auto-reset on file change** (GitHub-style).
- **Payoff = collapse + skip:** a viewed file collapses to its header in the diff;
  `]`/`[` skip to the next/prev **unviewed** file; the tree marks viewed rows ✓ and dims them.
- Richer tree also shows per-file `+/−` counts and a comment dot (trim-able).

## Persistence (`crates/app/src/store.rs`)
Add to `Store`:
```rust
/// Reviewed files per review id → (path → content signature). A file counts as
/// viewed only while its current signature matches, so new commits that touch a
/// file auto-reset just that file.
#[serde(default)]
pub viewed: BTreeMap<String, BTreeMap<String, u64>>,
```
- **Review id:** PR → `"owner/repo#number"`; local → `"<repo_root>@<base_label>"`.
- **Signature:** a stable `u64` hash of the file's hunks (paths + line numbers +
  text). Recomputed from the current diff; a change to the file's diff → new
  signature → viewed resets for that file only.
- Methods: `is_viewed(review_id, path, sig) -> bool`, `set_viewed(review_id, path, sig, bool)`
  (insert/remove; drop empty inner maps). Best-effort save as today.

## App state + interaction (`main.rs`)
- `ItemData` gains `viewed: HashSet<usize /*file_ix*/>`, resolved at load: for each
  file, compute its signature, look it up in the store, insert file_ix if viewed.
  Store the review id + per-file signatures on `ItemData` for toggling.
- Toggle:
  - Key **`shift-v`** toggles the file at the top of the viewport (the app already
    computes the current file for tree-follow — reuse it).
  - **✓ click** in the tree row toggles that file.
  - On toggle: update `ItemData.viewed`, persist to store, `rebuild_rows_anchored()`
    + rebuild tree, `cx.notify()`.

## Payoff
- **Collapse (`build_rows`):** when a file's `file_ix ∈ viewed`, emit only its
  `Row::FileHeader` (skip hunks/gaps/comment rows). The header renders with a
  "viewed" style (dim + ✓). Clicking a viewed header un-views it (re-expands).
- **Nav:** `NextFile`/`PrevFile` skip file rows whose file is viewed; if all are
  viewed, fall back to normal behavior (don't get stuck).
- **Tree (`render_tree_row`):** ✓ marker + dim on viewed file rows; a ✓ hit-target
  toggles. Add `+N −M` counts and a comment dot (from `CommentIndex` counts) —
  trim-able.

## Testing
Unit-test the pure parts: signature stability/change-detection, viewed-resolve
from store, `build_rows` collapse for a viewed file, and next-unviewed nav
(skips viewed, doesn't loop forever when all viewed). Tree visuals + toggle
verified by screenshot.

## Scope / non-goals
- No auto-marking; no cross-file "view all"; no server sync (GitHub's viewed
  state isn't exposed via `gh` cleanly — local tracking only).
- Local items also get viewed tracking (keyed by repo_root+base), same mechanism.
