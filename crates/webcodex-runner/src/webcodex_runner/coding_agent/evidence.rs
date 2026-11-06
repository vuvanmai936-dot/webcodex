//! Test-only Rust ACP sequence-evidence candidate, not enabled in production.
//!
//! Linked only by #[cfg(test)] in webcodex-runner; stores no ACP body or prompt contents.
//! Single-writer only. No principal authorization, retention service, or
//! production integration; a matching identity string is NOT an ACL check.
//!
//! Standalone test command (when rustc is available):
//!   rustc --edition=2021 --test evidence.rs -o /tmp/acp-evidence-tests
//!   /tmp/acp-evidence-tests
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_EVENTS: u64 = 512;
const MAX_BYTES: u64 = 128 * 1024;
const MAX_PAGE: u64 = 64;
const VERSION: &str = "v1";

#[derive(Debug, PartialEq, Eq)]
pub enum EvidenceError {
    InvalidInput,
    IdentityMismatch,
    GapOrDuplicate,
    QuotaExceeded,
    IncompleteOrCorrupt,
    Io,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub sequence: u64,
    pub kind: String,
}

#[derive(Debug)]
pub struct Page {
    pub events: Vec<Evidence>,
    pub committed_through: u64,
    pub next_sequence: u64,
    pub has_more: bool,
    /// Metadata-only evidence does not recover discarded ACP event bodies.
    pub body_recoverable: bool,
}

pub struct Archive {
    root: PathBuf,
    identity: String,
}

fn allowed_identity(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-".contains(&b))
}

fn allowed_kind(s: &str) -> bool {
    matches!(
        s,
        "agent_message"
            | "reasoning"
            | "plan"
            | "tool_activity"
            | "file_change"
            | "terminal_activity"
            | "usage"
            | "permission_request"
            | "terminal"
    )
}

// CRC32 guards against accidental corruption, not intentional tampering.
// Production storage would use SHA-256 from the existing Runner dependencies.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb88320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn sync_dir(path: &Path) -> Result<(), EvidenceError> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|_| EvidenceError::Io)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn publish(path: &Path, bytes: &[u8]) -> Result<(), EvidenceError> {
    let parent = path.parent().ok_or(EvidenceError::InvalidInput)?;
    let tmp = path.with_extension("pending");
    if tmp.exists() {
        return Err(EvidenceError::IncompleteOrCorrupt);
    }
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|_| EvidenceError::Io)?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| EvidenceError::Io)?;
        drop(file);
        fs::rename(&tmp, path).map_err(|_| EvidenceError::Io)?;
        sync_dir(parent)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

impl Archive {
    pub fn new(
        root: PathBuf,
        run: &str,
        project: &str,
        authority: &str,
    ) -> Result<Self, EvidenceError> {
        if !allowed_identity(run) || !allowed_identity(project) || !allowed_identity(authority) {
            return Err(EvidenceError::InvalidInput);
        }
        if root.is_symlink() {
            return Err(EvidenceError::InvalidInput);
        }
        fs::create_dir_all(&root).map_err(|_| EvidenceError::Io)?;
        let archive = Self {
            root,
            identity: format!("{VERSION}\n{run}\n{project}\n{authority}\n"),
        };
        archive.inspect()?;
        Ok(archive)
    }

    fn manifest(&self) -> PathBuf {
        self.root.join("manifest")
    }

    fn segment(&self, sequence: u64) -> PathBuf {
        self.root.join(format!("{sequence:08}.event"))
    }

