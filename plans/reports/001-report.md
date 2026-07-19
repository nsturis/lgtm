# Report: Plan 001 — Soft-wrap logical-line correctness

Branch: `custom`. File touched: `crates/app/src/main.rs` only (as scoped).

The plan's cited line numbers (against `16b682e`) matched the current tree almost
exactly — `is_comment_row` (395), `wrap_rows` (640), `selection_text` (905),
`minimap_rows` (1956), `selection_info` (3875), `comment_anchor` (4255), and the
test anchors (`copy_multibyte_slice`, `comment_anchor_resolves_sides_and_skips_non_lines`,
`selection_info_resolves_anchor_and_text`, `minimap_rows_unified_kinds_and_fracs`,
`minimap_rows_split_pairs_and_gaps`, `minimap_line_frac_caps`) were all found by name
with no material drift. No adaptation of the plan's design was needed; one clippy
lint (`unnecessary_map_or`) required a mechanical substitution (see C0 below).

## Step 0 — `is_continuation_row`

Added verbatim as specified, next to `is_comment_row`, with one change: clippy's
`unnecessary_map_or` lint (new in this toolchain) flagged `l.map_or(true, |c| c.no == 0)`;
changed both split-arm calls to `l.is_none_or(...)` / `r.is_none_or(...)` (same
semantics) to keep clippy output unchanged from baseline.

## C1 — `selection_text` (copy/AI-chat spurious newlines)

**TDD:**
- RED: added `copy_reassembles_wrapped_line_without_interior_newline` (near
  `copy_multibyte_slice`, main.rs ~9766) covering: (a) a two-row unified selection
  where row 1 is a continuation (`old_no: None, new_no: None`) must concatenate
  with no `\n`; (b) a subsequent *fresh* logical line after a continuation must
  still get its `\n`; (c) a split-view continuation cell (`no == 0`) on the locked
  side must also concatenate with no separator. Ran `cargo test -p lgtm
  copy_reassembles...` → failed as expected:
  `left: "first part of a \nwrapped line"` vs `right: "first part of a wrapped line"`.
- GREEN: rewrote `selection_text` to build the string incrementally, pushing `\n`
  only when `wrote_any && !is_continuation_row(&rows[ix])`, matching the plan's
  exact rewrite. Re-ran → all 3 assertions pass; full `copy_*` and `selection_*`
  test groups pass (4 + 4 tests).
- `selection_info` reuses `selection_text` unchanged (confirmed — no duplicate
  logic), so it inherits the fix automatically.

## C2 — `comment_anchor` / `selection_info` (no==0 continuation cells)

**TDD:**
- RED: added `comment_anchor_refuses_continuation_split_cells` (near
  `comment_anchor_resolves_sides_and_skips_non_lines`) — a `Row::SplitLine` whose
  left/right cells both carry `no == 0`. Ran the test → failed:
  `left: Some((Left, 0))` vs `right: None`.
- GREEN: guarded both split arms of `comment_anchor` with `(c.no != 0).then_some(...)`,
  exactly as specified. Re-ran → passes.
- `selection_info`: found the same bug in the `lo`/`hi` line-range derivation — the
  split arms did `left.as_ref().map(|c| c.no)` with no `no == 0` guard, which could
  drag `lo` down to 0. The unified arm (`new_no.or(*old_no)`) was already correct
  since both are `None` on a continuation. Fixed by appending
  `.filter(|&no| no != 0)` to both split arms (minimal, matches existing structure —
  no `min`/`max` restructuring needed). No new test was strictly required to
  reproduce this in isolation (no existing fixture produces a `no == 0` cell inside
  a real selection range), but the fix is a direct, mechanical extension of the
  `comment_anchor` guard and is exercised transitively by
  `selection_info_resolves_anchor_and_text`, which still passes unchanged.

## C3 — minimap over-weighting (done, not deferred)

Determined this was a **clean, localized change** that satisfies the escape hatch's
own preferred option: keep minimap tick indexing in visual-row space (unchanged)
while reducing only the *visual weight*. Verified `minimap_scrub_to` (main.rs ~5524)
computes `total = data.rows.len()` — i.e. it already indexes in raw `Row` /
visual-row space, not `minimap_rows()` output space — and `minimap_scale` only
consumes that count. So collapsing a continuation row's *kind* to `Blank` (without
changing `minimap_rows`'s output length) touches zero viewport/scrub math.

Implementation: added a match guard `_ if is_continuation_row(row) => Blank` as the
first arm in `minimap_rows`'s closure, before the existing arms. The array stays
1:1 with `rows` (`mm.len() == rows.len()` still holds); continuation rows just paint
as empty ticks instead of colored ones.

Test added: `minimap_rows_blanks_continuations_but_keeps_index_alignment` (near
`minimap_line_frac_caps`), covering both a unified continuation row and a split
`no == 0` continuation cell, asserting `mm.len() == rows.len()` and that the
continuation row/cell maps to `MinimapKind::Blank`. Passes, along with the two
pre-existing minimap tests (`minimap_rows_unified_kinds_and_fracs`,
`minimap_rows_split_pairs_and_gaps`) unmodified.

## Files touched

- `crates/app/src/main.rs` — the only file changed, all edits inside the scoped
  functions (`is_continuation_row` added, `selection_text`, `selection_info`,
  `comment_anchor`, `minimap_rows` modified) plus 3 new unit tests.

## Verification results

- `cargo test -p lgtm`: **82 passed; 0 failed; 3 ignored** (full suite, includes
  the 3 new tests: `copy_reassembles_wrapped_line_without_interior_newline`,
  `comment_anchor_refuses_continuation_split_cells`,
  `minimap_rows_blanks_continuations_but_keeps_index_alignment`).
- `cargo clippy -p lgtm --all-targets`: baseline (stashed, pre-change) vs after —
  same warning count (27 `warning:` headers in both), diffed line-by-line: every
  difference is a pure line-number shift from inserted code; no new warning text.
  One transient new warning (`unnecessary_map_or` on the two `map_or(true, ...)`
  calls in `is_continuation_row`) was fixed by switching to `is_none_or` before
  the final comparison.
- `cargo build -p lgtm`: compiles clean; only the 2 pre-existing warnings
  (`PaletteStep::Sources` never constructed, `Store::is_pinned_repo`/`is_pinned_pr`
  never used) remain — both pre-date this change.

## Concerns

None outstanding. `selection_info`'s split-arm fix has no dedicated new unit test
in isolation (only covered transitively via the unchanged
`selection_info_resolves_anchor_and_text` test, which doesn't exercise a `no == 0`
cell within a selection), since the plan didn't call for a fixture producing one
and building one would have meant hand-rolling wrapped split rows outside
`build_rows`. The fix is a 6-token, symmetric extension of the same guard already
proven correct in `comment_anchor` and `is_continuation_row`, so risk is low, but
flagging it as the one place a test wasn't added per-function as literally as the
other two.

## Done-criteria self-check

- [x] `cargo test -p lgtm` passes, including new selection + comment_anchor tests.
- [x] `cargo clippy -p lgtm` shows no NEW warnings vs. baseline.
- [x] Copy of a selection spanning a wrapped line reassembles the original line
      (verified by test).
- [x] `comment_anchor` returns `None` for a `no == 0` split cell (verified by test).
- [x] C3 implemented with a passing minimap test (not deferred).
