# Plan — Surface added keymaps in the footer

> REQUIRED SUB-SKILL for execution: subagent-driven-development. Steps use `- [ ]`.

**Base:** `42d652a` (run LAST, after the viewed-files plan adds `shift-v`).
**File:** `crates/app/src/main.rs` (`render_footer`). **Verify:** `cargo build -p lgtm`, `cargo clippy -p lgtm`.

**Goal:** The footer keymap strip is missing keys added on `custom` and in the new
features. Surface the useful ones without clipping on normal window widths.

## Context
`render_footer` renders a horizontal row of `hint(&[keys], label)` chips. Current
chips: `]/[` files, `n/p` hunks, `v` unified/split, `m` minimap, `c` comments,
`/` filter files, `home/end` top/bottom, `⌘k` palette, `⌘t` open, `⌘b` sidebar,
`⌘j` chat, `r` refresh, `⌘⏎` review.

Missing/new (added on custom + features): `w` wrap, `⌘1–9` tabs, `⇧V` viewed
(new), and `⌘⇧S` screenshot (utility, lowest priority). `ctrl-tab` cycles items.

## Steps
- [ ] Read `render_footer` and the `hint` helper. Note the exact `Keystroke::parse`
  strings it accepts (e.g. how `cmd-enter`/`home` are passed) so new chips parse.
- [ ] Add chips, grouped logically next to related ones:
  - after `unified/split`: `hint(&["w"], "wrap")`
  - after `comments`: `hint(&["shift-v"], "viewed")`
  - near `sidebar`/`open`: `hint(&["cmd-1…9"], "tabs")` — if `Keystroke::parse`
    can't take the literal `"cmd-1…9"`, render this chip as plain text (a small
    label without a parsed `Kbd`), or use `hint(&["cmd-1"], "…9 tabs")`. Pick
    whatever renders cleanly; the goal is discoverability, not a parsable binding.
  - lowest priority (include only if it fits): `hint(&["cmd-shift-s"], "screenshot")`.
- [ ] **Overflow ("if there's room"):** the strip is a single row and will clip on
  narrow windows once these are added. Make it degrade gracefully: give the footer
  container `.flex_wrap()` so chips wrap to a second line instead of clipping, OR
  wrap the strip in an `overflow_x_scroll` container. Prefer `flex_wrap` if the
  footer height can grow; if the footer is fixed-height, use `overflow_x_scroll`
  and drop `cmd-shift-s` from the strip. Choose the one that doesn't clip and note
  which you did.
- [ ] `cargo build -p lgtm`; `cargo clippy -p lgtm` (no new warnings).
- [ ] Screenshot verification will be done by the maintainer (the layout is visual).

## Scope / boundaries
- `render_footer` only. Don't change keybindings themselves (they already exist);
  this is display-only. Don't restructure the footer beyond the overflow handling.

## Done criteria
- The footer shows `w` (wrap), `⇧V` (viewed), and `⌘1–9` (tabs) chips, and does not
  clip existing chips on a normal-width window (wrap or scroll handles overflow).
- Build + clippy clean.
