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

## Intents

What paired companions ask each other is bounded to four structured
intents (#110): deliver a `message`, answer an `availability` query for a
window, set a `reminder`, and put a `proposal` to the owner. Every intent
carries the same header: a `version`, a `correlation_id` the sender chose
(with the sender's companion id it identifies the request, so a redelivery
is recognised), the `sender`, the `represented_owner` the sender speaks
for, the `purpose` in one line, the `disclosure` class it asks for
(`none`, `availability`, `personal`, or `sensitive`, the classes the
policy in [companion-storage.md](companion-storage.md) judges by), and an
`issued_at`/`expires_at` lifetime of at most a week. The typed payload sits
under `intent` with its `type`.

Decoding fails closed and each fault is reported on its own: a version
outside the supported range (checked before anything about the shape),
an unknown intent `type`, an unknown disclosure class, a field no shape
has at any depth, a missing correlation id, sender, represented owner,
purpose, disclosure, or lifetime, an expired or future-dated intent past
a two-minute skew allowance, an inverted or over-long window, and a
label that is empty, over 120 characters, or carries a control
character, a line or paragraph separator, or an invisible format
character (a bidi override, a zero-width character, the byte order
mark), so it can neither show a second line nor read backwards. Free
text inside a payload (a message body, a reminder text, a proposal
description) is peer content: it is bounded, never printed in a log or
an error, and only ever shown inside a block marked as untrusted data
from that companion, never as instructions.

The answer is typed too: `accepted` (with what was disclosed, never
above the class asked for), `denied` (with the policy reason and, for a
rate limit, how long to wait), or `needs_owner` (the owner has to
answer; a peer is told why it waits, never when quiet hours end). Either
side keeps a receipt of the exchange naming who asked whom for what, on
whose behalf and to what end, the class requested and the class granted,
and why: the policy reason and the rule that applied, or the owner's own
approval. A receipt has no field for the payload, and it is only ever
written for a response that answers its intent (the same correlation id,
an answer of the intent's class, a class granted no higher than the one
asked for), so the record can never say more was disclosed than was
requested. The wire shapes are pinned by the fixtures under
`server/tests/fixtures/federation/intents/`; the inbound handling,
delivery, and the companion tools that send intents arrive with the rest
of #110.

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

Telling the peers takes as long as it takes: the server gives every origin
its own transport timeout (15 seconds) and tries them one after another, so
the report for a rotation, a confirmation, or a revocation exists only once
the last peer has answered or run out of time. The CLI waits for that
report rather than guessing at a cap of its own. If the server was reached
but the answer never arrived, the command says so and points at
`nolune federation peers`, which shows what stands, instead of at
`nolune gateway`: the work may have been done, and a rotation must not be
repeated on the strength of a wrong message.

## Policy, approvals, and audit (#109)

A paired peer companion has no implicit access to anything. Every verified
envelope is classified into an intent (`ping`, `message`, `availability`,
`reminder`, `proposal`) and a disclosure class (`none`, `availability`,
`personal`, `sensitive`: what an answer would reveal about this owner) and
judged against the owner's policy before anything is dispatched; a kind or
class the server does not know is denied. Memory and tool access have no
intent class at all: there is nothing to grant. By default only a `ping`
at `none` is allowed (pairing is the consent to be reachable); a message, a
reminder, a proposal, and a query for whether you are free ask the owner,
and the `sensitive` class is denied until the owner writes a rule. A rule
is `allow`, `ask`, or `deny` for one intent at one class, optionally until
a deadline (`expires_at`), and matches exactly. Before the rules, a
revoked or unpaired peer is denied whatever they say and a peer past its
rate limit (60 requests a minute unless the owner sets otherwise) is told
to retry later; after them, inside the owner's quiet hours anything that
would land in front of the owner is deferred, and the peer is told only
that, never when the quiet hours end.

When the answer is `ask`, the request lands in the owner's queue and the
peer is told `approval_required`, the same way on every retry, whether the
owner has not looked yet or has denied it once: nothing about whether the
owner has looked, decided, or when, crosses the wire until the intent is
allowed. Under Settings → Connections → Companions the owner sees "wants
to send you a message" with when it asked and when it lapses (a day),
picks one bounded scope, and allows or denies it: **once** (the next
matching request goes through and uses the approval up; unused, it lapses
after an hour; a denial once holds until the request would have lapsed,
without asking again, and shows only in this owner's receipts), **until**
a deadline (a day, a week), or for that **kind of request** for good (both
as a rule). Under each paired row, what the peer may do is listed one line
per intent and class with the rule it is under, and a select writes or
revokes one rule at a time; the very next request is judged by it.
Revoking a peer, by this owner or by the peer's own notice, drops every
rule and pending request it had, under every pairing and id it has had. A
request belongs to the pairing it was made under: a companion that starts
over with a fresh invite asks afresh, and whatever its earlier pairing
asked or was granted admits nothing.

Every decision leaves a human-readable audit receipt on both sides: the
answering companion records what it was asked and what it decided, the
requesting companion what it asked and what came back, and the owner's own
approvals, denials, rule changes, and revocations are recorded too. A
receipt names the companion ids, the intent and class, the verdict and
reason, and the time; it never contains what the peer sent. Receipts are
kept bounded (the newest thousand, two hundred per pairing for the peer's
traffic and two hundred more for the owner's own decisions about it, so a
peer's retries never push out the record of what the owner decided, thirty
days)
and are listed by `GET /api/federation/receipts`. Peer text, when the
structured intents carry some, is data and never instructions: it can only
reach the model inside a delimited block that names it as untrusted
content from a named companion, and never becomes a tool argument. The
files (`policy.json`, `approvals.json`, `audit.jsonl`, all `0600` beside
`peers.json`), the defaults table, and the exact check order are in
[companion-storage.md](companion-storage.md) "Policy and audit".

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
| Settings → Connections → Companions | The same actions in the browser: rows with Confirm and Revoke, Invite a companion, Accept an invite, Rotate signing key; requests waiting for you with Allow and Deny within one scope; what each paired companion may do, one rule per request kind |
| `POST /api/federation/invites`, `/accept`, `GET /api/federation/peers`, `POST …/peers/{id}/confirm`, `…/revoke`, `/api/federation/rotate` | Owner routes behind the API token or session |
| `GET /api/federation/policy`, `GET /api/federation/approvals`, `POST …/approvals/{id}/approve`, `…/deny` (`{"scope": "once" \| "until" + "expires_at" \| "class"}`), `DELETE …/approvals/{id}`, `POST …/peers/{id}/rules`, `POST …/peers/{id}/rules/revoke` (`{"intent", "disclosure"}`), `GET /api/federation/receipts` | Owner routes for the policy, the queue, and the audit log (#109) |
| `POST /federation/v1/pair`, `…/pair/confirm`, `…/pair/revoke`, `…/ping`, `…/rotate` | Peer routes, public, verified by signature only |

The CLI talks to the running server of the selected profile with its API
token, so the server must be up (`nolune gateway`); the invite line is the
only output that ever carries a secret, and it goes to stdout once.
