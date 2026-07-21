# Upstreaming guide

**Strategy:** `custom` is the single source of truth (build/use it). Upstream PR
branches to `ellie/lgtm` are cut **on-demand, in dependency order** — not
maintained in parallel — because later features stack on earlier ones.

Remotes: `origin` = `nsturis/lgtm` (fork), `upstream` = `ellie/lgtm`.

## Dependency order (cut/PR in this sequence)

Independent — clean off `upstream/main`, PR in any order:
1. **language-support** — tree-sitter langs + PHP `php_only`/0.23 pin + Vue. (`crates/syntax`, README)
2. **finder-launch-path** — recover login-shell PATH for Finder/Dock launches. (`main.rs`)
3. **resizable-sidebar** — draggable sidebar width. (`main.rs`)
4. **cmd-number-tab-switch** — `⌘1–9` jump to tab N. (`main.rs`)
5. **ci-gate** — `.github/workflows/ci.yml` (fmt/clippy -D warnings/test on macos-15) + dead-code/clippy cleanup. (independent; land early so later PRs are gated)

Stacked — each needs the ones above it landed first:
6. **soft-wrap** — long-line wrapping (needs the split/diff render baseline).
7. **rebuild-perf** — cache unwrapped rows + memoize hunk syntax (builds on soft-wrap's `set_rows`).
8. **pr-dashboard** — cmd-k repo home, recents, pinning, `store.rs` (needs store + palette).
9. **mark-as-viewed** — viewed collapse + richer tree (needs `store.rs` from #8 and `build_rows_cached` from #7).
10. **batched-comments** — pending PR review drafts + one-call submit (needs the perf build path from #7).
11. **footer-keymap** — surfaces keys added across #4/#6/#9 + screenshot (needs those keys to exist).
12. **conflict-badge** — red "conflicts" tag in the PR titlebar from `gh pr view`'s
    `mergeable` (`crates/gh`, `main.rs`). Needs the PR titlebar / review-decision
    baseline (from #8). Commit `178b62d`.
13. **single-instance-tabs** — `on_open_urls` opens folders as tabs in the running
    app + `public.folder` doc type in `release.yml`'s bundle plist (`main.rs`).
    Needs the multi-tab `open_item` infra (from #8). Pushing the workflow file
    needs `workflow` scope (like #5). Commit `4a0e787`.
14. **sticky-file-header** — pin the current file's header to the top of the diff
    while scrolling; redraw-on-wheel (`main.rs`). Reuses `render_row`'s FileHeader,
    so needs the viewed/comments fields from #9 + #10 and the render baseline.
    Commit `4134a6d`.

Amendments to existing buckets (fold in when those are cut, not a standalone PR):
- **viewed-advance + key swap** (`8a2699d`) — `v` marks viewed & advances to the
  next unviewed file, `shift-v` toggles view. The `toggle_viewed`/advance parts
  belong with #9 **mark-as-viewed**; the footer-hint swap belongs with #11
  **footer-keymap**.

## Existing fork branches (snapshots; may drift from custom)
- `feat/language-support`, `fix/finder-launch-path`, `feat/resizable-sidebar`,
  `feat/cmd-number-tab-switch` — the independent set (#1–4); currently clean off `main`.
- `feat/pr-dashboard` — snapshot of #8; will drift as `custom` evolves.
- (No branches for soft-wrap / perf / mark-as-viewed / batched-comments / footer —
  cut on-demand.)

## To cut a branch when ready to PR feature X
```sh
git fetch upstream
git checkout -b feat/X upstream/main          # or off the last-landed dependency
git cherry-pick <X's commits from custom>     # see `git log upstream/main..custom`
# resolve conflicts from missing deps → either land deps first, or base on them
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
git push -u origin feat/X
gh pr create --repo ellie/lgtm --base main --head nsturis:feat/X
```
Note: pushing workflow files (the ci-gate PR) needs SSH or `gh auth refresh -s workflow`.
