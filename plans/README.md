# Improvement plans — lgtm

Written against commit `16b682e`. Source: `/improve` audit (correctness, security,
performance, tech-debt). These cover the correctness regressions from the recent
soft-wrap feature plus the highest-leverage quality/perf findings.

All three plans edit `crates/app/src/main.rs` (002 also adds a field; 003 also
adds CI + edits Cargo). Because they touch the same file, **execute them
sequentially in this order**, not in parallel:

| # | Plan | Findings | Effort | Risk | Status |
|---|---|---|---|---|---|
| 001 | Soft-wrap logical-line correctness | C1, C2, C3 | S–M | LOW–MED | TODO |
| 002 | Diff rebuild performance | P1, P2 | M | MED | TODO |
| 003 | CI quality gate + clippy + dead code | DX1, DX2, T1 | S | LOW | TODO |

**Order rationale:** 001 and 002 fix/refactor render code; 003 runs last so its
`clippy -D warnings` gate reflects the final state (and clears any warnings 001/002
introduce). 003's dead-code removal (T1) is independent but grouped with the gate.

**Verification (every plan):** `cargo test --workspace` and
`cargo clippy --workspace` from the repo root. Build the app with `cargo build -p lgtm`.

Considered and rejected (do not re-audit): transitive duplicate crate versions
(hashbrown/rand/itertools ×N) — disjoint subtrees, compile-time noise only, no
correctness risk. Testing gh shell-out wrappers against a live `gh` — flaky; the
serde parsing is already covered.
