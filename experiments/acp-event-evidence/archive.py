"""Isolated ACP *metadata-only* evidence archive prototype; NOT wired to WebCodex.

Never feed ACP prompt text, thoughts, tool args or file content to this module.
Single writer only. No production authorization or retention service is implemented.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re

MAX_EVENTS = 512
MAX_BYTES = 128 * 1024
ALLOWED_KINDS = frozenset({"agent_message", "tool_call", "tool_update", "usage", "terminal"})
_ID = re.compile(r"^[A-Za-z0-9_:-]{1,128}$")


class EvidenceError(Exception):
    """Evidence cannot be trusted or accepted."""


def _json(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode()


def _digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _check_id(value: str) -> str:
    if not isinstance(value, str) or not _ID.fullmatch(value):
        raise EvidenceError("invalid identity")
    return value


def _atomic(path: Path, data: bytes) -> None:
    # Temporary file and destination must share a directory/filesystem.
    import tempfile
    tmp = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=".staging-", delete=False) as f:
            tmp = Path(f.name)
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, path)
        tmp = None
        if hasattr(os, "O_DIRECTORY"):
            fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(fd)
            finally:
                os.close(fd)
    finally:
        if tmp is not None:
            tmp.unlink(missing_ok=True)


class EvidenceArchive:
    """Single-process demo of per-event segments + committed manifest.

    Readers must provide exact same Run/project/authority tuple. This binding
    is *not* a replacement for actual WebCodex principal authorization.
    """

    def __init__(self, root: Path, run_id: str, project: str, authority: str):
        self.root = Path(root)
        self.identity = {
            "run_id": _check_id(run_id),
            "project": _check_id(project),
            "authority": _check_id(authority),
        }
        if self.root.is_symlink():
            raise EvidenceError("archive root symlink")
        self.root.mkdir(parents=True, exist_ok=True)
        self.manifest = self.root / "manifest.json"
        self._load()

    def _load(self) -> dict:
        if not self.manifest.exists():
            if any(self.root.iterdir()):
                raise EvidenceError("orphan or incomplete archive; manual reconcile")
            return {"version": 1, "identity": self.identity, "entries": [], "bytes": 0}
        if self.manifest.is_symlink():
            raise EvidenceError("manifest symlink")
        try:
            data = json.loads(self.manifest.read_bytes())
            if (set(data) != {"version", "identity", "entries", "bytes"}
                    or data["version"] != 1 or data["identity"] != self.identity
                    or not isinstance(data["entries"], list) or not isinstance(data["bytes"], int)):
                raise EvidenceError("manifest schema or authority mismatch")
            total = 0
            if len(data["entries"]) > MAX_EVENTS:
                raise EvidenceError("excess events")
            for i, entry in enumerate(data["entries"], 1):
                if (set(entry) != {"sequence", "sha256", "size"}
                        or type(entry["sequence"]) is not int or entry["sequence"] != i
                        or type(entry["size"]) is not int or not 0 < entry["size"] <= 1024
                        or not isinstance(entry["sha256"], str)
                        or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"])):
                    raise EvidenceError("invalid segment index")
                segment = self.root / f"{i:08d}.json"
                if segment.is_symlink() or not segment.is_file():
                    raise EvidenceError("missing or linked segment")
                blob = segment.read_bytes()
                if len(blob) != entry["size"] or _digest(blob) != entry["sha256"]:
                    raise EvidenceError("corrupt segment")
                obj = json.loads(blob)
                if obj != {"sequence": i, "kind": obj.get("kind")} or obj["kind"] not in ALLOWED_KINDS:
                    raise EvidenceError("invalid segment content")
                total += len(blob)
            if total != data["bytes"] or total > MAX_BYTES:
                raise EvidenceError("invalid committed size")
            # An unlisted segment is a possible interrupted write, not confirmed evidence.
            expected = {"manifest.json"} | {f"{i:08d}.json" for i in range(1, len(data["entries"]) + 1)}
            if {p.name for p in self.root.iterdir()} != expected:
                raise EvidenceError("uncommitted or unexpected files; manual reconcile")
            return data
        except EvidenceError:
            raise
        except (OSError, ValueError, TypeError, KeyError, AttributeError) as exc:
            raise EvidenceError("unreadable evidence; fail closed") from exc

    def append(self, sequence: int, kind: str) -> None:
        state = self._load()
        if type(sequence) is not int or sequence != len(state["entries"]) + 1:
            raise EvidenceError("sequence gap or duplicate")
        if kind not in ALLOWED_KINDS:
            raise EvidenceError("only classified metadata can be stored")
        if len(state["entries"]) >= MAX_EVENTS:
            raise EvidenceError("event quota")
        blob = _json({"sequence": sequence, "kind": kind})
        if state["bytes"] + len(blob) > MAX_BYTES:
            raise EvidenceError("byte quota")
        segment = self.root / f"{sequence:08d}.json"
        if segment.exists() or segment.is_symlink():
            raise EvidenceError("segment already exists")
        # Segment first, manifest last: a crash leaves a visible orphan, never
        # a falsely committed segment. The operator must reconcile or discard.
        _atomic(segment, blob)
        next_state = {
            **state,
            "entries": state["entries"] + [{
                "sequence": sequence, "sha256": _digest(blob), "size": len(blob)
            }],
            "bytes": state["bytes"] + len(blob),
        }
        _atomic(self.manifest, _json(next_state))

    def page(self, after: int = 0, limit: int = 64) -> dict:
        if type(after) is not int or after < 0 or type(limit) is not int or not 1 <= limit <= 64:
            raise EvidenceError("invalid cursor or page bound")
        state = self._load()
        high_water = len(state["entries"])
        if after > high_water:
            raise EvidenceError("cursor exceeds committed evidence")
        result = []
        for seq in range(after + 1, min(after + limit, high_water) + 1):
            result.append(json.loads((self.root / f"{seq:08d}.json").read_bytes()))
        return {
            "events": result, "committed_through": high_water,
            "next_sequence": result[-1]["sequence"] if result else after,
            "has_more": after + len(result) < high_water,
            "body_recoverable": False,
        }
