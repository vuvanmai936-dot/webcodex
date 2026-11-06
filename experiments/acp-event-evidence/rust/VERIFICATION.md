# Rust ACP evidence candidate — verification and integration gate

Date: 2026-11-06. Branch: `research/acp-event-evidence-20261106`.

## Delivered

Source: `experiments/acp-event-evidence/rust/evidence.rs` (standalone experimental Rust; no `mod` wiring or production runtime modifications).

- Single-writer sequence-ordered, metadata-only evidence store (sequence/kind only).
- Max 512 events and 128 KiB of event payloads, up to 64 events per read.
- Sync temporary segment then atomic publish; sync manifest to acknowledge committed index.
- Reopen validates segment content and CRC32 integrity, exact identity tuple and expected files.
- Explicit `body_recoverable=false` contract.
- Six Rust `#[test]` functions for 300-event reread, gaps/duplicates, tampering, orphan, mismatched identity and invalid cursor.
- CRC32 is only an accidental-corruption check, **not a cryptographic integrity or authenticity claim**.

## What was actually tested

The connected Runner environment reported `rustc None` and `cargo None` on a read-only toolchain check. Thus, **no Rust compile, cargo test, clippy, fmt, or end-to-end ACP test was executed**. Avoid calling these six authored Rust tests “passed”.

A separate Python static-check script read the exact GitHub Rust source and verified:
- Only metadata fields (no model prompt/body fields);
- Segment publication appears before manifest publication;
- Explicit non-body-recoverability field;
- Event/byte quotas;
- Exactly six authored Rust test attributes;
- No production Runner module import.

The first static check failed because its own test-attribute regular expression was improperly escaped; the corrected second validation returned six PASS, exit status success. These are **source-shape checks, not compilation or functional tests**.

Previously, the independent Python metadata prototype passed 11 actual tests, including a fresh-process recovery of 300 entries. Those results **do not transfer** to the Rust implementation.

## Mandatory next validation

On an authorized disposable machine with Rust installed, with the research branch checked out:

```sh
cd experiments/acp-event-evidence/rust
rustc --edition=2021 --test evidence.rs -o ./evidence-tests
./evidence-tests --nocapture
```

The test binary is a local experimental artifact and should not be committed or installed in a production service. Follow with `rustfmt --check evidence.rs` and independent code review.

Before any Runner integration:
1. Replace prototype CRC32 with existing `sha2` and follow the Runner durable-store's cross-platform atomic publish patterns.
2. Add concurrent-writer exclusion, symlink/ancestor hardening, reliable crash recovery or explicit tombstone reconciliation, disk-pressure policy, retention, authorization checks and event-size validation.
3. Keep metadata-only and body-recovery semantics separate. Do not claim recovering missing ACP event payloads.
4. Add a feature-gated, opt-in Runner attachment and independent integration tests **only after Rust tests pass**.
5. Avoid changing the current Run pre-prompt dispatch barrier, event cursor protocol or default 256-event ring.

No upstream Issue or PR, no production configuration change, and no main-branch modification were performed.
