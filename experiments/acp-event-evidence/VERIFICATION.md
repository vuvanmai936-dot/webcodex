# Isolated ACP event-evidence prototype — verification record

Date: 2026-11-06 (Asia/Singapore). Branch: `research/acp-event-evidence-20261106`.
Target: standalone `experiments/acp-event-evidence/archive.py`, **not** wired to WebCodex Runner or ACP transport.

## Implementation delivered

- Bounded metadata-only (sequence + allowlisted event kind) archive.
- Event segment is fsync'd and atomically published *before* its manifest entry; manifest records SHA-256 and lengths.
- Reopening validates identities, segment continuity, hashes, file presence, unexpected files, total bytes and quotas, failing closed on corrupted/partial evidence.
- Explicit `body_recoverable=false` to prevent misrepresenting metadata as an event-body archive.
- Prototype limits: 512 events, 128 KiB total, up to 64 events per page, single writer only.
- Does not persist raw ACP text, thoughts, prompt, tool arguments or secrets.

## Actual execution evidence

Tests were fetched from the GitHub research branch and copied into a Runner-owned execution script's Python temporary directory, then executed with:

`python3 -B -m unittest discover -s <isolated-temp-dir> -p 'test_*.py' -v`

The script used `tempfile.TemporaryDirectory` and removed the test copy on completion; no production WebCodex configuration or existing files were changed.

| Verification | WebCodex Job ID | Result |
| --- | --- | --- |
| Initial prototype suite | `wc_job_XmCwQSW397ApdvMH` | 7 tests passed, exit code 0, 10.986 s |
| Missing-segment/symlink/orphan additions | `wc_job_gyjJUiOehs9wre8C` | 10 tests passed, exit code 0, 11.315 s |
| Separate-process recovery addition | `wc_job_XAczGank19TJHaDo` | **11 tests passed**, exit code **0**, 14.687 s |

The final test suite includes an actual fresh Python interpreter process reopening the archive and enumerating all 300 committed metadata entries. This is **cross-process metadata recovery**, not ACP provider/Runner recovery and not recovery of full ACP payload bodies.

## Important limits and next gates

1. **No actual Runner integration**; the compiled Rust workspace, production ACP APIs, cancellation and authorization were not modified or tested.
2. **Not production secure**: single writer only; no file-locking, principal authorization, expiration service, encryption, sensitive event redaction or full path ancestry hardening. Stored authority strings are a prototype tuple check, not an access-control decision.
3. An interrupted segment-before-manifest commit is detected as an orphan and blocks reopening pending manual reconciliation; automatic safe replay/repair remains future work.
4. Only metadata is preserved. It cannot reconstruct the 2605 dropped ACP event bodies in the real batch-4 failure. Full-body archive needs distinct maintainer approval and privacy/threat review.
5. Subsequent implementation should use Runner-approved Rust storage patterns and policy rather than importing this Python prototype as production runtime code.
6. Nested Codex MCP `unknown_project` same-principal reproduction remains separate and **not completed**.
7. No upstream Issue or PR has been posted for this prototype; no claim of upstream acceptance.
