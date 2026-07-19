# Follow-up plan: true soft-wrap for long diff lines

## Status
Deferred. This branch (`fix/highlighting-sidebar-wrapping`) ships **reliable
horizontal scroll** so long lines are always reachable:

- **Unified view** already scrolls horizontally (the diff `uniform_list` uses
  `ListHorizontalSizingBehavior::Unconstrained` + a tracked scroll handle).
- **Split view** previously hard-clipped long lines (`overflow_hidden` cells
  under `FitList`, no horizontal scroll). Each split cell's *code column* is now
  an `overflow_x_scroll` container with a stable per-`(row, side)` id, so gpui
  persists each line's horizontal offset and the trackpad/wheel reveals the rest.
  The line-number + marker gutter stays fixed.

True soft-wrap (long lines flowing onto multiple visual rows, no horizontal
scroll) is a **separate, larger change** and is intentionally NOT in this branch.

## Why true soft-wrap is hard here
The diff pane is built on gpui's `uniform_list`, which **requires every row to be
the same fixed height** (`ROW_HEIGHT = 22.0`). Soft-wrapping makes a row's height
depend on its content and the current pane width — variable height — which
`uniform_list` cannot express. `ROW_HEIGHT` is also hardwired into geometry all
over `crates/app/src/main.rs`:

- `pane_text_hit` / `pane_hit` — mouse position → (row, col) for text selection
  (`y / ROW_HEIGHT`, plus monospace `char_width` column math).
- `scroll_to_row` / center-on-row math (`row * ROW_HEIGHT`).
- Minimap: row→bar mapping, viewport rectangle, and scrub hit-testing
  (`render_minimap`, `minimap_scrub_to`) all assume uniform rows.
- The hover "+" affordance and comment overlays position by `row_ix * ROW_HEIGHT`.
- Context-expansion scroll compensation (`inserted as f32 * ROW_HEIGHT`).

So wrapping isn't a local render tweak — it invalidates the row model that hit
testing, scrolling, and the minimap are all built on.

## Proposed approach (when picked up)
Gate wrapping behind a toggle (suggested key: `w`, matching `m`/`c`) so the fast
uniform path stays the default.

1. **Row model → visual rows.** Introduce a wrapped-layout pass that, for a given
   pane content-width and `char_width`, expands each logical `Row` into 1..N
   *visual* rows and records, per logical row, its visual-row offset and span.
   Wrap on char-cell boundaries (monospace) so column math stays arithmetic;
   optionally prefer whitespace break points like `wrap_body` already does for
   comment bodies.
2. **List primitive.** Replace `uniform_list` with gpui's variable-height
   `list()` (`ListState`) for the wrapped mode, or keep `uniform_list` but make
   its unit a *visual* row (uniform height, N visual rows per logical row). The
   latter preserves the fixed-height fast path and is likely the smaller change:
   the list length becomes `total_visual_rows`, and each index maps back to
   `(logical_row, wrap_segment)`.
3. **Hit testing.** `pane_text_hit` maps `y → visual_row`, then
   `(logical_row, segment)`; `col` becomes `segment_start + x/char_width`. Add a
   reverse map (logical → first visual row) for `scroll_to_row`.
4. **Minimap.** Drive it from logical rows (unchanged density) but translate the
   viewport rectangle through the logical→visual offset table so the highlighted
   window lines up; scrub maps visual-y back to a logical row.
5. **Overlays.** Recompute hover "+" and comment-thread y from the visual-row
   offset of their logical row.
6. **Selection across wraps.** A selected logical line may span several visual
   rows; `render_row`/`row_selection_range` must clip the selection per visual
   segment.

## Test coverage to add
- Wrap layout: a logical row of width W at pane width P → expected segment count
  and per-segment byte ranges (incl. multibyte/tab handling).
- Round-trip: `logical → first_visual → hit_test(mid of a wrapped segment)` →
  original `(row, col)`.
- Minimap viewport rectangle for a diff containing wrapped rows.

## Rough size
Medium-large: new layout module + reworked `pane_text_hit`, `scroll_to_row`,
minimap math, and selection clipping, all behind the `w` toggle. Est. a focused
day-plus with the tests above as the success criteria.