    fn inspect(&self) -> Result<Vec<(u64, String, u32)>, EvidenceError> {
        let manifest = self.manifest();
        if manifest.is_symlink() {
            return Err(EvidenceError::IncompleteOrCorrupt);
        }
        if !manifest.exists() {
            let mut dir = fs::read_dir(&self.root).map_err(|_| EvidenceError::Io)?;
            if dir.next().is_some() {
                return Err(EvidenceError::IncompleteOrCorrupt);
            }
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(&manifest).map_err(|_| EvidenceError::IncompleteOrCorrupt)?;
        let (identity, entries) = text
            .split_once("--\n")
            .ok_or(EvidenceError::IncompleteOrCorrupt)?;
        if format!("{identity}--\n") != format!("{}--\n", self.identity) {
            return Err(EvidenceError::IdentityMismatch);
        }
        let mut parsed = Vec::new();
        let mut bytes = 0_u64;
        for line in entries.lines() {
            if parsed.len() as u64 >= MAX_EVENTS {
                return Err(EvidenceError::QuotaExceeded);
            }
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() != 3 {
                return Err(EvidenceError::IncompleteOrCorrupt);
            }
            let sequence = fields[0]
                .parse::<u64>()
                .map_err(|_| EvidenceError::IncompleteOrCorrupt)?;
            let kind = fields[1];
            let expected = u32::from_str_radix(fields[2], 16)
                .map_err(|_| EvidenceError::IncompleteOrCorrupt)?;
            if sequence != parsed.len() as u64 + 1 || !allowed_kind(kind) {
                return Err(EvidenceError::IncompleteOrCorrupt);
            }
            let segment = self.segment(sequence);
            if segment.is_symlink() {
                return Err(EvidenceError::IncompleteOrCorrupt);
            }
            let content = fs::read(&segment).map_err(|_| EvidenceError::IncompleteOrCorrupt)?;
            let expected_content = format!("{sequence}\t{kind}\n");
            if content != expected_content.as_bytes() || crc32(&content) != expected {
                return Err(EvidenceError::IncompleteOrCorrupt);
            }
            bytes += content.len() as u64;
            if bytes > MAX_BYTES {
                return Err(EvidenceError::QuotaExceeded);
            }
            parsed.push((sequence, kind.to_owned(), expected));
        }
        let expected_files = parsed.len() + 1;
        let actual_files = fs::read_dir(&self.root)
            .map_err(|_| EvidenceError::Io)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| EvidenceError::Io)?;
        if actual_files.len() != expected_files
            || actual_files.iter().any(|item| {
                let path = item.path();
                if path == manifest {
                    return false;
                }
                !parsed
                    .iter()
                    .any(|(sequence, _, _)| path == self.segment(*sequence))
            })
        {
            return Err(EvidenceError::IncompleteOrCorrupt);
        }
        Ok(parsed)
    }

    pub fn append(&self, sequence: u64, kind: &str) -> Result<(), EvidenceError> {
        if !allowed_kind(kind) {
            return Err(EvidenceError::InvalidInput);
        }
        let previous = self.inspect()?;
        if sequence != previous.len() as u64 + 1 {
            return Err(EvidenceError::GapOrDuplicate);
        }
        if sequence > MAX_EVENTS {
            return Err(EvidenceError::QuotaExceeded);
        }
        let content = format!("{sequence}\t{kind}\n");
        let used: u64 = previous
            .iter()
            .map(|(seq, k, _)| format!("{seq}\t{k}\n").len() as u64)
            .sum();
        if used + content.len() as u64 > MAX_BYTES {
            return Err(EvidenceError::QuotaExceeded);
        }
        if self.segment(sequence).exists() {
            return Err(EvidenceError::IncompleteOrCorrupt);
        }
        // Segment before manifest. On crash, orphan is detected and blocks reads.
        publish(&self.segment(sequence), content.as_bytes())?;
        let mut manifest = self.identity.clone();
        manifest.push_str("--\n");
        for (seq, k, digest) in previous {
            manifest.push_str(&format!("{seq}\t{k}\t{digest:08x}\n"));
        }
        manifest.push_str(&format!(
            "{sequence}\t{kind}\t{:08x}\n",
            crc32(content.as_bytes())
        ));
        publish(&self.manifest(), manifest.as_bytes())
    }

