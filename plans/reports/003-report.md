# Plan 003 report — CI gate + clippy clean + dead-code removal

Branch: `custom`. Commits:
- `ad21f99` refactor: remove dead palette cluster, clear all clippy warnings
- `d8518d1` ci: add fmt/clippy/test quality gate on macos-15

## T1 — dead `PaletteStep::Sources` cluster + Store dead code

Grepped `Sources`, `PALETTE_SOURCES`, `SOURCE_PR`, `SOURCE_FOLDER`,
`filtered_sources`, `palette_activate_source` in `crates/app/src/main.rs` and
removed every reference:

- `PaletteStep::Sources { selected: usize }` enum variant
- `const PALETTE_SOURCES: [&str; 2]`, `const SOURCE_PR`, `const SOURCE_FOLDER`
- `fn filtered_sources`
- `fn palette_activate_source`
- match arms in `palette_back`, `palette_move`, `palette_query_changed`,
  `palette_confirm`
- the header (`None`) and body render branch in `render_palette`
- the test `source_options_filter_by_substring`

**Non-obvious fallout (fixed, not just noted):** removing `palette_activate_source`
(the only place that ever constructed `PaletteStep::RepoInput`) turned
`RepoInput` into new dead code — `cargo build` immediately reported
`variant RepoInput is never constructed`. Confirmed via `git show HEAD:...` that
`palette_activate_source`'s `SOURCE_PR` arm was RepoInput's *only* constructor
even before this change (RepoHome already bypasses it, calling
`palette_fetch_prs` directly for a typed `owner/repo`). Removed the entire
`RepoInput` cluster too: the enum variant, the `palette_back`/`palette_query_changed`/
`palette_confirm` arms, the render header/body branch, and the now-unused
`query` binding at the top of `palette_confirm`. `parse_repo_slug` stays (still
used by `palette_home_confirm`).

**`Store::is_pinned_repo` / `is_pinned_pr` decision:** grepped
`is_pinned_repo|is_pinned_pr` workspace-wide — the only call sites were their
own unit test (`toggle_repo_and_pr_are_reversible`); no non-test caller.
Deleted both methods. `toggle_pinned_repo`/`toggle_pinned_pr`/`pinned_prs_for`
are genuinely used from `main.rs` (confirmed) and were kept. Rewrote the one
affected test to assert via `s.pinned_repos.contains(..)` and
`s.pinned_prs_for(..)` instead of the deleted predicates, so it still exercises
`toggle_pinned_repo`/`toggle_pinned_pr` end to end.

Verified after T1 alone: `cargo build -p lgtm` clean, `cargo test -p lgtm` —
83 passed, 0 failed, 3 ignored.

## DX2 — clippy warnings cleared

Ran `cargo clippy --workspace --all-targets` (no `-D warnings` yet) after T1 to
get the real baseline — it differed from the plan's guess (which didn't
anticipate `single_range_in_vec_init`/`useless_vec`, or the `RepoInput`
fallout, or a lint from my own T1 test edit). Full list and disposition:

| # | Location | Lint | Disposition |
|---|---|---|---|
| 1 | `diff-core/src/lib.rs:600,604` | `single_range_in_vec_init` (test asserts on `Vec<Range<_>>`) | **Allowed** — scoped `#[allow(clippy::single_range_in_vec_init)]` on `mod tests`, with comment. Both of clippy's rewrites (`(0..3).collect()` vs `vec![0; 3]`) change the value's meaning; these are intentional single-span fixtures. |
| 2 | `lsp_client.rs:291` | `single_match` (match with only `Ok`/`Err(_)` arms) | **Fixed** — rewrote as `if let Ok(msg) = ...`, moved the explanatory comment above the `if let` (was on the `Err(_)` arm). |
| 3 | `store.rs:214` | `unnecessary_get_then_check` (`.get("a/b").is_none()`) — introduced by my own T1 test rewrite | **Fixed** — `!s.pinned_prs.contains_key("a/b")`. |
| 4 | `main.rs:951` (`selection_text`) | `needless_range_loop` | **Fixed** — `for ix in a..=b` → `for (ix, row) in rows.iter().enumerate().take(..).skip(..)`, `ix` still passed by value to `row_selection_range`. |
| 5 | `main.rs:4022` (`selection_info`) | `needless_range_loop` | **Fixed** — same enumerate/take/skip pattern. |
| 6 | `main.rs:2181` (`minimap_runs`) | `unnecessary_map_or` (`map_or(true, ..)`) | **Fixed** — `.is_none_or(..)` (stable since Rust 1.82; toolchain is 1.91.1). |
| 7 | `main.rs:5688` (`render_composer_affordance`-ish click handler) | `collapsible_if` | **Fixed** — nested `if matches!(..) { if .. { return None } }` merged into one `if a && b { return None }`. |
| 8 | `main.rs:7038` (`render_plus`) | `collapsible_if` | **Fixed** — identical merge, same shape as #7. |
| 9 | `main.rs:1109` `push_gap_rows` (9 args) | `too_many_arguments` | **Allowed** — scoped `#[allow(clippy::too_many_arguments)]` + one-line "known god-module smell, tracked under ARCH refactor" comment. No signature change. |
| 10 | `main.rs:6070` `open_composer` (8 args) | `too_many_arguments` | **Allowed** — same. |
| 11 | `main.rs:6278` `add_local_comment` (8 args) | `too_many_arguments` | **Allowed** — same. |
| 12 | `main.rs:10215` test helper `rc(..)` (8 args) | `too_many_arguments` | **Allowed** — same treatment for consistency (test-only fixture builder). |
| 13 | `main.rs` — 8 more `single_range_in_vec_init`/single-element `Vec`/array-of-`Range` sites in tests (lines 8863, 8865, 8921, 8922, 9149, 9356, 9366, 9380, 10945 pre-edit numbering) | `single_range_in_vec_init` | **Allowed** — same scoped `#[allow(...)]` on `mod tests` as #1, one attribute covers all of these. |
| 14 | `main.rs` ×3 `useless_vec` in tests (`row_range_unified_multi_row`, `row_range_split_sides_and_absent_cells`, `row_range_clamps_columns_to_text`) | `useless_vec` | **Fixed** — `vec![...]` → array literal `[...]` (values only indexed/borrowed, never grown). |

