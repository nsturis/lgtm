# Plan 001 — Soft-wrap logical-line correctness (C1, C2, C3)

Written against `16b682e`. File: `crates/app/src/main.rs` only. Language: Rust.
Verify with `cargo test -p lgtm` and `cargo clippy -p lgtm` (no new warnings).

## Background

The diff pane soft-wraps long lines: a single logical code line becomes several
fixed-height **visual rows**. `wrap_rows` (~main.rs:640) builds them, marking
continuation segments so the renderer blanks their gutter:

- **Unified** (`Row::Line`): a continuation row has `old_no == None && new_no == None`.
- **Split** (`Row::SplitLine`): a continuation cell has `Cell.no == 0` (a sentinel;
  real line numbers are 1-based).

Three downstream consumers wrongly treat visual rows as logical lines. Fix all three,
ideally via one shared helper.

## Step 0 — add a shared continuation predicate

Add near `is_comment_row` (main.rs:395):

```rust
/// True when `row` is a soft-wrap continuation (a later visual segment of a
/// logical line), as marked by `wrap_rows`: a unified line with both numbers
/// cleared, or a split row whose present cell(s) carry the no==0 sentinel.
fn is_continuation_row(row: &Row) -> bool {
    match row {
        Row::Line { old_no: None, new_no: None, .. } => true,
        Row::SplitLine { left, right } => {
            let l = left.as_ref();
            let r = right.as_ref();
            // At least one present cell, and every present cell is a continuation.
            (l.is_some() || r.is_some())
                && l.map_or(true, |c| c.no == 0)
                && r.map_or(true, |c| c.no == 0)
        }
        _ => false,
    }
}
```

- [ ] Add the function. Run `cargo build -p lgtm` (unused-fn warning is expected until later steps use it).

## Step 1 — C1: stop copy/chat from inserting newlines at wrap boundaries

**Current** `selection_text` (main.rs:905) joins every visual row with `"\n"`:

```rust
fn selection_text(sel: &Selection, rows: &[Row]) -> String {
    let (start, end) = sel.ordered();
    let mut parts = Vec::new();
    for ix in start.row..=end.row.min(rows.len().saturating_sub(1)) {
        if let Some(range) = row_selection_range(sel, ix, &rows[ix]) {
            let text = row_side_text(&rows[ix], sel.side).unwrap_or_default();
            parts.push(&text[range]);
        }
    }
    parts.join("\n")
}
```

A wrapped logical line is several rows → its segments get `\n` between them →
pasted code is corrupted. Rewrite so a continuation row concatenates to the
previous segment with no separator:

```rust
fn selection_text(sel: &Selection, rows: &[Row]) -> String {
    let (start, end) = sel.ordered();
    let mut out = String::new();
    let mut wrote_any = false;
    for ix in start.row..=end.row.min(rows.len().saturating_sub(1)) {
        if let Some(range) = row_selection_range(sel, ix, &rows[ix]) {
            let text = row_side_text(&rows[ix], sel.side).unwrap_or_default();
            // A continuation row belongs to the previous row's logical line, so
            // no newline before it; a fresh logical row gets one (except first).
            if wrote_any && !is_continuation_row(&rows[ix]) {
                out.push('\n');
            }
            out.push_str(&text[range]);
            wrote_any = true;
        }
    }
    out
}
```

