#!/usr/bin/env python3
"""Regenerate the federation wire fixtures with an implementation that is
independent of the server's Rust code: the canonical bytes are assembled here
and signed by OpenSSL (3.x, Ed25519), so the Rust verifier is checked against a
second implementation rather than against itself.

The signing key is derived from a fixed public string, so it is test material,
not a secret. The Rust unit tests derive the same key and must reproduce these
files byte for byte (Ed25519 signatures are deterministic).

Usage: python3 server/tests/fixtures/federation/generate.py [path/to/openssl]
"""

import base64
import hashlib
import json
import os
import struct
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
OPENSSL = sys.argv[1] if len(sys.argv) > 1 else "openssl"

VERSION = 1
CREATED_AT = 1789862400  # 2026-09-20T00:00:00Z
BODY = b'{"kind":"ping"}'

FIXTURE_SEED_LABEL = b"nolune/federation/fixture-seed/v1"
COMPANION_ID_DOMAIN = b"nolune/federation/companion-id/v1\0"
IDENTITY_SIGNING_DOMAIN = b"nolune/federation/identity/v1\0"
ENVELOPE_SIGNING_DOMAIN = b"nolune/federation/envelope/v1\0"

# PKCS#8 DER prefix for an Ed25519 private key; the 32-byte seed follows.
PKCS8_ED25519_PREFIX = bytes.fromhex("302e020100300506032b657004220420")


def b64url(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode("ascii")


def canonical(domain: bytes, *fields) -> bytes:
    """Domain tag, then each field: u32/u64 big-endian, or u32 length + bytes."""
    out = bytearray(domain)
    for kind, value in fields:
        if kind == "u32":
            out += struct.pack(">I", value)
        elif kind == "u64":
            out += struct.pack(">Q", value)
        elif kind == "bytes":
            out += struct.pack(">I", len(value)) + value
        else:
            raise ValueError(kind)
    return bytes(out)


def run(args, stdin=None):
    return subprocess.run(args, input=stdin, check=True, capture_output=True).stdout


def main():
    seed = hashlib.sha256(FIXTURE_SEED_LABEL).digest()
    with tempfile.TemporaryDirectory() as tmp:
        key_der = os.path.join(tmp, "key.der")
        with open(key_der, "wb") as handle:
            handle.write(PKCS8_ED25519_PREFIX + seed)
        public_der = run(
            [OPENSSL, "pkey", "-inform", "DER", "-in", key_der, "-pubout", "-outform", "DER"]
        )
        public_key = public_der[-32:]

        def sign(message: bytes) -> bytes:
            message_path = os.path.join(tmp, "message.bin")
            with open(message_path, "wb") as handle:
                handle.write(message)
            return run(
                [
                    OPENSSL, "pkeyutl", "-sign", "-rawin",
                    "-inkey", key_der, "-keyform", "DER", "-in", message_path,
                ]
            )

        companion_id = b64url(hashlib.sha256(COMPANION_ID_DOMAIN + public_key).digest())

        identity_message = canonical(
            IDENTITY_SIGNING_DOMAIN,
            ("u32", VERSION),
            ("bytes", companion_id.encode("ascii")),
            ("bytes", public_key),
            ("u64", CREATED_AT),
        )
        identity = {
            "version": VERSION,
            "companion_id": companion_id,
            "public_key": b64url(public_key),
            "created_at": CREATED_AT,
            "signature": b64url(sign(identity_message)),
        }

        envelope_message = canonical(
            ENVELOPE_SIGNING_DOMAIN,
            ("u32", VERSION),
            ("bytes", companion_id.encode("ascii")),
            ("bytes", hashlib.sha256(BODY).digest()),
        )
        envelope = {
            "version": VERSION,
            "sender": companion_id,
            "body": b64url(BODY),
            "signature": b64url(sign(envelope_message)),
        }

    for name, value in (("identity_v1.json", identity), ("envelope_v1.json", envelope)):
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            json.dump(value, handle, indent=2)
            handle.write("\n")
        print(f"wrote {name}")


if __name__ == "__main__":
    main()
