# Plan 002 — Diff rebuild performance (P1, P2)

Written against `16b682e`. File: `crates/app/src/main.rs`. Verify with
`cargo test -p lgtm`, `cargo clippy -p lgtm`, `cargo build -p lgtm`.

## Background

`Render::render` calls `apply_wrap` **every frame** (main.rs ~8298). When the pane
width changes (window/sidebar resize), the derived wrap column count changes, and
`apply_wrap` calls `data.rebuild_rows_anchored()` — which re-runs `build_rows` over
the **entire diff**, including tree-sitter re-highlighting of every non-upgraded
hunk (`hunk_syntax` → `syntax::highlight_lines`, main.rs ~967), then re-wraps and
rebuilds the minimap. During a drag the column count changes many times → many
full rebuilds → visible stutter on large diffs.

Wrap width changes only require **re-wrapping**, not re-**building** (the logical
rows and their syntax spans are identical; only the wrap boundaries move).

## P1 — cache the unwrapped rows; on width change, re-wrap only (do this first)

Design: `set_rows` already receives the freshly-built, **unwrapped** rows and then
wraps them. Store the unwrapped rows so a width-only change can re-wrap from the
cache and skip `build_rows`/tree-sitter entirely.

- [ ] Read `set_rows` (main.rs:2408), `rebuild_rows_anchored` (main.rs:2420),
  `apply_wrap` (main.rs ~4402), and the `ItemData` struct (main.rs:2334) first.

- [ ] Add fields to `ItemData` (near `rows`/`file_rows`/`hunk_rows`):
  ```rust
  /// Unwrapped rows as built by build_rows, cached so a wrap-width change
  /// re-wraps without re-running build_rows + tree-sitter.
  base_rows: Vec<Row>,
  base_file_rows: Vec<usize>,
  base_hunk_rows: Vec<usize>,
  ```
  Initialise them in the `ItemData { .. }` construction (main.rs ~2688): set them
  to the same `rows`/`file_rows`/`hunk_rows` values used there (clone the vecs, or
  reorder construction so the base is stored then wrapped — simplest: `base_rows:
  rows.clone()` etc. at construction, since wrap_cols starts 0 so display == base).

- [ ] Change `set_rows` so the incoming (built, unwrapped) triple is stored as the
  base, then the display rows are derived from it:
  ```rust
  fn set_rows(&mut self, built: (Vec<Row>, Vec<usize>, Vec<usize>)) {
      self.base_rows = built.0;
      self.base_file_rows = built.1;
      self.base_hunk_rows = built.2;
      self.rewrap();
  }

  /// Derive the displayed rows from the cached base rows at the current
  /// wrap_cols. Cheap: no build_rows, no tree-sitter — just wrap + minimap.
  fn rewrap(&mut self) {
      let (rows, file_rows, hunk_rows) = if self.wrap_cols > 0 {
          wrap_rows(self.base_rows.clone(), self.wrap_cols)
      } else {
          (self.base_rows.clone(), self.base_file_rows.clone(), self.base_hunk_rows.clone())
      };
      self.minimap = minimap_rows(&rows);
      self.minimap_cache.replace(None);
      self.rows = rows;
      self.file_rows = file_rows;
      self.hunk_rows = hunk_rows;
  }
  ```
  (`Row` derives/needs `Clone`; `Cell` is already `Clone`. If `Row` is not `Clone`,
  add `#[derive(Clone)]` to `Row` and any of its field types that lack it —
  check and note this.)

- [ ] Change `apply_wrap` (main.rs ~4402): when only `wrap_cols` changed, call the
  cheap re-wrap path instead of a full rebuild. Replace the
  `data.wrap_cols = target; data.rebuild_rows_anchored();` block with:
  ```rust
  data.wrap_cols = target;
  data.rewrap();
  cx.notify();
  ```
  Note: `rebuild_rows_anchored` preserves scroll anchor across a rebuild; `rewrap`
  does not re-anchor. Wrap width changes should keep the top row stable — if the
  simple `rewrap` visibly jumps the scroll position on resize, wrap the rewrap in
  the same anchor-save/restore `rebuild_rows_anchored` uses (read that fn:
  save top_row/frac before, restore offset after). Prefer reusing that anchoring;
  only skip it if it proves unnecessary.

- [ ] Confirm every existing `rebuild_rows_anchored` / `set_rows` caller still works:
  a real diff change (comment toggle, gap expand, upgrade install, refresh) must go
  through `build_rows` → `set_rows` (which now refreshes the base). Only pure
  width changes use `rewrap`. Grep all callers and verify.

- [ ] **Verify:** `cargo test -p lgtm` passes. Manually reason through: resize →
  `apply_wrap` → `rewrap` (no `build_rows`); comment toggle → `rebuild_rows_anchored`
  → `build_rows` → `set_rows` → base refreshed. Report both paths in your notes.

## P2 — memoize hunk syntax highlighting (second, optional)

Even with P1, `build_rows` still re-highlights every non-upgraded hunk each time it
runs (initial load, comment toggle, gap expand, upgrade install). Memoize it.

- [ ] Read `hunk_syntax` (main.rs ~940-975) and its call in `build_rows` (main.rs ~1193).
- [ ] Add a cache on `ItemData` keyed by `(file_ix, hunk_ix)` →
  `Vec<Vec<(Range<usize>, syntax::Token)>>` (the per-row spans hunk_syntax returns).
  Populate on first compute, reuse on subsequent `build_rows`. Invalidate (clear the
  cache) when the diff changes: on refresh and when an upgrade is installed (those
  already rebuild — clear there). Because `build_rows` is a free function, thread the
  cache in as a parameter, or move the memo into a small wrapper the ItemData owns.
- [ ] **ESCAPE HATCH:** if wiring the cache through `build_rows`' many call sites
  (see the ~25 callers, mostly tests) forces a signature change that ripples widely,
  STOP and report — P1 already removes highlighting from the hot (resize) path, so
  P2 is optional. Do not change the `build_rows` public signature in a way that
  breaks the test call sites; prefer an ItemData-owned memo consulted inside
  `hunk_syntax` via an optional cache argument, or defer P2.
- [ ] **Test:** if implemented, a unit test that highlighting a hunk twice returns
  identical spans and (if observable) the second call hits the cache.

## Scope / boundaries

- In scope: `ItemData` fields + `set_rows`/`rewrap`/`apply_wrap` (P1); `hunk_syntax`
  memo (P2). `crates/app/src/main.rs` only.
- Out of scope: `wrap_rows`/`wrap_line_spans` logic, the renderer, the background
  upgrade pool, `Arc`-ifying row text (PERF-04 — separate, not in this plan).
- Do not change scroll/hit-test math except the anchor reuse noted in P1.

## Done criteria

- `cargo test -p lgtm` and `cargo clippy -p lgtm` clean (no new warnings).
- A width-only change re-wraps without calling `build_rows` (trace it in your report;
  optionally add a debug assertion/counter during dev, removed before commit).
- Scroll position stays stable across a resize (no jump).
- P2 either implemented with a test, or deferred with a one-line reason.
