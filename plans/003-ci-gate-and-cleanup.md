# Plan 003 — CI quality gate + clippy clean + dead-code removal (DX1, DX2, T1)

Written against `16b682e`. Files: `crates/app/src/main.rs`, `crates/app/src/store.rs`,
new `.github/workflows/ci.yml`. Run this plan LAST (after 001, 002) so the gate
reflects the final code. Verify with `cargo clippy --workspace` (must be zero
warnings at the end), `cargo test --workspace`, `cargo fmt --check`.

## Step 1 — T1: remove the dead `PaletteStep::Sources` cluster

The palette entry step is now `RepoHome`; the old `Sources` step is unreachable
(clippy: "variant `Sources` is never constructed"). Remove the whole cluster:

- [ ] Read the palette code first. Remove, in `crates/app/src/main.rs`:
  - the `PaletteStep::Sources { .. }` enum variant,
  - const `PALETTE_SOURCES`, `SOURCE_PR`, `SOURCE_FOLDER`,
  - fn `filtered_sources`,
  - fn `palette_activate_source`,
  - every `Sources` match arm in `palette_back`, `palette_move`,
    `palette_query_changed`, `palette_confirm`, and the `Sources` render branch in
    `render_palette` (header + body),
  - the test `source_options_filter_by_substring` (it tests dead code).
  Grep `Sources`, `PALETTE_SOURCES`, `SOURCE_PR`, `SOURCE_FOLDER`, `filtered_sources`,
  `palette_activate_source` to find all references; remove until none remain.
- [ ] Also in `crates/app/src/store.rs`: `is_pinned_repo` and `is_pinned_pr` are
  unused (clippy dead_code). Grep to confirm no non-test caller. If truly unused,
  delete both methods and any test that only exercises them. (If a caller exists,
  leave them — report which.)
- [ ] `cargo build -p lgtm` compiles; `cargo test -p lgtm` passes. The two
  `never constructed`/`never used` clippy warnings are gone.

## Step 2 — DX2: clear the remaining clippy warnings

Baseline had ~12 warnings; Step 1 removes the dead-code ones. Clear the rest so
`-D warnings` can be turned on.

- [ ] Run `cargo clippy --workspace` and read every remaining warning. Expect:
  `needless_range_loop` (~2), `map_or` simplification (~1), `collapsible_if` (~2),
  `single_match`/match-for-single-pattern in `lsp_client.rs` (~1), and
  `too_many_arguments` (~3: functions with 8–9 params).
- [ ] For the mechanical lints (needless_range_loop, map_or, collapsible_if,
  single_match): `cargo clippy --fix --allow-dirty --workspace` handles most; review
  each auto-fix for correctness, then hand-fix any it leaves. Keep changes minimal
  and behavior-identical.
- [ ] For `too_many_arguments` (refactoring to param structs is out of scope and
  risky here): add a scoped `#[allow(clippy::too_many_arguments)]` on each of the 3
  offending functions with a one-line comment noting it's a known god-module smell
  tracked under the ARCH refactor. Do NOT restructure their signatures.
- [ ] `cargo clippy --workspace` → **zero** warnings. `cargo test --workspace` passes.
  `cargo fmt --check` passes (run `cargo fmt` if not, review the diff stays minimal).

## Step 3 — DX1: add a CI quality gate

The only workflow (`.github/workflows/release.yml`) builds + ships a dmg with no
test/clippy/fmt gate. Add one.

- [ ] Create `.github/workflows/ci.yml`. **Use `macos-15`** (the app links Metal via
  its build script and does not build on Linux — the existing release job is macOS
  for this reason). Run on `push` and `pull_request`:
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
- [ ] Optional: make the release `dmg` job depend on `check` by adding `needs: check`
  in `release.yml` (only if you also merge the workflows or reference it correctly —
  otherwise leave release.yml as-is; the separate CI workflow already gates PRs and
  main pushes). Note what you chose.
- [ ] Verify locally that the exact CI commands pass:
  `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.

## Scope / boundaries

- In scope: dead-code removal, clippy clean-up (fixes + scoped allows), the new CI
  workflow. Out of scope: the ARCH-01 module split, param-struct refactors, changing
  app behavior.
- Do not "improve" adjacent code while clearing clippy — only touch what a lint flags.

## Done criteria

- `cargo clippy --workspace --all-targets -- -D warnings` exits 0 (zero warnings).
- `cargo test --workspace` passes; `cargo fmt --all --check` passes.
- `.github/workflows/ci.yml` exists and runs fmt/clippy/test on macos-15.
- No `Sources`/`PALETTE_SOURCES`/`filtered_sources`/`palette_activate_source`
  references remain; `is_pinned_repo`/`is_pinned_pr` removed (or justified).
