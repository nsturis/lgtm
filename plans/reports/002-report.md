# Plan 002 report — Diff rebuild performance (P1, P2)

Branch: `custom`. File touched: `crates/app/src/main.rs` only, as scoped.

## P1 — cache the unwrapped rows; re-wrap only on width change

Implemented essentially as specced, with one deliberate deviation on the
anchor mechanism (explained below).

- `Row` now derives `Clone` (it didn't before). No other type needed a new
  derive: `Cell` was already `Clone`; every field type inside `Row`/`Cell`
  (`SharedString`, `LineKind`, `FileStatus`, `CommentSide`, `syntax::Token`,
  `Range<usize>`) was already `Clone`/`Copy`.
- `ItemData` gained three fields: `base_rows: Vec<Row>`, `base_file_rows:
  Vec<usize>`, `base_hunk_rows: Vec<usize>` — the unwrapped rows as last
  produced by `build_rows`.
- `set_rows` now stores its incoming `(Vec<Row>, Vec<usize>, Vec<usize>)`
  into `base_rows`/`base_file_rows`/`base_hunk_rows` and calls a new
  `rewrap()`, which does exactly what `set_rows` used to do inline: wrap
  `base_rows.clone()` at `wrap_cols` (or pass the base through unchanged if
  `wrap_cols == 0`) and refresh `rows`/`file_rows`/`hunk_rows`/`minimap`.
  `rewrap()` never touches `build_rows` or `hunk_syntax` — it only calls
  `wrap_rows` (pure, already-computed spans/text) and `minimap_rows`.
- The fresh-`ItemData` construction path (`ReviewItem::install`, the
  `_ =>` / not-yet-`Ready` branch) sets `base_rows`/`base_file_rows`/
  `base_hunk_rows` to clones of the initial `rows`/`file_rows`/`hunk_rows`
  (consistent with `wrap_cols: 0` at construction, so base == display).
- `apply_wrap` (main.rs, `fn apply_wrap`) now calls `data.rewrap_anchored()`
  instead of `data.rebuild_rows_anchored()` when only `wrap_cols` changed.

### Anchor deviation from the plan's pseudocode

The plan's pseudocode has `rewrap()` itself be side-effect-free (no scroll
touch), then suggests "wrap the rewrap in the same anchor-save/restore
`rebuild_rows_anchored` uses" if a plain rewrap jumps the scroll. I kept
`rewrap()` pure (matches the pseudocode) and added a **separate** method,
`rewrap_anchored()`, that:

1. Saves `top_row`/`frac` from the current scroll offset (same as
   `rebuild_rows_anchored`).
2. Computes `top_base = count of non-continuation rows before top_row` —
   i.e. the rank of the logical (unwrapped) line currently at the top,
   using the existing `is_continuation_row` helper (already used by the
   minimap and hit-testing to detect a wrap continuation segment).
3. Calls `self.rewrap()`.
4. Finds `new_top` = the display-row index of the `top_base`-th logical-line
   start in the *new* rows, via a new helper `nth_row_start` (a direct
   mirror of the existing `nth_noncomment_row`, just keyed on
   `!is_continuation_row` instead of `!is_comment_row`).
5. Restores the scroll offset to `new_top * ROW_HEIGHT + frac`.