After all of the above: `cargo clippy --workspace --all-targets` → **zero**
warnings, and `cargo clippy --workspace --all-targets -- -D warnings` exits 0.

**fmt:** `cargo fmt --all --check` failed on the pre-existing tree — verified
by stashing all my changes and re-running it against clean `custom` HEAD
(827 lines of diff, unrelated to this work: e.g. `server_command` signature
wrapping in `lsp_client.rs`, import wrapping in `main.rs`). Since the CI gate
requires this to pass, ran `cargo fmt --all` across the whole workspace.
Reviewed the resulting diff in `git/lib.rs` and `syntax/lib.rs` (files I hadn't
otherwise touched) — purely whitespace/line-wrap reflow, no semantic change.

## DX1 — CI workflow

Added `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  check:
    runs-on: macos-15
    steps:
      - uses: actions/checkout@v4
      - uses: Swatinem/rust-cache@v2
      - name: Format
        run: cargo fmt --all --check
      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Test
        run: cargo test --workspace
```

`runs-on: macos-15` per the plan (the app links Metal via its build script,
same reason `release.yml`'s `dmg` job is macOS-only).

**`needs: check` decision:** left `release.yml` unchanged. `needs:` only
resolves job names within the *same* workflow file; there's no way to make the
`dmg` job in `release.yml` depend on the `check` job in `ci.yml` without
merging the two workflows (out of scope — the plan says this is optional and
to leave `release.yml` alone if not merging). `ci.yml` already independently
gates every PR and every push to `main`.

## Verification — exact CI commands, run locally

```
$ cargo fmt --all --check
(no output, exit 0)

$ cargo clippy --workspace --all-targets -- -D warnings
    Checking diff-core v0.1.0 (.../crates/diff-core)
    Checking syntax v0.1.0 (.../crates/syntax)
    Checking git v0.1.0 (.../crates/git)
    Checking lgtm v0.1.0 (.../crates/app)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.40s
(exit 0)

$ cargo test --workspace
test result: ok. 7 passed; 0 failed; 0 ignored   (claude)
test result: ok. 12 passed; 0 failed; 0 ignored  (diff-core)
test result: ok. 8 passed; 0 failed; 0 ignored   (gh)
test result: ok. 9 passed; 0 failed; 0 ignored   (git)
test result: ok. 83 passed; 0 failed; 3 ignored  (lgtm/app)
test result: ok. 9 passed; 0 failed; 0 ignored   (syntax)
+ 5x "0 passed" doc-test suites, all ok
(exit 0)
```

## Done-criteria checklist

- [x] `cargo clippy --workspace --all-targets -- -D warnings` exits 0.
- [x] `cargo test --workspace` passes; `cargo fmt --all --check` passes.
- [x] `.github/workflows/ci.yml` exists, runs fmt/clippy/test on `macos-15`.
- [x] No `Sources`/`PALETTE_SOURCES`/`filtered_sources`/`palette_activate_source`
      references remain (also removed the now-dead `RepoInput` cluster as
      fallout); `is_pinned_repo`/`is_pinned_pr` removed (no caller existed).

## Concerns / notes for the user

- The `RepoInput` removal is beyond the plan's literal text but is direct,
  mechanical fallout of T1 (confirmed dead before *and* after by grep/build);
  removing it was necessary to reach zero warnings without an `#[allow]`, and
  it's the same kind of change T1 already does elsewhere.
- `cargo fmt --all` reformatted `git/lib.rs` and `syntax/lib.rs` in addition to
  the files this task touched, because the whole tree was already failing
  `fmt --check` before this work started. This is required for the new CI fmt
  gate to be meaningful; the diff there is whitespace-only (spot-checked).