    pub fn page(&self, after: u64, limit: u64) -> Result<Page, EvidenceError> {
        if limit == 0 || limit > MAX_PAGE {
            return Err(EvidenceError::InvalidInput);
        }
        let entries = self.inspect()?;
        let high = entries.len() as u64;
        if after > high {
            return Err(EvidenceError::InvalidInput);
        }
        let events: Vec<Evidence> = entries
            .into_iter()
            .skip(after as usize)
            .take(limit as usize)
            .map(|(sequence, kind, _)| Evidence { sequence, kind })
            .collect();
        let next_sequence = events.last().map_or(after, |event| event.sequence);
        Ok(Page {
            events,
            committed_through: high,
            next_sequence,
            has_more: next_sequence < high,
            body_recoverable: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "wc-acp-metadata-{}-{nanos}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            Self(root)
        }
        fn archive(&self) -> Archive {
            Archive::new(
                self.0.clone(),
                "wc_agent_run_test",
                "project-test",
                "authority-test",
            )
            .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn three_hundred_events_reopen_and_page() {
        let fixture = Fixture::new();
        let archive = fixture.archive();
        for sequence in 1..=300 {
            archive.append(sequence, "agent_message").unwrap();
        }
        let reopened = fixture.archive();
        let mut cursor = 0;
        let mut sequences = Vec::new();
        loop {
            let page = reopened.page(cursor, 64).unwrap();
            assert!(!page.body_recoverable);
            assert_eq!(page.committed_through, 300);
            sequences.extend(page.events.iter().map(|event| event.sequence));
            cursor = page.next_sequence;
            if !page.has_more {
                break;
            }
        }
        assert_eq!(sequences, (1..=300).collect::<Vec<_>>());
    }

    #[test]
    fn refuses_gaps_and_duplicates() {
        let fixture = Fixture::new();
        let archive = fixture.archive();
        assert_eq!(
            archive.append(2, "terminal"),
            Err(EvidenceError::GapOrDuplicate)
        );
        archive.append(1, "terminal").unwrap();
        assert_eq!(
            archive.append(1, "terminal"),
            Err(EvidenceError::GapOrDuplicate)
        );
        assert_eq!(
            archive.append(3, "terminal"),
            Err(EvidenceError::GapOrDuplicate)
        );
    }

    #[test]
    fn corrupted_segment_fails_closed() {
        let fixture = Fixture::new();
        let archive = fixture.archive();
        archive.append(1, "terminal").unwrap();
        fs::write(archive.segment(1), b"wrong\n").unwrap();
        assert!(matches!(
            archive.page(0, 64),
            Err(EvidenceError::IncompleteOrCorrupt)
        ));
    }

    #[test]
    fn orphan_after_segment_before_manifest_fails_closed() {
        let fixture = Fixture::new();
        let archive = fixture.archive();
        fs::write(archive.segment(1), b"1\tterminal\n").unwrap();
        assert!(matches!(
            archive.page(0, 64),
            Err(EvidenceError::IncompleteOrCorrupt)
        ));
    }

    #[test]
    fn wrong_identity_fails_closed() {
        let fixture = Fixture::new();
        fixture.archive().append(1, "terminal").unwrap();
        assert!(matches!(
            Archive::new(
                fixture.0.clone(),
                "wc_agent_run_test",
                "project-test",
                "other"
            ),
            Err(EvidenceError::IdentityMismatch)
        ));
    }

    #[test]
    fn canonical_coding_agent_event_kinds_match_core_contract() {
        for kind in [
            "agent_message",
            "reasoning",
            "plan",
            "tool_activity",
            "file_change",
            "terminal_activity",
            "usage",
            "permission_request",
            "terminal",
        ] {
            assert!(allowed_kind(kind), "unrecognized canonical kind: {kind}");
        }
        assert!(!allowed_kind("tool_call"));
        assert!(!allowed_kind("tool_update"));
    }


    #[test]
    fn rust_archive_child_read() {
        let Ok(root) = std::env::var("WC_ACP_EVIDENCE_CHILD_ROOT") else {
            return;
        };
        let archive = Archive::new(
            PathBuf::from(root),
            "wc_agent_run_test",
            "project-test",
            "authority-test",
        )
        .unwrap();
        let mut cursor = 0;
        let mut count = 0;
        loop {
            let page = archive.page(cursor, 64).unwrap();
            assert!(!page.body_recoverable);
            assert_eq!(page.committed_through, 300);
            count += page.events.len();
            cursor = page.next_sequence;
            if !page.has_more {
                break;
            }
        }
        assert_eq!(count, 300);
        assert_eq!(cursor, 300);
    }

    #[test]
    fn three_hundred_events_recover_in_fresh_process() {
        let fixture = Fixture::new();
        let archive = fixture.archive();
        for sequence in 1..=300 {
            archive.append(sequence, "tool_activity").unwrap();
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("rust_archive_child_read")
            .arg("--nocapture")
            .env("WC_ACP_EVIDENCE_CHILD_ROOT", &fixture.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "fresh process failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "fresh process did not execute child probe"
        );
    }

    #[test]
    fn missing_middle_segment_never_claims_completeness() {
        let fixture = Fixture::new();
        let archive = fixture.archive();
        for sequence in 1..=3 {
            archive.append(sequence, "terminal_activity").unwrap();
        }
        fs::remove_file(archive.segment(2)).unwrap();
        assert!(matches!(
            archive.page(0, 64),
            Err(EvidenceError::IncompleteOrCorrupt)
        ));
    }

    #[test]
    fn invalid_cursor_is_rejected() {
        let fixture = Fixture::new();
        let archive = fixture.archive();
        archive.append(1, "terminal").unwrap();
        assert!(matches!(
            archive.page(2, 1),
            Err(EvidenceError::InvalidInput)
        ));
        assert!(matches!(
            archive.page(0, 65),
            Err(EvidenceError::InvalidInput)
        ));
    }
}
