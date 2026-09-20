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

# Transport envelope (PR 3): the fixture identity pings a second fixture
# identity, the recipient, with a fixed nonce and a 60 s lifetime.
TRANSPORT_BODY = b'{"kind":"ping","version":1}'
ISSUED_AT = CREATED_AT
EXPIRES_AT = CREATED_AT + 60
# Key rotation (PR 3): the fixture identity rotates to a third fixture key a
# day later, endorsed by the old key and acknowledged by the new one.
ROTATED_AT = CREATED_AT + 86400

FIXTURE_SEED_LABEL = b"nolune/federation/fixture-seed/v1"
PEER_SEED_LABEL = b"nolune/federation/fixture-seed/v1/peer"
ROTATED_SEED_LABEL = b"nolune/federation/fixture-seed/v1/rotated"
FIXTURE_NONCE_LABEL = b"nolune/federation/fixture-nonce/v1"
COMPANION_ID_DOMAIN = b"nolune/federation/companion-id/v1\0"
IDENTITY_SIGNING_DOMAIN = b"nolune/federation/identity/v1\0"
ENVELOPE_SIGNING_DOMAIN = b"nolune/federation/envelope/v1\0"
TRANSPORT_SIGNING_DOMAIN = b"nolune/federation/transport/v1\0"
ROTATION_SIGNING_DOMAIN = b"nolune/federation/rotation/v1\0"

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


class Signer:
    """One Ed25519 key derived from a public label, signed through OpenSSL."""

    def __init__(self, tmp: str, label: bytes, name: str):
        seed = hashlib.sha256(label).digest()
        self.key_der = os.path.join(tmp, f"{name}.der")
        with open(self.key_der, "wb") as handle:
            handle.write(PKCS8_ED25519_PREFIX + seed)
        public_der = run(
            [OPENSSL, "pkey", "-inform", "DER", "-in", self.key_der, "-pubout", "-outform", "DER"]
        )
        self.public_key = public_der[-32:]
        self.companion_id = b64url(
            hashlib.sha256(COMPANION_ID_DOMAIN + self.public_key).digest()
        )
        self.message_path = os.path.join(tmp, f"{name}-message.bin")

    def sign(self, message: bytes) -> bytes:
        with open(self.message_path, "wb") as handle:
            handle.write(message)
        return run(
            [
                OPENSSL, "pkeyutl", "-sign", "-rawin",
                "-inkey", self.key_der, "-keyform", "DER", "-in", self.message_path,
            ]
        )

    def identity(self, created_at: int) -> dict:
        message = canonical(
            IDENTITY_SIGNING_DOMAIN,
            ("u32", VERSION),
            ("bytes", self.companion_id.encode("ascii")),
            ("bytes", self.public_key),
            ("u64", created_at),
        )
        return {
            "version": VERSION,
            "companion_id": self.companion_id,
            "public_key": b64url(self.public_key),
            "created_at": created_at,
            "signature": b64url(self.sign(message)),
        }


def main():
    with tempfile.TemporaryDirectory() as tmp:
        signer = Signer(tmp, FIXTURE_SEED_LABEL, "fixture")
        peer = Signer(tmp, PEER_SEED_LABEL, "peer")
        rotated = Signer(tmp, ROTATED_SEED_LABEL, "rotated")

        identity = signer.identity(CREATED_AT)
        peer_identity = peer.identity(CREATED_AT)
        rotated_identity = rotated.identity(ROTATED_AT)

        envelope_message = canonical(
            ENVELOPE_SIGNING_DOMAIN,
            ("u32", VERSION),
            ("bytes", signer.companion_id.encode("ascii")),
            ("bytes", hashlib.sha256(BODY).digest()),
        )
        envelope = {
            "version": VERSION,
            "sender": signer.companion_id,
            "body": b64url(BODY),
            "signature": b64url(signer.sign(envelope_message)),
        }

        # The transport envelope signs over the body's digest, which it also
        # carries, so a tampered body is caught by the hash and a tampered
        # hash by the signature.
        nonce = hashlib.sha256(FIXTURE_NONCE_LABEL).digest()[:16]
        body_hash = hashlib.sha256(TRANSPORT_BODY).digest()
        transport_message = canonical(
            TRANSPORT_SIGNING_DOMAIN,
            ("u32", VERSION),
            ("bytes", signer.companion_id.encode("ascii")),
            ("bytes", peer.companion_id.encode("ascii")),
            ("bytes", nonce),
            ("u64", ISSUED_AT),
            ("u64", EXPIRES_AT),
            ("bytes", body_hash),
        )
        transport = {
            "version": VERSION,
            "sender": signer.companion_id,
            "recipient": peer.companion_id,
            "nonce": b64url(nonce),
            "issued_at": ISSUED_AT,
            "expires_at": EXPIRES_AT,
            "body_hash": b64url(body_hash),
            "body": b64url(TRANSPORT_BODY),
            "signature": b64url(signer.sign(transport_message)),
        }

        # The rotation binds the old identity to the new one: the old key
        # endorses, the new key proves possession, over the same bytes.
        rotation_message = canonical(
            ROTATION_SIGNING_DOMAIN,
            ("u32", VERSION),
            ("bytes", signer.companion_id.encode("ascii")),
            ("bytes", signer.public_key),
            ("bytes", rotated.companion_id.encode("ascii")),
            ("bytes", rotated.public_key),
            ("u64", ROTATED_AT),
        )
        rotation = {
            "version": VERSION,
            "previous": identity,
            "identity": rotated_identity,
            "rotated_at": ROTATED_AT,
            "endorsement": b64url(signer.sign(rotation_message)),
            "signature": b64url(rotated.sign(rotation_message)),
        }

    for name, value in (
        ("identity_v1.json", identity),
        ("envelope_v1.json", envelope),
        ("peer_identity_v1.json", peer_identity),
        ("rotated_identity_v1.json", rotated_identity),
        ("transport_v1.json", transport),
        ("rotation_v1.json", rotation),
    ):
        with open(os.path.join(HERE, name), "w", encoding="utf-8") as handle:
            json.dump(value, handle, indent=2)
            handle.write("\n")
        print(f"wrote {name}")


if __name__ == "__main__":
    main()