I did **not** put this anchor logic inside `rewrap()` itself, because
`rewrap()` is also called from `set_rows()`, which is used by every "real"
rebuild path (upgrade install, gap expand, view toggle, refresh) — those
paths either don't want automatic re-anchoring (upgrade install: scroll is
untouched, row count is expected to match) or already manage their own
anchor scheme with different semantics (`rebuild_rows_anchored`'s
comment-based ranking, `expand_gap`'s manual inserted-row-count shift).
Coupling continuation-based re-anchoring into the shared `rewrap()` would
have silently changed behavior at those call sites. Keeping it as a
separate `rewrap_anchored()`, called only from `apply_wrap`, confines the
new anchor math to exactly the case it's needed for (pure width changes)
without touching any other call site's behavior.

Added a unit test, `nth_row_start_survives_a_rewrap`, that builds real rows
via `build_rows`, wraps them narrow (forces continuation rows on a long
line) and wide (no wrapping), and asserts that the `top_base`-ranked
logical line resolved via `nth_row_start` is the *same* logical line (same
`old_no`/`new_no`) in both wrappings — i.e. the anchor math is correct.

### Two-path trace (the core correctness claim)

**(a) Resize → `apply_wrap` → rewrap, no `build_rows`:**
`Render::render` → `apply_wrap` computes `target` wrap-cols from pane width.
If `data.wrap_cols != target`: `data.wrap_cols = target;
data.rewrap_anchored();`. `rewrap_anchored` calls `self.rewrap()`, which
reads `self.base_rows` (already cached from the last real build) and calls
`wrap_rows(self.base_rows.clone(), self.wrap_cols)` — a pure re-segmentation
of already-computed text/spans. **`build_rows` and `hunk_syntax` are never
called on this path.**

**(b) Comment toggle / upgrade / gap expand / view toggle / refresh →
`build_rows` → `set_rows` (base refreshed):**
- `toggle_comments` / `add_local_comment` / `refetch_comments` →
  `rebuild_rows_anchored()` → `build_rows_cached(...)` → `self.set_rows(built)`.
- `toggle_view` → `build_rows_cached(...)` → `data.set_rows(built)`.
- `expand_gap` → `build_rows_cached(...)` → `data.set_rows(built)`.
- Upgrade install (background upgrade pool result handler) →
  `build_rows_cached(...)` → `data.set_rows(built)`.
- `ReviewItem::install` (refresh path, `ItemState::Ready` branch) →
  conditionally `build_rows_cached(...)` (when mode/comments changed
  mid-refresh) → `data.set_rows((rows, file_rows, hunk_rows))`.
- Fresh load (`fetch_item`, background thread, and the not-yet-`Ready`
  branch of `install`) → plain `build_rows` (no `ItemData`/cache exists yet)
  → rows placed directly into the new `ItemData`'s `base_rows` alongside
  `rows`.

Every one of these calls `set_rows`, which stores the built triple into
`base_rows`/`base_file_rows`/`base_hunk_rows` before calling `rewrap()` — so
the cache is refreshed on every real diff/content change, and only a pure
width change skips straight to `rewrap`/`rewrap_anchored` without ever
reaching `build_rows`.

I grepped every `build_rows(`/`build_rows_cached(`/`set_rows(`/
`rebuild_rows_anchored(` call site and confirmed all production sites
funnel through this. Test call sites (unaffected — see P2) build rows
directly to assert on their content, not through `ItemData`.

## P2 — memoize hunk syntax highlighting: **implemented** (not deferred)

Assessed the escape hatch first: `build_rows` has ~30 call sites, but only
about six are production code (`fetch_item`, `ReviewItem::install`,
`rebuild_rows_anchored`, upgrade install, `expand_gap`, `toggle_view`); the
rest (~28) are tests. Changing `build_rows`'s signature directly would have
rippled into all of them, so I avoided that:

- `build_rows(diff, mode, upgrades, comments, show_comments)` — **signature
  unchanged**, still used by every test call site. Now a one-line wrapper
  that calls the new `build_rows_impl(..., cache: None)`.
- `build_rows_cached(diff, mode, upgrades, comments, show_comments, cache:
  &mut HunkSyntaxCache)` — new function, used only by the six production
  call sites listed above (all switched over).
- `build_rows_impl(..., cache: Option<&mut HunkSyntaxCache>)` — the actual
  body (previously `build_rows`'s body), unchanged except at the
  `hunk_syntax` call site: for non-upgraded hunks, it now does
  `cache.entry((file_ix, hunk_ix)).or_insert_with(|| hunk_syntax(lang,
  &hunk.rows)).clone()` when a cache is present, falling straight through to
  `hunk_syntax(...)` when it isn't (the `build_rows`/tests path).
- `HunkSyntaxCache = HashMap<(usize, usize), Vec<Vec<(Range<usize>,
  syntax::Token)>>>`, a new `ItemData` field `hunk_syntax_cache`, empty at
  construction.

**Invalidation:** cleared once, in `ReviewItem::install`'s refresh branch,
right alongside the existing `data.upgrades.clear()` — the diff is being
replaced there, so `(file_ix, hunk_ix)` keys would otherwise resolve to
stale spans for different content. Upgrade install does **not** need to
clear anything: an upgraded file's hunks take the `Some(upgrade) =>
upgrade.row_spans(...)` branch from then on, so its (now-stale)
`hunk_syntax_cache` entries are simply never read again — not a
correctness issue, just inert. Gap expand and view toggle need no
invalidation either (hunk content and `hunk_syntax`'s output are
mode-independent).

Net effect: comment toggle, gap expand, and view toggle — which previously
re-ran tree-sitter over every non-upgraded hunk in the whole diff on every
call — now only compute `hunk_syntax` once per hunk for the lifetime of the
current diff (until the next refresh).

Added a unit test, `build_rows_cached_reuses_hunk_syntax_across_calls`: runs
`build_rows_cached` once (asserts a cache entry appears and highlighting is
real, mirroring `build_rows_emits_syntax_spans`), tampers the cached entry
with a sentinel value, runs `build_rows_cached` again on the same diff, and
asserts the returned rows' `syntax` fields equal the sentinel verbatim
(proving the second call actually consulted the cache instead of
recomputing) and that the cache didn't grow (no wasted allocation on a hit).

## Row `Clone`

Only `Row` needed a new `#[derive(Clone)]`. Everything it (transitively)
contains was already `Clone`/`Copy`: `Cell` (`#[derive(Clone)]`),
`SharedString` (gpui), `LineKind`/`FileStatus`/`CommentSide`/`syntax::Token`
(all `Clone, Copy`), `Range<usize>` (std). No other type changes were
needed.

## Verification

- `cargo build -p lgtm`: compiles clean.
- `cargo test -p lgtm`: **84 passed**, 0 failed, 3 ignored (baseline was 82;
  added `nth_row_start_survives_a_rewrap` and
  `build_rows_cached_reuses_hunk_syntax_across_calls`).
- `cargo clippy -p lgtm`: **11 warnings, identical set to the pre-change
  baseline** (diffed both runs line-by-line) — no new warnings introduced.

## Concerns / notes

- `rewrap()` and `rewrap_anchored()` both `clone()` `base_rows` on every
  call. This is the intentional tradeoff: cloning `Vec<Row>` (mostly
  `SharedString`/small `Vec`s, cheap to clone) is far cheaper than
  re-running `build_rows` + tree-sitter, but it's not free for very large
  diffs. Out of scope per the plan (PERF-04, `Arc`-ifying row text, is a
  separate follow-up).
- The `hunk_syntax_cache` is never trimmed for files that get upgraded —
  as noted above this is harmless (dead entries, not stale reads) but does
  mean memory isn't reclaimed until the next refresh clears the whole map.
  Given P2 is a bonus/optional item, I didn't add per-file eviction on
  upgrade to keep the change minimal.
- No dev-only counters/instrumentation were left in; verification was done
  by code-path tracing (above) plus the two new unit tests, per the plan's
  "optionally add a debug counter... removed before commit" guidance (I
  skipped adding one since the trace + tests already pin down the
  invariant).
