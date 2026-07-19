# Plan — Batched pending PR comments

> REQUIRED SUB-SKILL for execution: subagent-driven-development. Steps use `- [ ]`.

**Base:** `42d652a`. **Files:** `crates/app/src/main.rs`, `crates/gh/src/lib.rs`.
**Verify:** `cargo test -p lgtm`, `cargo test -p gh`, `cargo clippy --workspace` (no new warnings), `cargo build -p lgtm`.
Spec: `docs/superpowers/specs/2026-07-19-batched-comments-design.md`.

**Goal:** PR inline comments queue as pending drafts and post together with the
verdict in one GitHub "create review" call; keep a "comment now" one-off. Replies
stay immediate.

## Task 1 — gh: create-review-with-comments call

- [ ] Read `crates/gh/src/lib.rs`: `submit_review` (verdict-only), `post_review_comment`,
  `ReviewVerdict`, `PrLocator`, and the private `gh(&[args])` helper.
- [ ] Add a public struct + fn:
```rust
    pub struct DraftComment {
        pub path: String,
        pub line: u64,
        pub side: String,   // "LEFT" or "RIGHT"
        pub body: String,
    }

    /// Create a review with inline comments in one call:
    /// POST repos/{o}/{r}/pulls/{n}/reviews with a JSON body on stdin.
    pub fn submit_review_with_comments(
        loc: &PrLocator, commit_oid: &str, verdict: ReviewVerdict, body: &str,
        comments: &[DraftComment],
    ) -> Result<()> {
        let payload = serde_json::json!({
            "commit_id": commit_oid,
            "event": verdict_event(verdict),   // "APPROVE"|"REQUEST_CHANGES"|"COMMENT"
            "body": body,
            "comments": comments.iter().map(|c| serde_json::json!({
                "path": c.path, "line": c.line, "side": c.side, "body": c.body
            })).collect::<Vec<_>>(),
        });
        // gh api --method POST repos/o/r/pulls/N/reviews --input - (stdin)
        // Use the same Command::new("gh") pattern as gh(...) but write `payload`
        // to stdin. See post_review_comment for how it already pipes JSON via
        // --input or -f fields; mirror that. Return Ok(()) on success, bail! with
        // stderr on failure.
    }
```
  - Reuse/extract the existing verdict→event mapping from `submit_review` (there is
    already one for the verdict-only path); factor it into `verdict_event`.
  - For the stdin JSON: check how `post_review_comment`/`fetch_review_comments`
    invoke `gh api` today and follow that idiom (likely `--input -` with
    `.stdin(...)`; if the codebase uses `-f`/`--raw-field`, a single `--input -`
    with a temp file or piped stdin is cleanest for nested `comments`).
- [ ] Unit-test the payload builder: extract the `serde_json::json!(...)` into a
  pure `fn review_payload(commit_oid, verdict, body, comments) -> serde_json::Value`
  and test event mapping + comments array shape (path/line/side/body). Do NOT test
  the live `gh` call in unit tests.
- [ ] `cargo test -p gh`, `cargo clippy -p gh`. Commit.

## Task 2 — pending drafts state + composer buttons

- [ ] Read `main.rs`: `Composer` struct (~3808), `submit_composer` (~6310),
  `add_local_comment`, `open_composer`, `ItemData`, and how the composer is rendered
  (`render_composer`).
- [ ] Add to `ItemData`:
```rust
    /// Pending PR review comments, submitted together with the verdict. PR-only;
    /// in-memory for the session (like local_review).
    pending_review: Vec<PendingComment>,
```
  with `struct PendingComment { path: String, side: CommentSide, line: u64, commit_id: String, body: String }`.
  Initialise `pending_review: Vec::new()` in the constructor.
- [ ] `submit_composer` PR branch: today it immediately posts. Change so the composer
  has two submit actions. Add a flag to `Composer` (e.g. `immediate: bool`) set by which
  button/key was used, or add a separate method. Behaviour:
  - **[Add to review]** (default, also the Enter key): push a `PendingComment`
    (from composer's path/side/line/commit_id/body) into `active_data().pending_review`,
    `rebuild_rows_anchored()`, close composer. No network.
  - **[Comment now]** (secondary button / e.g. `cmd-enter` in composer): today's
    immediate `gh::post_review_comment` path, unchanged.
  Local items: unchanged (single add-draft).
- [ ] `render_composer`: for PR items show both buttons; label them. (Local items keep one.)
- [ ] `cargo build -p lgtm`. Commit.

## Task 3 — render pending drafts in the diff (editable/deletable)

- [ ] Pending drafts must appear in the diff at their (path, line) with a "draft" style
  and edit/delete affordances. Reuse the existing comment-row rendering. Two options —
  pick the lower-friction:
  (a) When building rows, merge `pending_review` into the per-file comment anchors the
      same way fetched comments are (see how `CommentIndex`/`group_comments` feed
      `build_rows`), tagging pending ones so `render_row` styles them as drafts; or
  (b) after a hunk row for a pending (path,line), emit a dedicated `Row::PendingComment`.
  Add edit (reopen composer prefilled with the draft's body, remove the draft so
  re-submitting replaces it) and delete (remove from `pending_review`) — small buttons on
  the draft row, calling new `edit_pending`/`delete_pending` methods that rebuild rows.
- [ ] Test the pending add/edit/delete list operations as pure `Vec<PendingComment>` ops.
- [ ] `cargo build -p lgtm`. Commit.

## Task 4 — submit posts pending + verdict atomically

- [ ] Read `submit_review` (~6512) and the review dialog (`open_review`/`render_review`,
  `ReviewDialog`).
- [ ] On submit (cmd-enter / Review dialog confirm) for a PR item **with** pending drafts:
  build `Vec<gh::DraftComment>` from `pending_review` (map `CommentSide`→"LEFT"/"RIGHT",
  carry line/path/body; use the item's head commit oid — the `pr_meta.head_ref_oid`,
  same source `post_review_comment` uses for `commit_id`), then call
  `gh::submit_review_with_comments(loc, head_oid, verdict, body, &drafts)` on the
  background executor. On success: clear `pending_review`, refetch comments (reuse
  `refetch_comments`). With **no** pending drafts, keep calling the existing verdict-only
  `submit_review`.
- [ ] Error path: on failure, keep `pending_review` intact and surface the error in the
  review dialog (mirror the current submit error handling). Don't lose drafts.
- [ ] `cargo build -p lgtm`, `cargo test -p lgtm`, `cargo clippy --workspace` (no new
  warnings). Commit.

## Scope / boundaries
- PR items only for pending/batch; local review path unchanged.
- Replies to existing threads stay immediate (`post_reply`).
- Pending drafts are in-memory (session) — do not add persistence in this plan.

## Done criteria
- Typing an inline comment on a PR queues a pending draft (visible in the diff, editable
  & deletable); nothing posts until submit.
- Submitting the review posts all drafts + the verdict in ONE `gh` create-review call
  (verified against a real PR); a failed submit keeps the drafts.
- "Comment now" still posts a single comment immediately.
- `cargo test -p lgtm` + `-p gh` green; `cargo clippy --workspace` no new warnings.
