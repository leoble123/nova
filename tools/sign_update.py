"""Writes the signed update manifest (latest.json) for a release. Run by CI.

    NOVA_UPDATE_KEY=<hex ed25519 private key> python tools/sign_update.py <Nova.exe> <version> <download-url> <notes> > latest.json

Signs "nova-update\\n<version>\\n<sha256>", which the app verifies with the public key compiled
into it (src-tauri/src/update.rs). Generate a new key pair with: python tools/sign_update.py --keygen
"""
import hashlib, json, os, sys
from datetime import datetime, timezone
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization as ser

def message(version, sha):
    return f"nova-update\n{version}\n{sha.lower()}".encode()

if len(sys.argv) > 1 and sys.argv[1] == "--keygen":
    k = Ed25519PrivateKey.generate()
    raw = k.private_bytes(ser.Encoding.Raw, ser.PrivateFormat.Raw, ser.NoEncryption())
    pub = k.public_key().public_bytes(ser.Encoding.Raw, ser.PublicFormat.Raw)
    print("private (store as the NOVA_UPDATE_KEY secret, never commit):", raw.hex())
    print("public  (put in PUBLIC_KEY in src-tauri/src/update.rs):    ", pub.hex())
    sys.exit(0)

exe, version, url = sys.argv[1], sys.argv[2], sys.argv[3]
notes = sys.argv[4] if len(sys.argv) > 4 else ""
key = Ed25519PrivateKey.from_private_bytes(bytes.fromhex(os.environ["NOVA_UPDATE_KEY"].strip()))
data = open(exe, "rb").read()
sha = hashlib.sha256(data).hexdigest()
sig = key.sign(message(version, sha)).hex()
print(json.dumps({
    "version": version,
    "notes": notes,
    "pub_date": datetime.now(timezone.utc).isoformat(timespec="seconds"),
    "platforms": {"windows-x86_64": {"url": url, "sha256": sha, "signature": sig, "size": len(data)}},
}, indent=2))
