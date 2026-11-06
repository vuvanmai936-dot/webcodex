# WebCodex ACP reliability contribution plan (2026-11-06)

Status: research, not an upstream acceptance claim. Owner fork: `vuvanmai936-dot/webcodex`; isolated branch: `research/acp-event-evidence-20261106`.

## Objective and evidence

Improve evidence continuity for long-running CodingAgentRun observations without a second scheduler, UI, or ACP dispatch path. Prior real integrations observed `history_lost=true` across consumer conversations: batch 4 archived 1–64 and next saw 2670 onward (missing 65–2669); batch 10 retained 238 onward following 1–2; batch 9 reported provider usage-limit `failed/prompt_error`; another run was `cancelled` with no supported evidence of a bug. These are integration observations, not deterministic upstream reproduction.

Source baseline: `CODING_AGENT_MAX_RETAINED_EVENTS=256`, max 64 events per observe response; `LiveRunState` stores a bounded event ring. `DurableRunRecord` persists identity, dispatch phase, execution state, terminal metadata, not event bodies, with a terminal retention window. Existing upstream `coding_agent/tests/protocol.rs` already asserts honest `history_lost=true` on overflow; repeating that test adds little.

## Plan and stage gates

1. **Baseline and safety (read-only).** Confirm fork/upstream revisions, inspect CodingAgentRun observation contract, Runner durable store, existing tests and issue/PR overlap. Do not modify live WebCodex Server/Runner/Tunnel or the existing `main` and historical ACP branch.
2. **Minimal contract design.** Prepare an opt-in, bounded, Runner-owned ACP evidence archive proposal. Define exact Run/project/authority binding; max bytes, count and age; filtering of prompt/credentials; page cursor semantics; replay and gaps; write-commit barriers; store corruption and unavailable-state behavior; retention interaction with Run identity; backwards compatibility. Default must **not** persist sensitive raw content.
3. **Tests before implementation.** Add deterministic fixture of >256 events, disconnected consumer and process restart. Separate existing bounded observation behavior from opted-in archival recovery; test truncated writes, revoked authority, expiry, missing records, and no false `history_lost=false`. Do not falsely claim persistence based on in-memory-only tests.
4. **Incremental implementation (conditional).** Only once design and current code surface make a small, reviewable change possible; no new scheduler/UI/API by default. Keep the existing `CodingAgentRun` identity, safe no-redispatch behavior, and current tool semantics. Prefer a contract/diagnostic-only PR if storage design is not approved by maintainers.
5. **Independent `unknown_project` track.** Compare ChatGPT-to-WebCodex and nested Codex ACP-to-MCP against the *same* disposable registered project, normalized Project ID and test file. Record principal, adapter version, exact error and permission policy without exposing credentials. If principal/policy differ, access rejection may be correct. Do not raise a bug until same-authority mismatch is reproduced.
6. **Verification / upstream decision.** Run targeted unit/integration tests in a disposable environment, inspect diffs and licenses, document failures and coverage. Then open a narrowly scoped upstream RFC or PR with real repro and migration/safety bounds, never invent test results or represent a design limit as a confirmed bug.

## Acceptance

- Existing ACP observation protocol and default event retention continue to work; overflow keeps reporting truthful gaps.
- Optional archive (if accepted) proves sequence-complete committed evidence across observer disconnect and Runner restart, bounded by explicit limits and permissions.
- Corrupt, expired or incomplete evidence reports uncertainty or a gap rather than fabricated completeness.
- No duplicate ACP dispatch, no second task system, and no linkage that treats `completed` as successful source delivery.
- Root-cause evidence and reproducible test required for any `unknown_project` bug PR.

## Current execution record

- Authenticated GitHub account: `vuvanmai936-dot`; writable fork confirmed.
- Branch `research/acp-event-evidence-20261106` created from fork `main`; old `feat/acp-coding-agent-bridge` is heavily diverged and intentionally untouched.
- Upstream existing overflow regression: `crates/webcodex-runner/src/webcodex_runner/coding_agent/tests/protocol.rs`; no duplicate test proposed.
- Current local runtime fingerprint remains unverified because the runtime status MCP action was unavailable on this conversation surface.
- **No tests run, no source implementation, no upstream Issue/PR published as of this plan commit.**

## External links

- Upstream: https://github.com/yyjeqhc/webcodex
- Prior accepted bug fix: https://github.com/yyjeqhc/webcodex/pull/1359
- ACP architecture: `docs/agent/acp-coding-agent-run.md`
- ACP Runner durable store: `crates/webcodex-runner/src/webcodex_runner/coding_agent/store.rs`
