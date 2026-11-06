"""Isolated tests for metadata-only ACP evidence prototype.

Run from repository root:
  python3 -m unittest discover -s experiments/acp-event-evidence -p 'test_*.py' -v
"""
import importlib.util
from pathlib import Path
import tempfile
import unittest

_spec = importlib.util.spec_from_file_location("acp_evidence_archive", Path(__file__).with_name("archive.py"))
_mod = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_mod)
Archive, Error = _mod.EvidenceArchive, _mod.EvidenceError


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "events"
        self.args = ("wc_agent_run_fixture", "project-fixture", "authority-fixture")

    def archive(self):
        return Archive(self.root, *self.args)

    def test_more_than_live_ring_capacity_reopens_and_pages(self):
        archive = self.archive()
        for i in range(1, 301):
            archive.append(i, "agent_message")
        reopened = self.archive()
        cursor, seen = 0, []
        while True:
            page = reopened.page(cursor, 64)
            self.assertFalse(page["body_recoverable"])
            seen.extend(e["sequence"] for e in page["events"])
            cursor = page["next_sequence"]
            if not page["has_more"]:
                break
        self.assertEqual(seen, list(range(1, 301)))
        self.assertEqual(page["committed_through"], 300)

    def test_fresh_python_process_recovers_committed_metadata(self):
        import json
        import subprocess
        import sys

        a = self.archive()
        for i in range(1, 301):
            a.append(i, "usage")
        reader = """
import importlib.util, json, pathlib, sys
spec = importlib.util.spec_from_file_location("separate_archive", sys.argv[1])
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)
archive = mod.EvidenceArchive(pathlib.Path(sys.argv[2]), *sys.argv[3:])
cursor, seen = 0, []
while True:
    page = archive.page(cursor, 64)
    seen.extend(e["sequence"] for e in page["events"])
    cursor = page["next_sequence"]
    if not page["has_more"]:
        break
print(json.dumps({"sequences": seen, "body_recoverable": page["body_recoverable"]}))
"""
        result = subprocess.run(
            [
                sys.executable, "-B", "-c", reader,
                str(Path(__file__).with_name("archive.py")),
                str(self.root), *self.args,
            ],
            check=True, capture_output=True, text=True, timeout=20,
        )
        data = json.loads(result.stdout)
        self.assertEqual(data["sequences"], list(range(1, 301)))
        self.assertFalse(data["body_recoverable"])

    def test_gap_duplicate_and_quota_fail_closed(self):
        a = self.archive()
        with self.assertRaises(Error):
            a.append(2, "agent_message")
        a.append(1, "terminal")
        for bad in (1, 3, 0):
            with self.assertRaises(Error):
                a.append(bad, "terminal")
        with self.assertRaises(Error):
            a.append(2, "content_with_password")
        for i in range(2, 513):
            a.append(i, "tool_call")
        with self.assertRaises(Error):
            a.append(513, "terminal")

    def test_torn_manifest_and_missing_segment_are_not_recoverable(self):
        a = self.archive()
        a.append(1, "terminal")
        (self.root / "manifest.json").write_bytes(b"{")
        with self.assertRaises(Error):
            self.archive()

    def test_corrupt_segment_is_rejected(self):
        a = self.archive()
        a.append(1, "terminal")
        (self.root / "00000001.json").write_bytes(b'{"sequence":1,"kind":"tool_call"}\n')
        with self.assertRaises(Error):
            self.archive()

    def test_orphan_after_segment_before_manifest_is_detected(self):
        self.archive()
        (self.root / "00000001.json").write_text('{"sequence":1,"kind":"terminal"}\n')
        with self.assertRaises(Error):
            self.archive()

    def test_missing_committed_segment_is_rejected(self):
        a = self.archive()
        a.append(1, "terminal")
        (self.root / "00000001.json").unlink()
        with self.assertRaises(Error):
            self.archive()

    def test_root_symlink_is_rejected(self):
        other = Path(self.temp.name) / "alternate"
        other.mkdir()
        link = Path(self.temp.name) / "link"
        link.symlink_to(other, target_is_directory=True)
        with self.assertRaises(Error):
            Archive(link, *self.args)

    def test_uncommitted_extra_file_after_manifest_is_rejected(self):
        a = self.archive()
        a.append(1, "terminal")
        (self.root / "00000002.json").write_text('{"sequence":2,"kind":"terminal"}\\n')
        with self.assertRaises(Error):
            self.archive()

    def test_identity_mismatch_and_invalid_cursor(self):
        a = self.archive()
        a.append(1, "terminal")
        with self.assertRaises(Error):
            Archive(self.root, self.args[0], self.args[1], "different-authority")
        with self.assertRaises(Error):
            a.page(after=2)
        with self.assertRaises(Error):
            a.page(limit=65)
        with self.assertRaises(Error):
            a.page(after=-1)

    def test_page_does_not_claim_event_body_recovery(self):
        a = self.archive()
        a.append(1, "agent_message")
        page = a.page()
        self.assertEqual(page["events"], [{"kind": "agent_message", "sequence": 1}])
        self.assertFalse(page["body_recoverable"])
        self.assertFalse(page["has_more"])


if __name__ == "__main__":
    unittest.main()
