# Mark-as-viewed + richer file tree — implementation report

Branch: `custom`. Plan: `docs/superpowers/plans/2026-07-19-viewed-files.md`. Base for
line-number references in the plan: `0053c4e`; this work was done against the current
tip of `custom` (commit `7a2e175` and later), found by name/grep rather than line number
as instructed.

## Status: DONE

All 5 tasks implemented and committed individually. `cargo build -p lgtm` clean,
`cargo test -p lgtm` green (91 passed, 0 failed, 3 pre-existing ignored), `cargo clippy -p
lgtm` (and `--all-targets`) produce zero warnings.

## Commits (on `custom`)

1. `cb8074b` feat: persist viewed files in store — Task 1
2. `695cd92` feat: file signature, review id, resolve viewed on load — Task 2
3. `3f8054c` feat: collapse viewed files in the diff pane — Task 3
4. `a42dac2` feat: shift-v toggle, skip viewed files in ]/[ — Task 4
5. `d5cc3e9` feat: tree row viewed checkbox, comment dot, dim viewed rows — Task 5

## Task 1 — store: persist viewed files

`crates/app/src/store.rs`: added `Store::viewed: BTreeMap<String, BTreeMap<String, u64>>`
(review id → path → signature), `is_viewed(review, path, sig) -> bool`, and
`set_viewed(review, path, sig, viewed)` (insert/remove, drops empty inner maps).

TDD evidence: wrote `viewed_set_check_and_unset` (set → is_viewed true; different sig →
false; unset → removed, empty inner map dropped) and `viewed_round_trips_through_disk`
before touching anything else; both pass against the new methods only, no production
code needed beyond the fields/methods themselves.

## Task 2 — signature + review id + resolve on load

`crates/app/src/main.rs`:
- `file_signature(file: &FileDiff) -> u64` — hashes display path + each hunk's
  `(old_start, old_count, new_start, new_count)` + every row's text. Verified with
  `file_signature_stable_and_changes_with_diff`: same file → same signature; different
  file → different signature; mutating a row's text or a hunk's line range → different
  signature.
- `review_id(source: &Source) -> String` — `"{owner}/{repo}#{number}"` for PRs,
  `"{repo_root display}@{base_label}"` for local. Verified with
  `review_id_distinguishes_pr_and_local_sources`, including that two local sources with
  the same repo but different `base_label` get distinct ids.
- `ItemData` gained `review_id: String` and `viewed: HashSet<usize>`, plus
  `ItemData::resolve_viewed(&mut self, store: &Store)`, called (not the `data +=
  store` constructor route mentioned as an alternative in the plan — I took the
  plan's stated preference) right after the item is installed, and again on refresh.

## Task 3 — collapse viewed files in the diff

- `Row::FileHeader` gained `file_ix: usize` (needed for the click-to-toggle handler,
  not explicitly called out by the plan but required to know which file a header
  belongs to) and `viewed: bool`.
- Threaded `viewed: &HashSet<usize>` through `build_rows_cached` → `build_rows_impl`
  only; the test-facing `build_rows` free fn keeps its original 5-arg signature and
  passes `&HashSet::new()` internally, so none of the ~28 existing test call sites
  needed touching (confirmed: they all still compile/pass unchanged).
- In `build_rows_impl`, a file whose `file_ix` is in `viewed` pushes only its
  `FileHeader` row (marked `viewed: true`) then `continue`s before the binary-row
  check and the hunks loop — binary files that are viewed also collapse to just the
  header, per the plan.
- `render_row`'s `FileHeader` arm now shows a green `✓` (blank otherwise), dims the
  whole row at `viewed`, and the row is clickable (`cursor_pointer` + `on_click`)
  calling `toggle_viewed(file_ix)` — clicking toggles either direction, not just
  un-view; the plan only specified un-view-on-click but a single toggle is simpler
  and symmetric with the tree checkbox.
- `ReviewApp::toggle_viewed(file_ix, cx)`: flips `data.viewed` membership, computes
  the file's signature, persists via `store.set_viewed(...)`, calls
  `data.rebuild_rows_anchored()` (which now also passes `&data.viewed` into
  `build_rows_cached`) + `data.rebuild_tree()`, saves the store, notifies.
- TDD: `build_rows_collapses_viewed_files_to_their_header` builds with one file
  marked viewed and asserts: `file_rows.len()` unchanged (still one header row per
  file), the viewed file's header carries `viewed: true` and is immediately followed
  by a `Spacer` then the next file's header (no hunks/lines/binary row leaked
  through), and the *other* (unviewed, binary) file still gets its `Binary` row.

### Threading `viewed` into the cached build path — how, and one correctness fix

Beyond the 5 production call sites the plan named, `install()`'s refresh branch
(`ItemState::Ready(data)` arm) needed care: it conditionally rebuilds rows from the
*newly fetched* diff (comment/mode/local-review changes) but at the point that
rebuild ran, `data.viewed` still held the *stale* pre-refresh set. I reordered that
branch to assign `data.diff`, recompute `data.review_id`, and call
`data.resolve_viewed(store)` **before** the conditional rebuild, and extended the
rebuild condition itself with `|| !data.viewed.is_empty()` — otherwise a refresh that
didn't change mode/comments would silently un-collapse every previously-viewed file
(the background-computed rows never know about `viewed`). `install()`'s signature
gained a `store: &Store` parameter; its one call site (`spawn_fetch`) now passes
`&app.store` — this compiles because `item` (borrowed via
`app.items.iter_mut().find(...)`) and `app.store` are disjoint fields, verified by a
clean build.

## Task 4 — `shift-v` toggle + skip viewed in `]`/`[`

- Added `ToggleViewed` action + `KeyBinding::new("shift-v", ToggleViewed, Some("ReviewApp"))`.
- Extracted `ReviewApp::active_file_ix(&self) -> Option<usize>` from the inline logic
  `render_sidebar` used to compute "current file" (top-of-viewport file, respecting a
  pending `deferred_scroll_to_item`); `render_sidebar` now calls it instead of
  duplicating the math. The `shift-v` handler calls it and `toggle_viewed`.
- `NextFile`/`PrevFile` handlers now call new pure functions `next_unviewed_target` /
  `prev_unviewed_target(targets, viewed, cursor)` instead of the generic
  `jump_next`/`jump_prev` (which remain unchanged and still back `NextHunk`/`PrevHunk`,
  which don't care about viewed state). Each finds the nearest file-header row past/
  before `cursor` whose file index isn't in `viewed`, falling back to the plain
  nearest-row search (ignoring viewed) when no unviewed candidate remains in that
  direction, so an all-viewed diff can't get stuck.
- TDD: `next_prev_unviewed_target_skip_viewed_files` — plain next/prev with nothing
  viewed, skipping one viewed file in each direction, `None` past the last target (no
  wraparound), and the all-viewed fallback in both directions including at the very
  ends.

## Task 5 — richer tree row

- `render_tree_row`'s `File` arm (both the plain tree and the fuzzy-filtered list)
  gained a `viewed_checkbox(file_ix, viewed, entity)` — a 12px `✓`/blank hit-target
  that stops its click from bubbling to the row's own jump-to-file `on_click`
  (`on_mouse_down` + `stop_propagation`, matching the pattern already used elsewhere
  in this file for nested clickables) — and dims the whole row when viewed
  (`opacity(0.6)`).
- Per-file `+N`/`−M` counts already existed in this codebase (the `stats` closure) —
  not something I had to add.
- Added a `comment_dot(data, path)` helper: a small blue dot shown when
  `CommentIndex::counts` has a nonzero anchored-comment count for that path; already
  available via `data.comments`, no new plumbing needed.
- Nothing trimmed here — the escape hatch wasn't needed; wiring the checkbox and dot
  into the existing row was straightforward once `data.viewed` existed.

## Files changed

- `crates/app/src/store.rs`
- `crates/app/src/main.rs`

No other files touched, per the plan's scope. `wrap_rows`, the perf-caching contract
around `base_rows`, and hit-testing beyond nav were left alone.

## Verification

```
cargo build -p lgtm        # clean, no warnings
cargo test -p lgtm          # 91 passed; 0 failed; 3 ignored (pre-existing, unrelated)
cargo clippy -p lgtm         # no warnings
cargo clippy -p lgtm --all-targets   # no warnings
```

New tests added (10 total): `viewed_set_check_and_unset`,
`viewed_round_trips_through_disk` (store.rs); `file_signature_stable_and_changes_with_diff`,
`review_id_distinguishes_pr_and_local_sources`, `build_rows_collapses_viewed_files_to_their_header`,
`next_prev_unviewed_target_skip_viewed_files` (main.rs).

## What was trimmed / deferred

Nothing from the plan's escape hatches was invoked — the "richer tree extras" (✓,
+/− counts, comment dot) all shipped; +/− counts turned out to already exist in the
codebase.

One deliberate, minor deviation from the plan's literal wording: the plan describes
the `FileHeader`'s `on_click` as toggling viewed *off* ("re-expand"). I implemented a
plain toggle (works in both directions) since (a) it's simpler than conditionally
wiring the handler only when `viewed`, (b) it's symmetric with the tree checkbox and
`shift-v`, and (c) clicking a header to mark it viewed when *not* collapsed is
harmless and arguably useful (quick "seen it, skip" from the diff pane itself).

Screenshot/manual verification of the visual polish (dim styling, ✓ color, dot
placement) was not done — the design doc calls for that ("Tree visuals + toggle
verified by screenshot") but running the actual GUI app was out of scope for this
pass; all logic is covered by unit tests instead.

## Concerns

- `toggle_viewed` calls `data.rebuild_rows_anchored()`, whose viewport-anchoring
  math (`nth_noncomment_row`) assumes a rebuild only adds/removes *comment* rows.
  Collapsing/expanding a file removes/adds *diff* rows (hunks/lines), which the
  anchor math doesn't model exactly — this is the same function the plan explicitly
  names for this step, so it's a known, accepted approximation: the viewport may
  land a few rows off after a toggle rather than pixel-perfect, particularly when
  toggling a file that isn't the one currently at the top. Not a correctness bug,
  just imprecise anchoring; flagging in case it's noticeable in practice and worth a
  follow-up (a dedicated anchor pass that accounts for row-count deltas per file
  rather than assuming only comment rows moved).
- No screenshot-based visual QA was performed (see above) — worth a quick manual
  pass to confirm the ✓ color/opacity choices read well in both the diff pane and
  the tree.

## Fixes

Follow-up pass fixing three review findings from the commit range `cb8074b..d5cc3e9`.
All changes in `crates/app/src/main.rs` plus a one-line `Hash` derive in
`crates/diff-core/src/lib.rs`; committed on `custom`.

### 1. [Major] Fresh install never applied resolved `viewed` to the initial rows

`ReviewItem::install()`'s fresh-item branch (the `_ =>` arm, not `ItemState::Ready`)
built `ItemData` from `Loaded.rows` — rows the background thread built via the
public `build_rows()` with an empty viewed set (no `Store` on that thread). It then
called `data.resolve_viewed(store)`, which only updates `data.viewed`, never
rebuilds `data.rows`. Net effect: reopening a previously-reviewed item after
restart briefly (and, since nothing else triggers a rebuild, durably) showed
viewed files expanded — the "persists across restart" guarantee was broken for
the very first paint.

Fix: mirrored the refresh branch's pattern — after `data.resolve_viewed(store)`,
if `!data.viewed.is_empty()`, rebuild via `build_rows_cached(&data.diff, data.mode,
&data.upgrades, data.comments.as_ref(), data.comments_visible,
&mut data.hunk_syntax_cache, &data.viewed)` and `data.set_rows(built)` before the
item is marked `Ready`.

Test added: `fresh_install_applies_resolved_viewed_to_initial_rows` — builds a
`Loaded` with expanded rows (as the background thread would), pre-populates a
`Store` with `set_viewed` for one file under the same `review_id`, calls
`ReviewItem::install()` directly, and asserts `data.viewed` contains that file
*and* `data.rows` shows its header collapsed (no `HunkHeader`/`Line` rows between
its header and the next file's).

### 2. [Should-fix] `toggle_viewed` mis-anchored the viewport

`toggle_viewed` called `rebuild_rows_anchored`, which anchors by counting
non-comment rows before the viewport top. That's exactly right for its other
three call sites (`toggle_comments`, `add_local_comment`, `refetch_comments`),
where only comment rows are inserted/removed everywhere in the document. But
collapsing/expanding one file changes the *diff*-row count (hunks/lines) for
that one file specifically — so toggling a file above the current scroll
position (trivial via the tree's ✓ checkbox) shifted every non-comment row
count below it, jumping the viewport by roughly the toggled file's row count.

Anchor approach chosen: added `locate_in_file`/`resolve_in_file` (free
functions, next to `nth_row_start`/`nth_noncomment_row`) and a new
`ItemData::rebuild_rows_anchored_to_file` (next to `rebuild_rows_anchored`,
left untouched to avoid touching its correct comment-toggle behavior). Instead
of ranking by non-comment row count, it anchors on **(file index, offset from
that file's header row)** — captured from `file_rows` before the rebuild and
restored against the new `file_rows` after. Since only the toggled file's row
count changes, this is *exact* (not approximate) for the viewport's current
top file whenever that's not the toggled file itself; when the toggled file
*is* the one at the top, the offset clamps to the file's new (possibly
1-row) range, landing on its header rather than jumping arbitrarily far.
Both the scroll-top anchor and the cursor position use the same helper.
`toggle_viewed` now calls `rebuild_rows_anchored_to_file()` instead of
`rebuild_rows_anchored()`; the other three call sites are untouched.

Test added:
`locate_and_resolve_in_file_anchor_survives_a_row_count_shift_elsewhere` —
a pure test of `locate_in_file`/`resolve_in_file`: given file headers at rows
`[0, 10, 20]` with the viewport anchored 3 rows into file 1 (row 13), and file 0
collapsing from 10 rows to 1 (new headers `[0, 1, 11]`), the resolved top lands
at `1 + 3 = 4` (pinned to the same spot in file 1) rather than being thrown off
by file 0's row-count delta; also covers the toggled-file-itself case (viewport
inside the collapsing file clamps to its remaining header row).

### 3. [Minor] Binary files never reset `viewed`

`file_signature` hashed only `display_path()` + per-hunk content, but
`FileStatus::Binary` files have empty `hunks`, so the signature degenerated to
`hash(path)` and never changed when the binary content changed (new commit,
same path).

Fix: added `Hash` to `FileStatus`'s derive list in `crates/diff-core/src/lib.rs`
(it already derived `PartialEq, Eq, Clone, Copy`, so this is a one-line,
side-effect-free addition), then hashed `file.additions`, `file.deletions`, and
`file.status` into `file_signature` alongside the existing path/hunk hashing.

Test added: `file_signature_changes_for_binary_content_changes` — using the
existing `sample_diff()` binary fixture (`b.png`, `FileStatus::Binary`, empty
hunks), asserts the signature changes when `additions`/`deletions` change, and
separately when `status` changes.

### Verification

```
cargo build -p lgtm            # clean, no warnings
cargo test -p lgtm              # 94 passed; 0 failed; 3 ignored (pre-existing, unrelated)
cargo clippy -p lgtm            # no warnings
cargo clippy -p lgtm --all-targets   # no warnings
```

Covering tests (excerpt from the full run):
```
test tests::fresh_install_applies_resolved_viewed_to_initial_rows ... ok
test tests::locate_and_resolve_in_file_anchor_survives_a_row_count_shift_elsewhere ... ok
test tests::file_signature_changes_for_binary_content_changes ... ok
test tests::file_signature_stable_and_changes_with_diff ... ok
test tests::build_rows_collapses_viewed_files_to_their_header ... ok
test result: ok. 94 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out
```
