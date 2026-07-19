# PR dashboard: repo home, recents, and pinning

**Date:** 2026-07-19
**Status:** Approved design, ready for implementation plan
**Branch:** `fix/highlighting-sidebar-wrapping` (feature continues here)

## Problem

Opening a PR today means typing the full `owner/repo#123` into the palette or
the toolbar input every single time. The app persists **nothing** between runs
(`items` starts empty each launch), so there is no memory of repos you work in
and no way to keep favourite PRs close. The user wants: a quicklist of a repo's
PRs, quick switching, and pinning — organised around **one active repo at a
time**, with the ability to pin **both repos (favourites) and individual PRs**.

## Goals

- Never retype a repo you've used before: remember **recent** repos and let the
  user **pin** favourites.
- One keystroke from launch to a remembered repo's live PR list.
- **Pin PRs** within a repo so they sit at the top of that repo's list.
- Fast switching between already-open PR tabs.

## Non-goals (YAGNI for v1)

- Cross-repo "my PRs across everything" aggregation (user chose per-repo).
- CI/checks columns (`statusCheckRollup`) — heavy GraphQL payload.
- Background auto-refresh / polling.
- Adopting `gpui-component`'s `list`/`table` widgets — the existing
  `uniform_list` + palette pattern is lower-risk and matches the codebase.
- Review-decision status dot (approved / changes-requested). Deferred; `isDraft`
  is the only status signal in v1. Adding `reviewDecision` later is a one-field
  change to `gh::list_prs`.

## Surface: enhanced cmd-k palette

Chosen over a dedicated pane or sidebar section: it reuses the existing palette
state machine and PR-list step, so it is the least new code and keyboard-first.

```
cmd-k →  ┌ Repo Home ─────────────────────┐
         │ ★ acme/api   ★ acme/web        │   pinned repos
         │   org/infra  foo/bar           │   recent (MRU)
         │ ▸ type owner/repo…  · [folder] │   type a new one / open local
         └────────────────────────────────┘
pick repo →  ┌ acme/api ──────────────────┐
             │ ★ #482 Fix auth retry   me │   pinned PRs, on top
             │ ★ #471 Big refactor     jo │
             │ ─────────────────────────  │
             │   #479 Add rate limiter ka │   live open PRs (gh::list_prs)
             │   #468 Docs tweak       li │
             └────────────────────────────┘  Enter/click → opens diff (existing)
```

## Architecture

### New module: `crates/app/src/store.rs` (isolated, pure, testable)

The only persistence in the app. No gpui dependency, so it unit-tests against a
temp path.

```rust
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Store {
    pub pinned_repos: Vec<String>,             // "owner/repo", user order
    pub recent_repos: Vec<String>,             // MRU, capped
    pub pinned_prs: BTreeMap<String, Vec<u32>>, // "owner/repo" -> PR numbers
}
```

- **Location:** `dirs::config_dir()?/lgtm/state.json`
  (`~/Library/Application Support/lgtm/state.json` on macOS). Add
  `dirs = "6"` to `crates/app/Cargo.toml` (already transitive in the lockfile,
  so no new download). `serde` + `serde_json` are already available.
- **Load:** best-effort — missing file, unreadable, or malformed JSON all yield
  `Store::default()` (never panics, never blocks startup).
- **Save:** atomic — write `state.json.tmp` then `fs::rename` over `state.json`,
  mirroring the existing write-temp-then-rename idiom (`main.rs:3670`). Called
  after every mutation.
- **Mutators (all keep the store normalised):**
  - `toggle_pinned_repo(&mut self, slug: &str)` — add/remove in `pinned_repos`.
  - `note_recent_repo(&mut self, slug: &str)` — move-to-front, dedupe, cap at
    `RECENT_CAP` (8). A pinned repo need not also appear in recents.
  - `toggle_pinned_pr(&mut self, slug: &str, number: u32)` — add/remove in
    `pinned_prs[slug]`; drop the key when its vec empties.
  - `is_pinned_repo` / `is_pinned_pr` / `pinned_prs_for(slug)` — queries.
