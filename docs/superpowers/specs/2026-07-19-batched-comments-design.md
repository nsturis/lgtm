# Batched pending PR comments — design

**Date:** 2026-07-19 · **Base:** `7a2e175` · **Branch:** `custom`

## Problem
PR inline comments post to GitHub **immediately, one-by-one** (`submit_composer`
→ `gh::post_review_comment`), and the verdict posts separately
(`gh::submit_review`). Real review is: jot several comments, then submit them
together with an approve/request-changes. Local reviews already batch in-memory
(`local_review`); PRs should get the same, using GitHub's review API.

## Decision (from brainstorming)
**Pending by default + a "comment now" escape.** New inline comments queue as
pending drafts (shown in-diff, editable/deletable) and post together with the
verdict via one GitHub "create review" call. Keep an immediate "comment now" for
one-offs. Replies to existing threads stay immediate.

## State (`main.rs`)
- `ItemData` gains `pending_review: Vec<PendingComment>` where
  `PendingComment { path: String, side: CommentSide, line: u64, commit_id: String, body: String }`.
  (Mirrors the `local_review` draft shape; PR-only.)
- Composer (for PR items) shows two actions: **[Add to review]** → push a
  `PendingComment`; **[Comment now]** → today's immediate `post_review_comment`.
  Local items keep their single "add draft" action unchanged.

## Display
- Pending drafts merge into the displayed comments: extend the comment rows so a
  pending draft renders with a distinct "draft" style and **edit** (reopens the
  composer prefilled, removing the draft so re-submit replaces it) + **delete**
  (drops it) affordances. Simplest wiring: build pending drafts into the
  `CommentIndex`/rows with a `pending: bool` marker, or render them as an extra
  overlay row per (path,line). Implementer picks the lower-friction path that
  reuses the existing comment-row rendering.

## Submit (one atomic call)
- New `gh::submit_review_with_comments(loc, verdict, body, comments)` where
  `comments: &[PendingComment]`. It builds a JSON payload and calls
  `gh api --method POST repos/{o}/{r}/pulls/{n}/reviews --input -` (stdin JSON):
  ```json
  { "commit_id": "<head oid>", "event": "APPROVE|REQUEST_CHANGES|COMMENT",
    "body": "<summary>",
    "comments": [ { "path": "...", "line": N, "side": "LEFT|RIGHT", "body": "..." } ] }
  ```
  (Use each draft's own commit_id; GitHub's create-review takes `line`+`side` for
  the current diff.) On success: clear `pending_review`, refetch comments.
- The Review dialog / `cmd-enter` path calls this instead of the current
  verdict-only `submit_review` **when there are pending comments**; with none, it
  may still use the existing verdict-only path. `submit_review` (verdict-only)
  stays for the no-pending case.

## Error handling
- If the batched call fails, keep the pending drafts (don't lose them) and surface
  the error in the review dialog, same as today's submit error path.
- Empty body + empty pending + "comment" event → no-op (existing guard).

## Testing
Unit-test the JSON-payload builder (`submit_review_with_comments`'s serialization:
correct event mapping, comments array, side/line) and the pending add/edit/delete
list ops. The live API call is verified against a real PR.

## Scope / non-goals
- Pending drafts are **in-memory for the session** (lost on quit) — consistent
  with `local_review`; server-side pending-review persistence is deferred.
- Replies to existing review threads remain immediate (`post_reply`).
- No multi-line/range comments beyond what the composer already captures.
