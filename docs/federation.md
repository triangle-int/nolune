# Companion federation (#108)

Two companions can be paired so that their servers verify each other's
signed messages; the companions this one is paired with are its peer
companions. A peer companion is a signing identity, never a machine, a
profile name, a port, or a hostname: the same key is trusted after the
companion moves hosts, and two profiles on one Mac are two peers that pair
exactly like companions on different continents. This page is the owner's
guide; the on-disk shapes, check orders, and limits are in
[companion-storage.md](companion-storage.md) under "Federation identity".

## Identity

Each server holds one Ed25519 signing key, created on first use under the
workspace root: `federation/signing_key.json` (the private seed, mode
`0600`) and `federation/identity.json` (the public, self-signed identity
document). The document's `companion_id` is derived from the public key, so
an id names a key and nothing else. Both files sit outside
`instances/companion/`, so an export never carries them; moving the
companion to another host means copying `federation/` along with
`instances/`, after which peers keep trusting the same key at whatever
address the owner approves next. Deleting `federation/` starts a new
identity that every peer must pair with again.

## Pairing

Trust is established by the two owners, one invite at a time:

1. On the issuing server, mint an invite: `nolune federation invite`, or
   **Invite a companion** under Settings → Connections → Companions. The
   result is one line, `nolune-invite-v1.…`, which packs this server's
   base URL, a one-time secret, and its identity document. It is shown
   exactly once, works once, and expires after ten minutes; the listing
   afterwards shows only the invite's id and clock.
2. Hand the line to the other owner out of band: a message, a note, a
   terminal. It is never a URL, and nothing here ever puts it in one.
3. On the accepting server, redeem it: `nolune federation accept
   <line>` (or pipe the line on stdin: `nolune federation accept < invite.txt`),
   or paste it into **Accept an invite** in the same section. That server
   pins the issuer's document from the line, signs a request with its own
   identity, and posts it to the issuer's `/federation/v1/pair`. The issuer
   verifies the signature, redeems the secret, and answers signed; both
   sides now list the other as pending.
4. On the issuing server, confirm: `nolune federation confirm <companion id>`
   or **Confirm** on the row. The peer becomes paired, the address it
   reported is approved, and it is told with a signed notice. Only the
   issuing owner can confirm; the accepting owner already confirmed by
   redeeming the invite.

`nolune federation peers` (or the section) lists this companion's id and
key, outstanding invites, and every peer with its state, who invited whom,
the approved origins, and when something it signed last verified here
(`last_seen_at`). Peer records live in `federation/peers.json`, which
holds the peer's document, state, origins, and rotation history and no
name, host, or profile. A URL-shaped line is refused on the client and on the
server before anything is sent; a wrong, replayed, or expired secret is
refused by the issuer without saying which.

## Envelope

After pairing, companions exchange signed transport envelopes over
`/federation/v1/*`: each one is addressed to one recipient, carries a
random `nonce` that the recipient accepts once, is bounded by `issued_at`
and `expires_at` with a two-minute skew allowance, commits to its body by
hash, and is signed by the sender's current key (or, for a short grace
window after a rotation, the key it retired). A tampered body, an altered
header, a replay, an expired or future-dated envelope, a downgraded
version, a sender that is unknown, pending, or revoked, and an envelope for
someone else each fail closed with their own typed refusal, and nothing
that failed consumes a nonce. The public routes verify signatures and
nothing else: no owner token, cookie, remote address, `Host` header, or
profile is consulted.

## Revocation and rotation

`nolune federation revoke <companion id>` or **Revoke** on the row
withdraws trust on this side and tells the peer, which does the same.
The record stays as `revoked` so a stale confirmation can never revive it;
only a new invite pairs the companion again. Revoking twice sends nothing.

`nolune federation rotate` (it asks first; `--yes` skips the question and
is required outside a terminal) or **Rotate signing key** replaces this
companion's key. The new self-signed document is endorsed by the old key,
the proof is appended to `federation/rotations.json` before the key files
are replaced, outstanding invites and pending pairings are withdrawn, and
every paired peer is told at its approved origins with a notice signed by
the retiring key. Peers re-key their record and keep the transition in its
`rotation_history`, so the trust chain can be re-verified later; a peer
that could not be reached is reported and keeps trusting the old key until
it hears the proof. A rotated companion has a new id, which the listing
shows beside its previous one.

## No implicit trust on a shared host

Profiles on the same host (`nolune gateway run --profile molinka` beside
the default profile, see [companion-storage.md](companion-storage.md)
"Profiles") share a machine, a user account, a binary, and a loopback
address, and none of that admits either to the other: there is
no implicit trust between them, and no filesystem, database, loopback, or
service-manager shortcut. Pairing two same-host profiles means minting an
invite on one and redeeming it on the other over their own ports, exactly
as above, and confirming it. Until then neither server lists the other, a
request at `/federation/v1/*` without a valid signature is refused
whatever address it comes from or `Host` header it carries, and one
profile's owner token opens nothing on the other's owner routes. The profile name, the port, and the address are deployment
metadata the owner types; the peer's key is the only trust anchor, and
`server/tests/federation_cli.rs` pairs two profiles on one host through
the wire alone to keep it that way.

## Reference

| Surface | What it does |
|---------|--------------|
| `nolune federation invite [--json]` | Mints an invite and prints the one line once; `--json` prints `{id, invite, expires_at, …}` |
| `nolune federation accept [INVITE] [--json]` | Redeems a line given as an argument or on stdin (`-` or nothing); never from a URL |
| `nolune federation peers [--json]` | Lists identity, rotations, outstanding invites (no secrets), and peers with state, origins, last sighting |
| `nolune federation confirm <COMPANION_ID>` | Pairs a peer that redeemed this server's invite and tells it |
| `nolune federation revoke <COMPANION_ID>` | Withdraws trust and tells the peer |
| `nolune federation rotate [--yes] [--json]` | Replaces the signing key and reports which peers were told |
| `--profile <name>` | Any of the above for that profile's server |
| Settings → Connections → Companions | The same actions in the browser: rows with Confirm and Revoke, Invite a companion, Accept an invite, Rotate signing key |
| `POST /api/federation/invites`, `/accept`, `GET /api/federation/peers`, `POST …/peers/{id}/confirm`, `…/revoke`, `/api/federation/rotate` | Owner routes behind the API token or session |
| `POST /federation/v1/pair`, `…/pair/confirm`, `…/pair/revoke`, `…/ping`, `…/rotate` | Peer routes, public, verified by signature only |

The CLI talks to the running server of the selected profile with its API
token, so the server must be up (`nolune gateway`); the invite line is the
only output that ever carries a secret, and it goes to stdout once.
