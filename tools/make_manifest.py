"""Writes the update manifest (latest.json) for a release. Run by CI.

    python tools/make_manifest.py <Nova.exe> <version> <download-url> <notes> > latest.json

Installed copies read it from the latest GitHub Release and accept the download only if its
SHA-256 matches (src-tauri/src/update.rs).
"""
import hashlib, json, sys
from datetime import datetime, timezone

exe, version, url = sys.argv[1], sys.argv[2], sys.argv[3]
notes = sys.argv[4] if len(sys.argv) > 4 else ""
data = open(exe, "rb").read()
print(json.dumps({
    "version": version,
    "notes": notes,
    "pub_date": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    "platforms": {"windows-x86_64": {"url": url, "sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}},
}, indent=2))
