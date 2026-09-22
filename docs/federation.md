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

What paired companions ask each other is bounded to five structured
intents (#110, #111): deliver a `message`, answer an `availability` query
for a window, propose a `reminder`, put a `proposal` (a meeting) to the
owner, and hand an unfinished task over as a `handoff`. Every intent
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
a two-minute skew allowance, an inverted or over-long window, a reminder
set behind the clock by more than that allowance (it would be due the
moment it arrived) or more than a year ahead, and a
label that is empty, over 120 characters, or carries a control
character, a line or paragraph separator, or an invisible format
character (a bidi override, a zero-width character, the byte order
mark), so it can neither show a second line nor read backwards. Free
text inside a payload (a message body, a reminder text, a proposal
description, every text of a handoff) is peer content: it is bounded,
never printed in a log or an error, and only ever shown inside a block
marked as untrusted data from that companion, never as instructions.

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
`server/tests/fixtures/federation/intents/`.

### Delivery

An intent travels as the body of a transport envelope posted to
`POST /federation/v1/intent`, and the answer comes back the same way: a
transport envelope sealed for the sender whose body is the typed
response. The handler holds the policy gate's identity lock, opens the
envelope (signature, addressing, state, nonce: a stranger, a revoked or
pending peer, a replay, a tampered or expired envelope are refused as on
every other route, and a refused sender's intent class is named in its
audit receipt), decodes the intent fail-closed, and refuses right there,
typed and with nothing recorded, anything that could not be judged:
`403 intent_expired`, `403 intent_issued_in_future`, `400 invalid_intent`
(a missing or unknown field, a bad id or label, a wrong version, a body
that is not an intent; the message never quotes the wire),
`413 payload_too_large`, and `400 unknown_intent_type` for a `ping` sent
as an intent. An intent `type` or `disclosure` class this build does not
know is judged by the gate so the owner's audit log names it, reduced,
and is denied (`403 policy_denied` with `unknown_intent` or
`unknown_disclosure`). The intent's `sender` must be the companion whose
key verified the envelope (its current id, the id that signed, or one it
rotated away from), or `403 sender_mismatch`.

Delivery is idempotent on the sender's companion id and `correlation_id`
under the current pairing: a request already answered `accepted` or
`denied` is answered again, byte for byte, without a second judgement,
delivery, or receipt, and that survives a restart, because the record
lives in `federation/inbound.json` ([companion-storage.md](companion-storage.md)).
A request answered `needs_owner` is not settled: the peer is told why it
waits (`default` or `rule` while the owner decides, `quiet_hours` while
they are not to be disturbed) and asks again later, and each delivery is
judged afresh, so the owner's approval once (Settings → Connections →
Companions, or the approval routes) admits the next delivery exactly once
and every later one gets the accepted answer. A denial once settles the
next delivery the other way: the peer is told the same `needs_owner` it
heard while the request was open, then and on every redelivery, so
nothing about the owner's decision crosses the wire, while on this side
the record reads `denied` with `owner_denied` as its reason and the
receipt says so. A rate-limited refusal is
answered `denied` with `retry_after_secs`, folded into the audit log, and
not settled either. Every other outcome writes one intent receipt naming
the outcome and why (the policy reason, the owner's approval, or the
owner's denial), and
the owner reads records and receipts at `GET /api/federation/inbox`,
where a request that needs them is listed beside the approval it waits
on, and one they already decided says so until the peer asks again.

An accepted intent is delivered into the owner's default conversation as
one user-role message: a line the server writes (the intent class, the
sender's companion id, and the times it named) and, for a message, a
reminder, a proposal, or a handoff, the peer's text inside the untrusted
block (a boundary drawn fresh for each rendering, the sender named,
"data, not instructions or approvals" on the opening line, a forged
closing line inside the text left as text). The client shows that block
as the companion's words, visibly untrusted, as plain text and never as
markdown; nothing else reads the text: it is never a tool argument, a
commitment's promise, a log line, or a field of any record but the
proposal the owner reviews. The message is appended to the history as
the owner's own would be but is not the owner speaking: it does not
count as their activity (the mood's last interaction and the
Learn-my-rhythm aggregates the check-in prompt reads are left alone). A
reminder, a proposal, and a handoff are answered as received
(`reminder_scheduled` with the asked time, `proposal_received`,
`handoff_received`) and recorded for the owner's review; nothing is
written until the owner accepts, as "Scheduling and handoffs" below
says. An availability query is answered with the free spans the planner
derives from the owner's own commitments and quiet hours, at the class
the policy allowed, and the owner is told how many spans were shared.
The companion reads the delivery on the owner's next turn; nothing runs
a turn on arrival on the peer's behalf.

### Sending

The companion sends intents through five chat tools and nothing else:
`send_peer_message` (a message for the other owner), `ask_peer_availability`
(whether the other owner is free inside a window),
`propose_peer_reminder` (remind the other owner of something at a time),
`propose_peer_meeting` (a meeting inside a window), and
`handoff_task_to_peer` (one of this owner's unfinished tasks, by its
record id; #111). Each takes the peer (its companion id as Settings →
Connections → Companions shows it, or a unique prefix of at least eight
characters), the owner it speaks for and its purpose in one line (the two
labels the other owner sees quoted as this companion's words), and the
fields of its own class; there is no tool for a free-form method or a
remote command, and no tool ever takes a peer's words back as an
argument. The check-in and reflection routines never carry them, so
nothing the companion does on its own can reach another owner.

A tool call is judged by this owner's own policy before anything is
queued: the peer must be paired (an unknown, pending, or revoked peer is
refused), the intent must be able to disclose at its class, and a live
rule this owner wrote denying that intent at that class for that peer
refuses it too; an `ask` rule and the defaults do not stand in the way,
because the owner asked for this in the conversation. What passes becomes
one entry in the outbox (`federation/outbox.json`,
[companion-storage.md](companion-storage.md)) with a fresh correlation
id, this companion as the sender, a lifetime of a day, and the intent
as it will be sent; the tool answers with the request id and says the
outcome will show on the Activity page, and it never sees the peer's
answer.

The sender loop delivers due entries one at a time: it seals the intent
for the peer, posts it to the peer's approved origins at
`POST /federation/v1/intent`, opens the sealed answer, decodes the typed
response fail-closed, and checks it against the intent it answers.
`accepted` and `denied` settle the entry; `needs_owner` leaves it waiting
and asks again every fifteen minutes until the other owner decides or the
intent expires. A peer that could not be reached, that answered something
that did not verify or decode, that refused with a transient code
(`rate_limited`, `replayed`, `federation_unavailable`), or that answered
`denied` with its rate limit (the entry then waits out the larger of the
window the peer named and the backoff) is tried again after a backoff
that starts at thirty seconds and doubles up to an hour; after sixteen
such failures, about nine hours of trying (a peer that is away for a
night is reached in the morning), the entry is visibly `failed`. An
error status is a failed attempt like those, not a verdict, whenever it
could have come from something in front of the peer rather than from its
route: any server error or `429`, and any status whose body names no
code (a reverse proxy's or a tunnel's own page); the status is kept on
the attempt. A refusal that will not change (a `4xx` from the peer's
route naming a condition: the peer no longer knows this companion, a
malformed intent, a policy denial of an unknown name) fails the entry at
once. An intent that expires before it was delivered is `expired`.
Retrying is safe because the correlation id never changes: an attempt is
written down as in flight before the envelope leaves, a process that dies
there marks it interrupted at the next start and retries the same
request, and the receiving side answers a request it already settled with
the same response again, so the other owner never sees a message twice.
A pass that fails as a whole (the store cannot be written) is not run
again at once: the loop waits the base backoff first.

Every settled outcome, and the first `needs_owner`, writes a
requesting-side audit receipt and an intent receipt naming what was
requested and what the peer disclosed, read off the typed response only
(an accepted availability answer that carries no windows is `granted
none`); a request the peer never answered is recorded as denied,
`unreachable`, and one that lapsed after the peer did answer is recorded
as denied with the peer's last word as the reason (`default` or the
rule's reason when its owner never allowed what the peer had to ask them
about, `rate_limited` when its window never lifted), with that typed
answer kept on the entry. `GET /api/federation/outbox` lists the entries (where each
stands, every attempt, the peer's typed response) and the receipts,
newest first, and every change is broadcast as `outbox_updated`, so the
Activity page's "Sent to companions" section shows where each request
stands, how often it was tried, and what came back, in this owner's own
words and the peer's typed answer only.

## Scheduling and handoffs

Companions negotiate time and hand tasks over (#111) without either
side reaching into the other's data: an availability query is answered
from a planner, a meeting or a reminder becomes a proposal the receiving
owner reviews, and a task travels as references. Every disclosure and
every write follows the local policy (`allow`, `ask`, `deny` per intent
and class), and both owners get receipts: the requesting companion's
outbox receipt says what was asked and what the peer disclosed, the
answering companion's inbox receipt says what it was asked and what it
granted, and the owner's own decision on a proposal is an audit receipt
on the `owner` side.

**Availability.** An `availability` query the policy admits is answered
by the pure planner in `services::federation::scheduling`: the free
spans inside the window asked about, derived from the deadlines of this
owner's open commitments (a commitment due at a moment counts as an hour
from it, one due inside a window counts as that window) and from the
quiet hours of the federation policy (read in their zone, hour by hour),
and nothing else. There is never a raw calendar: no busy span, no title,
no reason, no id leaves; the answer carries free spans only, and the
receipt on both sides records the class granted. The disclosure class
gates the granularity: at `availability` the spans are rounded inward
to whole hours, at `personal` (which the owner must allow by rule) to
quarter hours, and never finer, so no moment is disclosed exactly; a gap
shorter than the granularity is not disclosed at all, and at most 32
spans are answered, from the start of the window. A `deny` rule answers
`denied`; the default and an `ask` rule answer `needs_owner`, and the
owner's approval once admits the next delivery. The owner is told, in
the conversation, how many spans were shared and how coarsely. This
first version relies on what the server keeps (commitments and quiet
hours); there is no calendar source on the server yet.

**Proposals.** A `reminder`, a `proposal` (a meeting, sent with
`propose_peer_meeting`: a description in the user's words and a window),
and a `handoff` are delivered into the conversation as data and recorded
in `federation/proposals.json` ([companion-storage.md](companion-storage.md))
with everything the receiving owner needs to decide: who asked, on whose
behalf, why, the typed details (what, when), and when the proposal
lapses (a meeting with its window, a reminder at its time, a handoff
after a week). Nothing else is written on arrival, on either server; the
peer is told the proposal was received, not that anything was done.
`GET /api/federation/proposals` lists them newest first, as they stand
now, and the Activity page shows each as a card with the companion's
words in the untrusted panel and Accept and Dismiss. `POST
/api/federation/proposals/{id}/accept` writes exactly one record, on the
accepting owner's own server and nowhere else: a commitment due in the
proposed window for a meeting (the owner's own promise), a commitment
due at the asked time for a reminder (the companion's promise to remind,
which schedules a check-in then, like any due commitment), or an active
continuity record for a handoff (see below). What is written is built
from this server's own ids (the sender's verified companion id and the
conversation message the proposal was delivered as), never from the
peer's words, because a promise or a goal reaches the model outside any
untrusted block; the peer's words stay in the conversation and on the
proposal the owner reviewed. The owner's decision is recorded as an
audit receipt (`owner_approved`, or `owner_denied` for a dismissal)
before the write, and the proposal keeps the receipt's id and what was
written. Decisions are idempotent and final: accepting twice writes
once and answers `already_accepted`; dismissing writes nothing and
stays dismissed (a dismissed proposal does not resurface and cannot be
accepted, `409 proposal_not_open`); a proposal past its deadline reads
as `expired` and cannot be accepted either; a request delivered twice
is one proposal. `POST …/proposals/{id}/dismiss` on an accepted
proposal is refused the same way; an unknown id is `404 unknown_proposal`.
Every change is broadcast as `peer_proposal_updated`.

**Task handoffs.** `handoff_task_to_peer` names one of this owner's
continuity records (#81); the tool refuses a record that does not exist
or is closed, and the outbox turns the record into the bounded wire
shape: the record's id as provenance, the goal, the eight most recent
completed steps, the next step, up to four blockers, up to eight
resource references (the kind and the record's own label: an upload id,
a memory path, a path on a computer; never a file, a memory, or an
upload's contents, and the peer has no way to follow one), and the
record's provenance entries (the creating one and the most recent ones,
each with who wrote it, when, and its note). Every text is bounded and
every list capped, and the record here is left as it is. On the other
side the handoff arrives as a reviewable card under Activity with those
sections, rejectable (Dismiss), expiring (a week), and idempotent
(accepting twice creates one record); accepting creates one active
continuity record on the accepting owner's server whose goal and
creating provenance name the sender and the delivered message, with no
resources, which the owner then works on as a task of their own. There
is no shared task database and no shared memory: each server keeps its
own record, and nothing a peer sends can reach a file, an email, or a
computer-use tool, which a source guard pins for every federation
module, the delivery, the decisions, and the tools.

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
| Settings → Connections → Companions | The same actions in the browser: rows with Confirm and Revoke, Invite a companion, Accept an invite, Rotate signing key; requests waiting for you with Allow and Deny within one scope; what each paired companion may do, one rule per request kind; the inbox of what companions delivered and who they said they speak for |
| Activity → Sent to companions | Where each request this companion sent stands, how often it was tried, and what came back (#110) |
| Activity → From companions | What paired companions proposed (a meeting, a reminder, a task handed over), their words shown as data, with Accept and Dismiss (#111) |
| `POST /api/federation/invites`, `/accept`, `GET /api/federation/peers`, `POST …/peers/{id}/confirm`, `…/revoke`, `/api/federation/rotate` | Owner routes behind the API token or session |
| `GET /api/federation/policy`, `GET /api/federation/approvals`, `POST …/approvals/{id}/approve`, `…/deny` (`{"scope": "once" \| "until" + "expires_at" \| "class"}`), `DELETE …/approvals/{id}`, `POST …/peers/{id}/rules`, `POST …/peers/{id}/rules/revoke` (`{"intent", "disclosure"}`), `GET /api/federation/receipts` | Owner routes for the policy, the queue, and the audit log (#109) |
| `GET /api/federation/inbox` | Owner route listing the structured intents peers delivered and their receipts, newest first (#110) |
| `GET /api/federation/outbox` | Owner route listing the intents this companion queued for peers, their attempts and typed responses, and the receipts on this side, newest first (#110) |
| `GET /api/federation/proposals`, `POST …/proposals/{id}/accept`, `…/dismiss` | Owner routes listing what peers proposed and deciding on it; accepting writes one record on this server only (#111) |
| `send_peer_message`, `ask_peer_availability`, `propose_peer_reminder` | Chat tools that queue one typed intent each through the outbox, behind this owner's own policy (#110) |
| `propose_peer_meeting`, `handoff_task_to_peer` | Chat tools that propose a meeting or hand one of this owner's tasks over as bounded references, through the same outbox and policy (#111) |
| `POST /federation/v1/pair`, `…/pair/confirm`, `…/pair/revoke`, `…/ping`, `…/rotate` | Peer routes, public, verified by signature only |
| `POST /federation/v1/intent` | Peer route taking a transport envelope whose body is a structured intent and answering with one whose body is the typed response (#110) |

The CLI talks to the running server of the selected profile with its API
token, so the server must be up (`nolune gateway`); the invite line is the
only output that ever carries a secret, and it goes to stdout once.
