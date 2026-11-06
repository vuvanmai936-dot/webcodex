# RFC design note: bounded ACP event evidence (proposal only)

This document is a **design candidate**, not an enabled feature or a claim that its implementation passed tests. It accompanies [the contribution plan](acp-event-evidence-contribution-plan.md). No changes to the active Server/Runner/Tunnel are authorized or necessary for this document.

## Existing contract and gap

The Runner uses a bounded in-memory `LiveRunState` event ring (256 entries), paged observations (up to 64 events per response), truthful `history_lost` and `first_retained_sequence`, and a separate durable `DurableRunRecord` with identity, dispatch, execution and terminal fields. The bounded ring is documented and tested; it is **not itself a bug**. The durable record does not contain ACP event bodies. After an observer misses more than 256 events, history cannot be reconstructed from the Run state.

A durable archive must not be conflated with the pre-prompt irreversible-dispatch barrier. The existing barrier protects **whether** a prompt may have executed; it does not establish an atomic commit of every later event. Archive completeness therefore needs its own explicit consistency model.

## Proposed tiers and privacy policy

1. **Baseline/default (unchanged):** 256-event in-memory ring, existing tool API, no raw event-body persistence.
2. **Evidence-only (possible initial increment):** persist minimal sequence/index, normalized kind, bounded timestamps and integrity/continuity metadata; no prompt text, agent thoughts, tool arguments, file contents, environment, secrets, raw ACP JSON-RPC or provider session IDs. This tier detects loss but **cannot recover full event bodies**. It must never claim that it can.
3. **Full-event opt-in (separate approval):** archive *sanitized normalized* event payloads as emitted to authorized observers, not raw ACP transport. Must have explicit user/operator opt-in and per-Run authorization, storage/retention budget and redaction review. Highly sensitive model thoughts or tool content require exclusion or separately declared policy.

Do not overload `DurableRunRecord`'s 64 KiB state file with event batches. Prefer append-only, individually bounded segments under the already owned Run namespace with a separately versioned manifest. Keep archive identity bound to the same `run_id`, `authority_fingerprint`, exact `runtime_project_id` and Runner namespace.

## Crash-consistent commit contract

- Allocate strictly increasing event sequences in the existing Run event producer.
- Normalize/redact before any disk publication; zero unneeded raw data as feasible.
- Append bounded segment containing `run_id`, sequence interval, payload version and integrity digest.
- Publish segment data atomically; then atomically publish/advance a manifest high-water mark. An acknowledged durable event must remain discoverable after a restart.
- On restart, verify manifest-to-segment continuity and identity. A missing, malformed, truncated, out-of-order or hash-mismatched segment must produce **explicit uncertainty**, not a complete replay.
- Do not infer the existence of uncommitted events from a higher terminal sequence: gap status and high-water semantics must distinguish producer-emitted, durably-committed and consumer-delivered ranges.
- Preserve existing `history_lost` semantics for legacy ring-only observation. Any new archive fetch pathway needs a separately named completeness / availability status; avoid changing the meaning of existing fields.
- Never replay `start_coding_agent` or `session/prompt` because events are missing. Archive retrieval is read-only and must not affect Run dispatch or cancellation transitions.

## Bounds and lifecycle

Candidate questions, not accepted default values: total bytes per Run, per event and per segment; elapsed retention; global per-Runner cap; cleanup under disk pressure; interaction with existing 15-minute terminal retention; behavior after authority revocation or project unregistration; compatibility with provider changes and Runner restart. Do not choose arbitrary production defaults before maintainers review threat model and telemetry.

Persisting every normalized token chunk may be prohibitively expensive. Instrument volume and throughput in isolated tests. Consider content-free metadata first, or batching after normalization, but document how batching interacts with sequence completeness and crash boundaries.

## Isolation and authorization

- Archive reads must revalidate current caller/session principal, Run authority, project registration and exact Runner identity; possession of Run ID or observation token alone must not grant access.
- Archive state must not silently outlive the corresponding authorization mechanism. Secure deletion/retention obligations need a design decision.
- Reject unexpected symlinks and path traversal; construct storage paths only from encoded/hashed Run identities.
- Do not expose raw ACP session IDs, credentials, environment or local absolute paths in public tool errors.
- Audit aggregate read/denial events without logging archived payload bodies.

## Deterministic tests to implement before feature claim

| Fixture | Expected outcome |
| --- | --- |
| Legacy >256 event fixture | Existing ring observation returns `history_lost=true`, no false recovery |
| Opt-in >256 event fixture + new observer | Every committed sequence is obtainable, in order, without duplicated dispatch |
| Fresh Runner process + identical authorized Run | Committed archive pages can be replayed; uncertain Run dispatch status remains unchanged |
| Torn segment or torn manifest write | Explicit incomplete/corrupt result, no invented sequence |
| Missing middle segment | Gap is reported even if later segments exist |
| Expired archive | Explicit expiry/unavailable result, never false `history_lost=false` |
| Unauthorized/revoked principal | No events or event metadata disclosed |
| Oversize event, quota and disk full | Bounded failure; conservative uncertainty and safe cleanup |
| Legacy caller unaware of new feature | Current schema, retention and pagination remain unchanged |

Completion of source changes and passing tests are separate deliverables. Upstream architectural review is necessary before treating this as a compatible production feature.

## Auxiliary investigation: nested Codex MCP `unknown_project`

Do not equate successful ChatGPT MCP file access to Codex ACP nested MCP authorization. Build a disposable same-file A/B matrix with exact normalized Project ID, Runner selection, client principal, tool schema and project registration timing. Capture sanitized request ID and error kind without credentials. If effective principals differ, rejection may be intended. Only claim a bug once identical authorized identity and project binding reproducibly diverge.

## Requested maintainer feedback

1. Is an opt-in archive in scope for CodingAgentRun, or is loss beyond the bounded ring intentional for all consumers?
2. Would metadata-only continuity evidence be a useful separate first increment?
3. Is the existing Runner durable namespace suitable for segment storage, and what retention limits are acceptable?
4. Should any new archive retrieval use a separate API to avoid changing `observe_coding_agent` pagination semantics?