- **Constants:** `RECENT_CAP: usize = 8`.

### Pure ordering helper (testable)

```rust
/// Pinned PRs first (in `pinned` order), then the rest in gh's order,
/// with no PR appearing twice. Returns indices into `prs`.
fn order_prs(prs: &[gh::PrSummary], pinned: &[u32]) -> Vec<usize>;
```

Lives next to the palette code; unit-tested independently of rendering.

### `ReviewApp` state additions

- `store: Store` — loaded once in `ReviewApp::new` via `store::load()`.
- `active_repo: Option<String>` — the last repo the home opened into, so the
  home can highlight it and the PR-list step knows its slug for pin toggles.

### Palette changes (`main.rs`)

- New `PaletteStep::RepoHome` becomes the `cmd-k` entry point (replacing the
  current two-item `Sources` step as the default; folder-open stays reachable
  from it). Renders `pinned_repos` (★) then `recent_repos`; the shared
  `palette_input` filters the combined list, and a valid `owner/repo` typed in
  (via `parse_repo_slug`) is acceptable directly. Selecting/entering a repo →
  existing `palette_fetch_prs` → `PrList` step, and calls
  `store.note_recent_repo` + `active_repo = Some(slug)` + `store::save`.
- `PrList` rendering gains pinned-to-top ordering via `order_prs`, a divider row
  between pinned and unpinned, and a **pin star** per row that toggles
  `pinned_prs` and re-saves. Reuses `palette_pr_row` with an added star element.
- Repo-home rows get a **pin star** toggling `pinned_repos`.
- Keyboard: existing palette up/down/enter/escape continue to work across the
  new step. Add a pin key (e.g. `cmd-p` / a star click) — click is the baseline;
  a keybinding is optional polish.

### Quick switching

- The repo home is the primary fast switcher.
- Add actions `GoToTab1..GoToTab9` bound to `cmd-1`..`cmd-9` (context
  `"ReviewApp"`, alongside the existing block near `main.rs:192`), each calling
  `activate(n-1, …)` when that open tab exists. Complements the existing
  `ctrl-tab` cycling.

## Data flow

1. Startup: `ReviewApp::new` → `store::load()` into `self.store`.
2. `cmd-k` → `RepoHome` renders `pinned_repos` + `recent_repos` from `store`.
3. Pick/type a repo → `parse_repo_slug` → `palette_fetch_prs` (background
   `gh::list_prs`) → `PrList{ repo, prs }`; `note_recent_repo` + save;
   `active_repo` set.
4. `PrList` renders `order_prs(prs, store.pinned_prs_for(slug))`.
5. Star toggles → `store.toggle_pinned_*` → `store::save()` → `cx.notify()`.
6. Pick a PR → existing `palette_open_pr_row` → `open_item` (unchanged).

## Error handling

- All store I/O is best-effort: any failure to read/write leaves the app fully
  functional (just without persistence that run). No error dialogs for the
  config file.
- `gh::list_prs` failures already surface via the existing `PrListState::Failed`
  path — unchanged.
- A pinned repo/PR that 404s later still lists fine; opening it surfaces the
  existing fetch error. No proactive validation/pruning in v1.

## Testing

- `store.rs`: save→load round-trip; `note_recent_repo` move-to-front + dedupe +
  cap; `toggle_pinned_repo`/`toggle_pinned_pr` add/remove + empty-key cleanup;
  corrupt/missing file → `default`. Uses a temp dir, not the real config path
  (inject the path or use an env override in tests).
- `order_prs`: pinned-to-top ordering, no duplicates, pinned numbers absent from
  the live list handled gracefully.
- Existing palette/parse tests continue to pass; `parse_repo_slug` reused as-is.

## Rollout / boundaries

Single implementation plan. Touch points: new `store.rs`; `crates/app/Cargo.toml`
(`dirs`); `main.rs` (palette `RepoHome` step, `PrList` pin ordering + stars,
`ReviewApp` fields, `cmd-1..9` actions). The gh and other crates are untouched.