- [ ] Apply. `selection_info` (main.rs:3875) reuses `selection_text` for the chat, so it inherits the fix — confirm it still calls `selection_text` (don't duplicate logic).
- [ ] **Test:** add a unit test that a selection spanning a wrapped line reassembles the original text (no interior `\n`). Build a `Selection` over two rows where row 2 is a continuation (`Row::Line { old_no: None, new_no: None, .. }`) and assert `selection_text` returns the concatenation with no newline. Follow the existing selection tests near `selection_info_resolves_anchor_and_text` (main.rs:10582) for construction patterns.
- [ ] `cargo test -p lgtm selection` → passes.

## Step 2 — C2: don't anchor comments/selections to continuation cells (no==0)

**Current** `comment_anchor` (main.rs:4255) split arms read `.no` with no guard:

```rust
(Row::SplitLine { left, .. }, SelSide::Left) => {
    Some((CommentSide::Left, left.as_ref()?.no as u64))
}
(Row::SplitLine { right, .. }, SelSide::Right) => {
    Some((CommentSide::Right, right.as_ref()?.no as u64))
}
```

A `no == 0` continuation cell → a comment anchored to line 0 (GitHub rejects/misplaces).
Unified already returns `None` for continuations (`(*old_no)?`/`(*new_no)?` are None).
Guard the split arms to match:

```rust
(Row::SplitLine { left, .. }, SelSide::Left) => {
    let c = left.as_ref()?;
    (c.no != 0).then_some((CommentSide::Left, c.no as u64))
}
(Row::SplitLine { right, .. }, SelSide::Right) => {
    let c = right.as_ref()?;
    (c.no != 0).then_some((CommentSide::Right, c.no as u64))
}
```

- [ ] Apply. This makes the hover/click "+" affordance not offer a comment on a wrapped continuation row (matching unified) — desired.
- [ ] Read `selection_info` (main.rs:3875): where it derives the line range for a split selection from `left/right.no`, skip cells with `no == 0` (a `no == 0` must not drag the reported `lo` down to 0). If it uses `min`, filter out 0 first. Match the existing structure; keep it minimal.
- [ ] **Test:** extend/add near `comment_anchor_resolves_sides_and_skips_non_lines` (main.rs:10365): a `Row::SplitLine` whose cell has `no == 0` returns `None` from `comment_anchor` for that side.
- [ ] `cargo test -p lgtm comment_anchor selection_info` → passes.

## Step 3 — C3: minimap should count logical lines, not visual rows (OPTIONAL)

Each wrapped visual row currently emits one `MinimapRow` (main.rs:1956 `minimap_rows`,
called from `set_rows` main.rs:2519), so a few long lines over-weight the minimap.

**ESCAPE HATCH — read first:** the minimap's viewport rectangle and
`minimap_scrub_to` (search `minimap_scrub_to`, `minimap_scale`) currently assume
**1 minimap tick = 1 visual row** (they map scroll offset / click position in
visual-row space). If you collapse continuation rows out of `minimap_rows` without
also remapping those, the viewport highlight and scrub will point to the wrong place.

- [ ] Determine whether minimap tick indexing can be kept in **visual-row space**
  (unchanged) while only the *visual weight* is reduced — e.g. skip emitting a
  `MinimapRow` for continuation rows BUT keep index alignment by emitting a
  transparent/empty tick. If a clean, localized change (no edits to
  `minimap_scrub_to`/viewport math) achieves "continuation rows don't add a colored
  tick," do it and add a test near `minimap_rows_unified_kinds_and_fracs` (main.rs:9730).
- [ ] **If it requires reworking `minimap_scrub_to`/viewport mapping, STOP and skip C3.**
  It is cosmetic (LOW impact). Leave `minimap_rows` as-is and note in your report that
  C3 was deferred as not worth the risk. Do not partially rewire the scrub math.

## Scope / boundaries

- In scope: `crates/app/src/main.rs` only — `selection_text`, `comment_anchor`,
  `selection_info` (guard only), `minimap_rows` (optional), the new `is_continuation_row`,
  and the added tests.
- Out of scope: changing `wrap_rows`/`wrap_line_spans`/`wrap_cell` (they're correct),
  the renderer, hit-testing (`pane_text_hit`), or anything in other crates.
- Do NOT change the continuation sentinels (both-None / no==0) — consumers depend on them.

## Done criteria

- `cargo test -p lgtm` passes, including the new selection + comment_anchor tests.
- `cargo clippy -p lgtm` shows no NEW warnings vs. baseline.
- Copy of a selection spanning a wrapped line reassembles the original line (verified by test).
- `comment_anchor` returns `None` for a `no == 0` split cell (verified by test).
- C3 either implemented with a passing minimap test, or explicitly deferred with a note.
