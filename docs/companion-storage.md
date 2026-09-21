# Companion storage format

Nolune owns exactly one persistent companion identity and memory per server
(decision #100, implemented in #103). Connected computers, chats, and
relationship scopes are contexts of that identity, never separate companions.
This document is the reference for the on-disk layout and wire shape that
import/restore work (#74) and continuity records (#81) build on.

## Format version 1

### Canonical identity

| Item | Value |
| --- | --- |
| Canonical slug | `companion` |
| Companion directory | `~/.nolune/instances/companion/` |
| Identity marker | `instances/companion/companion.json` |
| Format version | `1` |

The identity marker is the source of truth for "this companion exists":

```json
{
  "format_version": 1,
  "slug": "companion"
}
```

Unknown fields are rejected. A marker with any other `format_version` or
`slug` is unsupported: the server refuses to read or write that companion
(HTTP `503 companion_format_unsupported`) and never rewrites the marker.

### Directory layout

```text
~/.nolune/
├── config.toml                  server configuration (global)
├── federation/                  companion signing identity (#108), see below
│   ├── identity.json            public, self-signed identity document
│   ├── signing_key.json         private Ed25519 seed, mode 0600
│   ├── peers.json               paired companions and their key rotations
│   ├── rotations.json           this companion's own key rotations
│   ├── policy.json              what each paired peer may ask for (#109)
│   ├── approvals.json           requests that asked the owner, until decided or lapsed
│   └── audit.jsonl              receipts for every judged intent, both sides
├── skills/                      installed skills (global)
├── vectors/                     derived vector index, keyed by slug
├── imports/                     restore staging (see Restore below); empty between imports unless a crash left a tree behind
└── instances/
    └── companion/               the one companion
        ├── companion.json       identity marker (see above)
        ├── soul.md              personality definition
        ├── project_state.json   name, timezone, and other settings
        ├── instance.toml        per-companion configuration
        ├── memory/              long-term memory library (source of truth)
        ├── memory_corrections.json  the user's corrections and open conflicts (see below)
        ├── chats/{chat_id}/     conversation history, agent markers, receipts/
        ├── scheduled/*.json     scheduled tasks
        ├── activity/*.json      proactive run records (docs/proactive-loop.md)
        ├── commitments/*.json   promises the companion follows through on (docs/proactive-loop.md)
        ├── continuity/*.json    resumable task records (see below)
        ├── machines.json        every computer that ever connected (see below)
        ├── proactive_policy.json quiet hours, budget, routine intervals
        ├── resume_ritual.json   Resume my work policy and its one suggestion (docs/proactive-loop.md)
        ├── heartbeat.md         optional guidance for check-ins
        ├── uploads/             user-uploaded files
        ├── drops/               proactive creative artifacts
        └── skills/              companion-scoped skills
```

Every persisted subsystem (settings, soul, history, memory, scheduler, machine
bindings, export) lives under this single directory. Retired layouts
(`stats/` per-day aggregates, `agents/` child-agent configs and histories,
`agent_runs/` traces, `thoughts/` raw monologue) are removed from the
companion directory at startup and never read. The derived vector index
under `vectors/` is keyed by the same slug and can always be rebuilt from
`memory/`.

### Continuity records

An unfinished task the user asked for survives chat, model, server, and
device restarts as one JSON file under `instances/companion/continuity/{id}.json`
(#81). The file is a link-and-provenance record, never a copy of the files
or memories it points at.

| Field | Meaning |
| --- | --- |
| `version` | `1`; a file with any other version is reported and left alone |
| `id` | `task_<unix seconds>_<8 hex>`, one path component |
| `goal` | the user's goal in their words, at most 500 characters |
| `state` | `active`, `waiting`, `ready_to_resume`, `completed`, `dismissed`, or `failed` |
| `origin` | `chat_id` and, when known, the `message_id` the task came from |
| `machine_ids` | connected computers the task needs |
| `resources` | links only: `upload` (`id`), `memory` (`path`), or `machine_path` (`machine_id`, `path`), each with the provenance that added it |
| `completed_steps` | what already happened, each with provenance |
| `blockers` | `machine_unavailable`, `resource_missing` (added and cleared by the server's reference check), or `other` (stated by the user or the tool), each with a detail and provenance |
| `next_step` | the suggested next step |
| `priority` | the user's stated priority, `low`, `normal`, or `high`; absent means normal (#83) |
| `due_at` | when the user wants it done, unix seconds; absent means no deadline (#83) |
| `handoff` | the user's handoff decision, if any (see [Handoff cards](#handoff-cards)) |
| `created_at`, `updated_at` | unix seconds |
| `provenance` | every write: `source` (`user`, `chat`, `tool`, `server`), `at`, and a note |

Bounds are enforced on every write: 50 steps, 20 blockers, 40 resources,
16 computers, 100 provenance entries (the creating entry is always kept),
300 characters per step, blocker, note, or next step, and 1 MiB per file.
No record built within those caps can reach the file cap, so a record that
has used every cap can still be completed or dismissed. A file that is
larger, is not JSON, or breaks an invariant is skipped and surfaced as an
error by the listing API; it is never deleted or rewritten. Every write is
a read-modify-write of one file under one lock for the directory, shared by
the API and the chat tool, and lands through a uniquely named temp file
and a rename, so a read never overwrites a write that landed in between.

Records are written only by explicit task activity: the
`task_continuity_update` chat tool, which the companion calls while doing
work the user asked for (a stated `priority` and `due` moment included,
the latter read like a commitment's: RFC 3339, or a local date and time in
the companion's timezone), and the API below. Nothing is ever inferred from
screenshots, connected-computer events, check-ins, reflections, or
schedules, and the tool is not part of any routine's tool set. Only
`active`, `waiting`, and `ready_to_resume` records are resumable;
`completed` and `dismissed` records stay inspectable but never reappear as
work to pick up. Reads run a reference check: a computer that is not
connected or an upload or memory path that cannot be found becomes an
explicit blocker with `server` provenance, and the blocker clears when the
reference is back.

| Route | Purpose |
| --- | --- |
| `GET /api/instances/companion/continuity?resumable=` | `{records, errors}`, most recently updated first |
| `GET /api/instances/companion/continuity/{id}` | one record after the reference check |
| `PUT /api/instances/companion/continuity/{id}` | apply `goal`, `state`, `completed_step`, `blocker`, `clear_blockers`, `next_step`, `priority`, `due_at`, `clear_due`, `machine_ids`, `resources` with a required `note` |
| `POST /api/instances/companion/continuity/{id}/complete` | mark done (optional `note`) |
| `POST /api/instances/companion/continuity/{id}/dismiss` | dismiss (optional `note`) |

### Handoff cards

A resumable continuity record is offered to the user as a handoff card
(#82): a review of the unfinished task and a deliberate choice of where to
continue it. The card is derived on every read from the record and the
[known machines](#known-machines) list, never from model text, so it stays
useful while the computer the task started on is offline: the origin is
named with its last known state, the finished steps, linked resources,
blockers, and proposed next step are the record's own, and nothing is
copied between computers.

| Card field | Meaning |
| --- | --- |
| `record_id`, `goal`, `state`, `origin_chat_id` | the record and the conversation the task came from, where a continuation runs |
| `origin` | the first computer the record names, as a computer summary: `machine_id`, `display_name`, `known`, `online`, `health`, `platform`, `last_seen` (`null` fields when the machine list has never heard of it) |
| `completed_steps`, `blockers`, `next_step` | the record's, as plain text |
| `resources` | each link with a `label` and `available` (`false` while the reference check reports it missing) |
| `required` | `capabilities` the destination's desktop must offer (screen and input actions, plus file actions when the record links a file on a computer) and the desktop `permissions` computer use needs (`screen_capture`, `accessibility`) |
| `decision`, `bound_to` | the record's `handoff` decision and, after acceptance, the computer it is bound to |
| `offered` | whether the card is shown: the record is resumable and was not kept or dismissed since the last explicit update |

The record's `handoff` field is written only by the routes below:

| `kind` | Fields | Meaning |
| --- | --- | --- |
| `accepted` | `machine_id`, `run_id`, `at`, optional `outcome` | continue on the computer with that stable id; `run_id` is the activity run in the same trail |
| `kept` | optional `machine_id`, `at` | leave the task on the origin computer |
| `dismissed` | `at` | stop offering the card |

`kept` and `dismissed` hide the card until new explicit work (the chat tool
or a `PUT`) updates the record, which clears the decision; the server's
reference check is not explicit work and never brings a card back. An
acceptance survives the progress the continuation records. Its `outcome`
(`status` `completed`, `failed`, or `cancelled`, `finished_at`, and a
`summary` no longer than a note) is recorded exactly once, for the bound
run only, with `server` provenance, so a run can never be written onto a
later acceptance.

**Continue here** and **Continue on…** are the same acceptance with the
stable id of the chosen computer; the client only differs in how it picked
it (the remembered or only connected desktop, or a picker over
`GET /machines`). The preview and the acceptance run the same checks
against that computer at that moment, each with a `kind`, a `severity`,
and a sentence the user can read:

| Severity | Checks |
| --- | --- |
| `blocking` (continuation refused) | `record_closed`, `model_unavailable`, `initiative_off`, `machine_unknown`, `machine_offline`, `machine_not_responding` (stale heartbeat), `capability_missing`, `permission_denied`, `resource_missing` (an upload or memory note the reference check cannot find, or a file the destination was asked for at acceptance and does not have), `resource_elsewhere` (a file on a computer that is not connected) |
| `approval` (the desktop will ask before the first action) | `permission_prompt`, `permissions_unknown` |
| `note` | `resource_elsewhere` while that computer is connected; `resource_unverified` for a file on the destination itself, which is looked for when the user confirms |

Missing files, unavailable apps, and insufficient permissions are therefore
reported before continuation and never silently skipped. No computer-use
action starts before the user accepts: the card and the preview are reads,
and the desktops' toolcall channels stay untouched by a refusal. The one
toolcall acceptance itself sends is a read-only `file_list` of the folder
of each file the record places on the destination (`~` and a root list
themselves), answered within 30 seconds; a file that is not listed, a
folder the desktop cannot read, or a desktop that does not answer refuses
the acceptance as `resource_missing` with the desktop's own reason, before
anything is bound or admitted. Files on other computers are never probed
through the destination.

Accepting binds the record to the chosen machine id (added to
`machine_ids`, the task active again), admits one run through the
[proactive loop](proactive-loop.md) under the `handoff` trigger with
`Target::Machine` (dedupe key `handoff:{id}`), and hands the task to its
conversation as an explicit `[handoff]` request naming the one computer to
act on and the record to report progress to. The conversation's own tools
do the work under the user's normal approvals. Acceptance is idempotent:
accepting again while that run is going, whichever computer is named,
answers the same run with `already_running: true` and starts nothing.
Acceptances of one record are serialized (a lock per record, held from the
checks through the binding), so two that arrive together, from a double
click or two clients, start one run and put one request in the
conversation; the loop's dedupe key stays as a backstop. Once the run has
finished, accepting again is new explicit work and a new run. When the
conversation stops, the run is closed from what the agent loop itself
reports (its exit is on record under the conversation's key before the key
is released; chat text is never read for this, since mood lines, rhythm
updates, and desktop connections write `[system]` lines into the same
conversation): `failed` with the loop's own error label and retryable,
`cancelled` when the conversation was stopped, otherwise `completed` with
the receipts from the trace after the request, or `failed` and retryable
when no turn took the request up. The request is found by its message id;
when a server-side compaction rewrote the history since, a summary written
after the request means everything that survived came after it, so a long
continuation keeps its receipts. The outcome lands on the record, so the
activity trail and the record tell one story. Cancelling the run from the
activity view stops the conversation; retrying it from there re-runs the
checks on the bound computer and links the attempt (`retry_of`). A restart
fails the interrupted run like any other, and at startup every acceptance
whose run died with the previous process is closed on its record with that
run's outcome (broadcast as `handoff_updated`); the record stays bound and
offered, so the card offers the task again instead of waiting on a run
that no longer exists. The client reloads its cards on every reconnect for
the same reason.

| Route | Purpose |
| --- | --- |
| `GET /api/instances/companion/handoffs` | `{handoffs, errors}`: the cards offered right now, most recently updated first, after the reference check |
| `GET /api/instances/companion/continuity/{id}/handoff` | one record's card, whether or not it is offered |
| `GET /api/instances/companion/continuity/{id}/handoff/preview?machine_id=` | `{card, destination, checks, ready}` for continuing there; nothing started |
| `POST /api/instances/companion/continuity/{id}/handoff/accept` | `{machine_id}` → `{card, run, already_running}`, or `409 handoff_not_ready` with `message` and `checks` |
| `POST /api/instances/companion/continuity/{id}/handoff/keep` | **Keep there**: bound to the origin, hidden until explicit work |
| `POST /api/instances/companion/continuity/{id}/handoff/dismiss` | **Dismiss**: hidden until explicit work; the record stays resumable |

Errors are `404 not_found`, `400 invalid`, `409 handoff_not_ready`, or
`500 handoff`, each with a `message`; a foreign slug fails closed with
`unknown_companion` like every other companion route. Every decision and
every finished continuation is broadcast as `handoff_updated`
(`instance_slug`, `card`), so the client replaces the card in place. The
client renders the card in the Activity view with **Continue here**,
**Continue on…**, **Keep there**, and **Dismiss**, and shows the preview
(the destination computer and the checks, grouped into stops, approvals,
and notes) before the user confirms a continuation.

### Known machines

Every computer that ever attached through the Nolune desktop app is one
record in `instances/companion/machines.json` (#80), so a disconnected
computer stays listed as offline with the time it was last seen instead of
vanishing, and a reconnect updates the same record instead of adding one.
The desktop registers under a stable id (a UUID it persists in its settings
store on first use) and sends its hostname for display; the user can give
the computer a name, which is stored on the server so every client shows it.

```json
{
  "version": 1,
  "slug": "companion",
  "machines": [
    {
      "machine_id": "4f3c1c2e-9b5e-4d2b-8f0a-1c2d3e4f5a6b",
      "display_name": "Studio Mac",
      "hostname": "studio.local",
      "os": "macos",
      "platform": "macos",
      "location": "desktop",
      "screen_width": 2560,
      "screen_height": 1440,
      "permissions": { "accessibility": "granted", "screen_capture": "denied" },
      "capabilities": ["screenshot", "left_click", "bash", "file_read"],
      "first_seen": 1789862400,
      "last_seen": 1789866000
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `version` | `1`; the file is refused with any other value |
| `slug` | always `companion`; a file naming another companion is refused |
| `machine_id` | the desktop's stable id: 1 to 128 bytes of letters, digits, `-`, `_`, `.`, `:` |
| `display_name` | the user's name, at most 64 characters on one line; `null` shows the hostname |
| `hostname`, `os`, `screen_width`, `screen_height` | as reported at the last registration; the two labels are cut at 256 characters |
| `platform` | `macos`, `windows`, `linux`, or `null` when `os` names none of them |
| `location` | `desktop` for every desktop registration; the server home is a Cua target (#16), never a desktop record |
| `permissions` | accessibility and screen capture as `granted` or `denied` (the protocol's names); `null` when the desktop did not report them |
| `capabilities` | action names the desktop executes; a desktop that reports none is recorded with the legacy set |
| `first_seen`, `last_seen` | unix seconds of the first registration and of the last heartbeat or disconnect |

Bounds: 64 records (a new computer evicts the longest-offline one), 64
capability names per record, 256 characters per hostname and OS label, 1 MiB
per file; the store refuses to write more than it reads, so it never leaves
a file behind that it would reject. The file is read on every access and
every change lands through a temp file and a rename. A file that is larger,
is not JSON, carries unknown fields, another version or slug, an invalid id,
or the same id twice fails closed: the listing route answers
`503 machines_format_unsupported`, renames are refused, and the file is
never rewritten, while connected computers keep working in memory. Fixing
or removing the file takes effect on the next access, without a restart,
and every computer that connected in the meantime is recorded then. A
connected computer's heartbeat reaches its record at least once a minute,
so a crash leaves `last_seen` at most a minute behind.

A desktop upgraded from a release that registered under its hostname takes
over that offline record the first time it registers under its stable id,
keeping `first_seen` and the user's name; the hostname-keyed row is dropped
instead of staying behind as a duplicate. An offline computer can also be
forgotten explicitly (`DELETE`); a connected one cannot.

The API reports each record with live state that is never written to disk:
`display_name` resolved to the hostname when unnamed (`custom_name` holds
the user's name or `null`), `online`, `health` derived from heartbeat age
(`healthy`; `degraded` when the socket is open but no heartbeat arrived for
45 seconds; `unavailable` when offline), and `driver_version` and
`cua_health`, filled in while a connected desktop has registered its Cua
driver (#17) and `null` otherwise. The server machine itself, when it has a
Cua driver (#16), is listed beside the records as one live `server_local`
row with both filled in; it is never written to this file (see
[computer-use.md](computer-use.md)).

| Route | Purpose |
| --- | --- |
| `GET /api/instances/companion/machines` | `{machines}`, online first, then most recently seen |
| `PUT /api/instances/companion/machines/{machine_id}` | `{display_name}`; blank or `null` shows the hostname again |
| `DELETE /api/instances/companion/machines/{machine_id}` | forget an offline computer (`204`); `409 machine_online` while it is connected, `404 not_found` otherwise |
| `POST /api/instances/companion/machine-hello` | optional `{machine_id}`; runs the connection check-in for the named or only connected computer, `409 ambiguous_machine` (with `machine_ids`) when several are connected and none is named, `409 machine_offline` or `404 not_found` for a named one that is not connected or not known |
| `POST /api/instances/companion/machine-bye` | optional `{machine_id}`; records which connected computers stay attached, the named one or all of them |

Every registration, disconnect, rename, and change of health is broadcast
to clients as one `machine_updated` server event carrying the same shape as
the listing: a heartbeat that goes stale is reported as `degraded` by a
watch that runs every 15 seconds, and the heartbeat that ends the stale
stretch as `healthy`; routine heartbeats are silent. A record that is
forgotten, taken over by a stable id, or evicted is broadcast as
`machine_forgotten` (`{instance_slug, machine_id}`), so clients drop the row
without refetching.

### Obsolete sibling directories

Any other directory under `instances/` is left over from the unpublished
multi-instance layout. The server:

- logs a warning listing them at startup,
- never lists, migrates, schedules, indexes, resumes, exports, or deletes them,
- rejects every request that addresses them (see below).

There are no external users of that layout, so no migration is provided.
Remove or archive those directories manually.

### Derived index recovery

`vectors/` is a derived cache keyed by the companion slug; `memory/` is the
source of truth. Recovery is automatic (#96): on startup the server asks the
index whether it `needs_backfill` (missing, corrupt, or written by another
format version) and rebuilds it from the memory files in the background,
committing only a complete candidate so the last good index stays searchable
if a provider call fails. Deleting a memory reconciles its index entries
immediately. There is no button and no manual reindex route; delete the
`vectors/` directory to force a rebuild on the next start.

### Memory recall receipts

Every assistant message keeps a bounded record of the memories that were
auto-recalled into its prompt (#84), so the client can answer "why did Nolune
remember this?" without exposing model reasoning:

```text
chats/{chat_id}/receipts/{message_id}.json
```

Each file is written atomically (temp file + rename) right after the turn and
is a derived record: `memory/` stays the source of truth. The shape:

```json
{
  "message_id": "msg_1758360000000_3",
  "chat_id": "default",
  "memories": [
    {
      "path": "about/basics.md",
      "source": "about/basics.md",
      "excerpt": "likes tea, lives in Lisbon",
      "reason": "semantic",
      "confidence": "high",
      "retrieved_at": "2026-09-20T09:00:00Z",
      "source_status": "present"
    },
    {
      "path": "photos/sky.png",
      "source": "photos/sky.png.md",
      "excerpt": "sky over Lisbon at dusk",
      "reason": "linked_to",
      "linked_from": "about/basics.md",
      "confidence": "low",
      "retrieved_at": "2026-09-20T09:00:00Z",
      "source_status": "missing"
    }
  ]
}
```

- `path` is the memory as the library shows it; `source` is the canonical
  file whose text was recalled. Media memories cite their bound text
  representation (`<media>.md`, see `media_text.rs`).
- `excerpt` is a bounded slice of the memory body; the stamped
  `created`/`updated` frontmatter is never part of it.
- `reason` is `semantic` (vector index), `keyword` (BM25), `linked_to` (one
  graph hop from `linked_from`), `matched` when hybrid search cannot say
  which channel found a media memory, or `pinned` for a memory the user
  pinned (recalled on every turn, see below).
- `confidence` is a bucket (`high`, `medium`, `low`); raw scores never leave
  the server.
- `source_status` is resolved again on every read: a memory that was deleted
  or moved after the turn is reported as `missing` and the receipt still
  loads. An empty `memories` list means no memory influenced that reply.

Routes (companion-scoped like every other `/api/instances/{slug}/…` path):
`GET /api/instances/{slug}/{chat_id}/receipts` lists a chat's receipts and
`GET /api/instances/{slug}/{chat_id}/receipts/{message_id}` returns one
(`404` only when no receipt exists). The transient `memory_recall` server
event carries the same `memories` entries at retrieval time.

### Memory flags and corrections

The user can correct, pin, or exclude a memory without editing internal state
(#84). Every control rewrites the canonical file under `memory/` and
reconciles the derived index right away (the path's vectors are replaced in
one step, or removed and marked for backfill when the embedding provider is
down; BM25 re-reads the file), so the next recall sees the corrected text
and never the old one, and everything survives a restart because nothing
lives only in memory. A text memory is read, rewritten, and re-indexed under
the same per-companion lifecycle gate the companion's own `memory_write` and
`memory_forget` hold, so a correction or flag change never interleaves with
the companion's read-modify-write of that file and never brings back a
memory a forget removed in the meantime (the correction answers `404`).

Two flags sit in a text memory's frontmatter next to the timestamps. A line
appears only when the flag is set, so an unflagged memory is byte-identical
to the layout before #84, and both flags are carried over whenever the
companion rewrites or appends to the file (`stamp_content`):

```text
---
created: 2026-09-20
updated: 2026-09-20
pinned: true
exclude_from_proactive: true
---
likes tea, lives in Lisbon
```

- `pinned: true` — the memory is auto-recalled into the user's chat on every
  turn, with `reason: "pinned"` on the receipt, whether or not the
  conversation matches it (up to eight, by path).
- `exclude_from_proactive: true` — the memory never reaches the companion's
  own routines: the check-in and reflection catalog omits it, and their
  `memory_list`, `memory_search`, `memory_read`, and `memory_write` tools
  never list, return, read, or rewrite it. The user's chat, auto-recall, and
  the library (`GET …/memory`, `GET …/memory/search`) still see it, and the
  listing reports both flags per entry.
- Flags apply to text memories; a media memory's flag request answers `422`.
  Correcting a media memory rewrites its bound text (`<media>.md`).

Corrections are recorded in `instances/companion/memory_corrections.json`, a
small versioned ledger next to the memory store, written atomically:

```json
{
  "version": 1,
  "entries": [
    {
      "id": "corr_1758360000_1a2b3c4d",
      "path": "about/basics.md",
      "statement": "likes oolong, lives in Porto",
      "previous": "likes tea, lives in Lisbon",
      "status": "applied",
      "corrected_at": "2026-09-20T09:00:00Z"
    },
    {
      "id": "corr_1758363600_5e6f7a8b",
      "path": "about/basics.md",
      "statement": "likes matcha, lives in Porto",
      "previous": "likes oolong, lives in Porto",
      "status": "needs_resolution",
      "corrected_at": "2026-09-20T10:00:00Z",
      "conflicts_with": "corr_1758360000_1a2b3c4d"
    }
  ]
}
```

`statement` is the user's text in full (a resolution re-applies it; at most
64 KiB), `previous` a bounded excerpt of what the memory said before. A
status is `applied` (in force: the memory reads as this statement),
`superseded` (a later correction, a resolution, or the companion's own
rewrite replaced it), `needs_resolution` (waiting for the user), or
`withdrawn` (the user kept the earlier statement). A file with another
`version` or malformed JSON is reported and never rewritten.

The ledger is bounded when it is written, never when it is read: at most
500 entries and 4 MiB on disk. Past either bound the oldest settled entries
(`superseded`, `withdrawn`) go first, then the oldest `applied` ones that no
open question points at; such a memory keeps its text, the ledger just no
longer remembers the correction, so the next correction of it applies as if
it were the first. Open questions are never dropped: a correction the ledger
cannot record because it holds nothing but open conflicts answers `507`
without touching the memory, and resolving some makes room. A correction is
recorded only after the memory file was rewritten, and the file is rewritten
only once the ledger is known to have room for the record.

Conflicts are never merged. A correction of a memory whose earlier
correction is still in force (the memory still reads exactly as that
statement) and whose text differs is parked as `needs_resolution` and the
memory stays as it was; the response lists both statements and the user
chooses. Only one question is open per memory: further statements answer
with the same open conflict until it is resolved. Once the companion has
rewritten the memory itself (or it was forgotten and written anew), the
earlier correction is no longer in force: the next correction of that memory
marks it `superseded`, closes any question that was parked against it the
same way (the memory reads as neither statement, so there is nothing left to
choose between), and applies directly. The `current` side of a `409` is
therefore always the text the memory holds on disk.

| Route | Purpose |
| --- | --- |
| `PUT /api/instances/companion/memory/{path}` | `{content}`: `200 {status: "applied", correction}`, `200 {status: "unchanged"}`, or `409 {status: "needs_resolution", conflict_id, current, proposed}`; `404` unknown memory, `422` empty or invalid, `413` over 64 KiB, `507` ledger full of open conflicts |
| `PATCH /api/instances/companion/memory/{path}` | `{pinned?, exclude_from_proactive?}`; a flag left out is unchanged; returns both |
| `GET /api/instances/companion/memory-corrections` | the ledger |
| `POST /api/instances/companion/memory-corrections/{id}/resolve` | `{keep: "current" \| "proposed"}` settles one open conflict |

Forgetting (`DELETE …/memory/{path}` and the `memory_forget` chat tool) is
unchanged and removes the file and its index entries regardless of flags.

### Profiles: several servers on one host

One host can run several fully isolated servers from one binary (#107). Each is a
*profile*: `nolune --profile <name> …` (or `--profile <name>` after any
subcommand) addresses its own data root, config, port, auth token, log, and
background service. The rule is one profile = one server = one companion: a
profile never holds more than one identity, and nothing is shared between
profiles.

| Profile | Data root | Service |
| --- | --- | --- |
| `default` (no flag) | `~/.nolune/` (or `NOLUNE_HOME`) | `dev.nolune.nolune` / `nolune.service` |
| any other `<name>` | `~/.nolune-profiles/<name>/` | `dev.nolune.nolune.<name>` / `nolune-<name>.service` |

Profile roots are siblings of `~/.nolune`, never inside it, so
`nolune uninstall --yes` on one profile cannot touch another. Inside a root the
layout above is identical. Names match `^[a-z0-9][a-z0-9-]{0,31}$` and are
local deployment metadata only: never a companion identity, a federation
address, or a trust anchor. `nolune onboard --profile <name>` picks a free
port above `26559` (or takes `--port`); `nolune gateway install --profile
<name>` refuses to share a port, a data root, or a service with a sibling
profile, and `nolune uninstall` / `nolune gateway uninstall` refuse to act on a
root that belongs to another profile. A named profile refuses to run when
`NOLUNE_HOME` points anywhere other than its own root. The binary itself is
shared: every profile's service runs `~/.nolune/bin/nolune`, so uninstalling
the default profile (even with `--keep-data`) warns which profiles' services
will stop at their next restart; reinstall and run
`nolune gateway install --profile <name>` again, or remove them with
`nolune gateway uninstall --profile <name>`. Co-located profiles receive no
implicit trust: they talk to each other only through the federation protocol
(#108).

## Wire shape

### Slug in URLs, bodies, and events

Routes keep the `instance_slug` path segment for now (`/api/instances/{slug}/…`,
`/api/chat/{slug}/…`, `/public/files/{slug}/…`, `/public/memory/{slug}/…`). The
only accepted value is `companion`, matched exactly.

- Any other slug in a path, in the `POST /api/chat` body, or in a public media
  URL returns `404 {"error":"unknown_companion"}` before any handler runs and
  has no side effects.
- `GET`, `HEAD`, `OPTIONS`, and `DELETE` on the canonical slug never create
  storage. Any other method creates-or-opens the companion by writing the
  identity marker first.
- `GET /api/companion` returns the one companion context:
  `{"slug":"companion","exists":false,"companion_name":"","soul_exists":false}`
  until onboarding creates it, then `exists: true` with its name. `GET /api/meta`
  reports `companion_slug` and `instances_count` (`0` or `1`).
- The multi-instance listing (`GET /api/instances`) and companion deletion
  (`DELETE /api/instances/{slug}`) were removed in #104. They, and any other
  unknown `/api/*` path, answer `404 {"error":"not_found"}` after
  authentication instead of serving the web client shell.
- The client never mints a slug: `/` opens `/companion`, and a stale
  `/{other}/…` URL redirects to `/companion/…` keeping its chat or settings
  context.
- Server events (`instance_slug` fields), resource capabilities
  (`instance_slug`), and machine registrations (`MachineInfo.instance_slug`)
  always carry `companion`. A machine registration that names any other slug
  is bound to `companion`.

### Export archive

`GET /api/instances/companion/export` streams a `tar.gz` whose entries are all
rooted at `companion/` (for example `companion/companion.json`,
`companion/soul.md`, `companion/memory/…`). Import (#74) must require a valid
`companion/companion.json` with `format_version: 1` and reject archives with
any other root, slug, or version. The full contract is the
[archive format](#archive-format) below.

## Archive format

Version 1 of the backup archive is the companion directory verbatim, so it
shares `format_version` with the storage layout above. The export route and
the `create_backup` tool produce it in-process
(`server/src/services/profile_archive.rs`, #74) and the same module reads it
back; no external `tar` runs in either direction.

### Container

- A gzip-compressed tar stream, offered as `companion.tar.gz` with media type
  `application/gzip`.
- Every entry path starts with `companion/`; the archive root is the companion
  directory. A `companion/` directory entry may appear once.
- Only regular files and directories. Symlinks, hard links, devices, fifos,
  sparse and contiguous files, and pax `size` overrides are refused.
- Paths are UTF-8, relative, `/`-separated, at most 4096 bytes, without empty,
  `.`, or `..` segments, backslashes, drive prefixes, or NUL bytes. GNU
  long-name entries and pax `path` records carry long paths.
- No entry is repeated, and a file and a directory never claim the same path.
  Missing parent directories are created on the way.
- Modes and ownership are not preserved: files are written `0644`,
  directories `0755`. Modification times are informational.

### Manifest

`companion/companion.json` is the manifest. It is the identity marker
described above, byte for byte:

```json
{
  "format_version": 1,
  "slug": "companion"
}
```

The writer emits it first. A reader validates it as soon as it is seen and
refuses the archive when it is missing, larger than 4 KiB, carries unknown
fields, or names another `format_version` or `slug`. Archives that still hold
a retired layout (`stats/`, `agents/`, `agent_runs/`, `thoughts/`) are refused
rather than trimmed.

### Limits

Readers enforce these caps while streaming, so a hostile archive is cut off
before it can fill disk or memory:

| Limit | Value |
| --- | --- |
| Entries, long-name and pax entries included | 100 000 |
| One regular file | 512 MiB |
| All regular files together | 8 GiB |
| Bytes leaving the gzip decoder | 8.25 GiB |

### Extraction

The reader writes only into a staging directory handed to it as a capability:
every file is created with `create_new`, symlinks are never followed, and
files and directories are fsynced before the reader reports success. It never
touches `instances/companion/` itself; publishing the staged tree is the
restore described next.

Exporting skips symlinks, special files, and retired layouts (they are
counted, never followed), so a fresh export always imports. An export that
fails part-way never completes the archive: the tar end-of-archive blocks and
the gzip trailer are withheld, so whatever a client kept of the download is
refused as truncated rather than restored with files missing.

### Restore

`services/profile_import.rs` (#74) replaces the companion with an archive in
one transaction under two gates. The first is the companion's lifecycle
gate, the same `VectorStore::lifecycle_lock` every memory write, delete,
media replacement, and backfill holds: a memory write that arrives during
an import waits and then lands in the imported tree, and two imports
serialize the same way. The second is the process-wide import gate
(`services/import_gate.rs`, reached through `MediaStore::import_gate`),
which every writer that reaches the tree through plain paths holds
*shared* while it runs: the companion boundary holds it for every admitted
mutating request (`POST`, `PUT`, `PATCH`, `DELETE` with a slug, the import
route excepted) from before `ensure_identity` until the response is built,
`POST /api/chat` holds it until the message is saved and the agent loop is
registered, and every admitted proactive run (`ProactiveLoop::begin`:
heartbeat, schedule, commitment check-in, handoff continuation, machine
connect) holds it until the run is completed, failed, or cancelled. The
import holds it *exclusively* from its busy check until the previous tree
is discarded, so a request that arrives during an import waits and then
runs against the imported tree, and a run that would start then is skipped
(`skipped/import`) without writing a record. Once the busy check below has
passed, the transaction runs on a task of its own that owns both gates: a
caller that stops waiting (an HTTP client that disconnects drops the
handler future) detaches from the import rather than stopping it between
two steps, and the import finishes on its own and logs its result.

1. **Refuse while busy.** The import takes the lifecycle gate, then waits
   up to ten seconds for ambient writers in flight to release the import
   gate; writers that outlast that (a proactive run in the middle of a
   model call) are reported, never interrupted. Then, while agent loops
   exist for the companion (`agent_tasks`; they write through plain paths
   and hold no gate, and only the conversation running the `restore_backup`
   tool is discounted, because it is blocked on the call), the restore
   returns a typed `busy` error before anything is staged. The route maps
   both refusals to `409 companion_busy`, and answers the agent-loop one
   before it reads the request body.
2. **Stage.** The archive is extracted into `imports/staging-<id>/` through
   the workspace capability. `imports/` is a top-level directory, never a
   sibling under `instances/`, so a half-extracted tree is never mistaken
   for an obsolete companion. A refused archive is discarded here and the
   companion is untouched.
3. **Validate.** The staged marker is re-read and validated right before
   the swap.
4. **Swap.** `instances/companion` is renamed to `imports/previous-<id>`,
   then the staged tree is renamed to `instances/companion`. If the second
   rename fails, the previous tree is renamed back and the error says so;
   the tree and the derived index are exactly what they were. If that
   rollback also fails, the previous companion is left intact at
   `imports/previous-<id>` and the error names it. Both renames and the
   rollback run on one blocking thread, so no other task is scheduled
   between them and, once started, they run to completion. The cached
   uploads directory handle is dropped on both sides of the swap.
5. **Rebuild derived state.** The vector collection is reset (which also
   invalidates BM25) and backfilled from the imported `memory/`; the catalog
   snapshot is rebuilt and the memory graph is loaded from the imported
   file. When the embedding provider is unconfigured or unreachable the
   result reports `derived_index: pending`, the collection stays marked for
   the startup backfill (`needs_backfill`), and BM25 rebuilds on the next
   search; otherwise `derived_index: rebuilt`. When the emptied collection
   itself cannot be written, the cached collection is discarded and its
   index file unlinked, so the result is `pending` for that reason and
   `needs_backfill` is `true` either way: no record of the replaced tree is
   served.
6. **Discard the previous tree.** `imports/previous-<id>` is removed only
   after the new tree is in place and derived state has been handled.

Should a writer that holds neither gate ever recreate `instances/companion`
between the two renames, the second rename fails with `AlreadyExists`, the
rollback fails the same way, and the error names `imports/previous-<id>`:
the previous tree is intact, never lost.

**Startup recovery.** `profile_import::recover_on_startup` runs in
`main.rs` before the obsolete-directory report, the migration, and every
writer, and reconciles what a crash left under `imports/`:

- `instances/companion` missing and exactly one `imports/previous-<id>`:
  the process died between the two renames; the parked tree is moved back
  and the companion is byte for byte what it was before the import. The
  derived index was never rebuilt, so it still describes that tree.
- `instances/companion` present and a `previous-*` beside it: the swap had
  published the import; the parked tree is the replaced data and is
  removed, and because the derived rebuild may not have run the vector
  collection is reset so the startup backfill re-indexes the live tree.
- `instances/companion` missing and several `previous-*` trees: nothing is
  moved; the log names them for an operator to choose.
- `staging-*` directories and `upload-*` archives are removed. Anything
  else under `imports/`, and any symlink there, is left alone and logged.

Three surfaces reach this restore, and nothing else writes the companion
tree wholesale:

- `POST /api/instances/companion/import` takes a multipart `file` field.
  The body streams into `imports/upload-<id>.companion.tar.gz` through the
  workspace capability as it arrives (bounded by a `DefaultBodyLimit` just
  above the reader's 8 GiB payload cap, never buffered in memory), the
  restore reads it from there, and the file is removed afterwards. The
  answer is `200` with `{ok, files, directories, bytes, derived_index,
  pending_reason?, indexed_chunks}`; `409 companion_busy` while an agent
  task runs; `400 archive_refused` (or `413 archive_too_large`) for an
  archive the reader rejects, with the companion exactly as it was; `500
  import_failed` or `import_stranded` (the message names
  `imports/previous-<id>`) when the swap itself failed. The Data settings
  page asks once, in the destructive confirmation dialog, before sending
  and shows the upload and the restore.
- The `restore_backup` tool takes only the upload id (`upload_<id>`) of an
  archive attached to the chat or produced by `create_backup`, opened
  through `MediaStore::open_upload_blob`, and only with
  `confirmed_by_user: true`, the user's explicit confirmation in that
  conversation: without it the tool refuses before anything is looked up,
  so text the model read cannot trigger a replacement. A path from the
  model is refused the same way. The conversation running the tool is
  blocked on it and does not count as busy; every other one still does.
  That conversation's remaining turn appends to the imported tree, with
  one caveat: a server-side compaction later in the same turn rewrites the
  chat's history from the loop's in-memory messages, which predate the
  restore. The tool's answer therefore asks the model to end the turn, and
  a fresh message afterwards loads the imported history.
- `nolune restore <archive> [--yes] [--profile <name>]` posts an
  operator-chosen local file to the local API with the token from
  `config.toml`. The CLI never opens the archive beyond streaming it, so
  the server's validation is the only path into the companion.

## Federation identity

Federation peers (#108) are companion signing identities, never machines,
profile names, ports, or hostnames. The identity lives at the workspace root,
outside `instances/companion/`:

| File | Contents | Mode |
| --- | --- | --- |
| `federation/identity.json` | public, self-signed identity document | `0600` |
| `federation/signing_key.json` | private Ed25519 seed: `{"version":1,"algorithm":"ed25519","secret_key":"…"}` | `0600` |

Both files are created together on first use and rewritten only by a key
rotation (below); a key without its document (or the reverse) fails closed
rather than being repaired silently. A rotation replaces the key file and
then the document, and its proof is in `federation/rotations.json` before
either: a server that died between the two renames finds the new key beside
the old document on the next start and completes the rotation from the
proof, which names both; any other key that does not match its document
fails closed. The export archive is rooted at `companion/`, so it never contains
either file: an export carries the companion's memory and settings, not its
federation identity. To move the companion to another host, copy `federation/`
alongside `instances/`; the identity verifies there because nothing in it
names the old host, port, service label, profile name, or path, and peers keep
trusting the same key. Deleting `federation/` creates a new identity on the
next use, which peers must pair with again.

The identity document, version 1:

```json
{
  "version": 1,
  "companion_id": "<base64url sha256 of the public key>",
  "public_key": "<base64url Ed25519 public key>",
  "created_at": 1789862400,
  "signature": "<base64url Ed25519 signature>"
}
```

`companion_id` is derived from `public_key`
(`sha256("nolune/federation/companion-id/v1\0" || key)`), the signature covers
the canonical bytes of the other four fields, and every binary field is
base64url without padding. A document with another version, an id that is not
derived from its key, unknown fields, a public key that is not a curve point
(or is a small-order point, which no seed produces), or a signature that does
not verify strictly is rejected before anything trusts it, and a signing key
file that other users can read is refused on load. Errors about the signing key
file describe its shape and never quote its contents. Wire fixtures live in
`server/tests/fixtures/federation/`; `generate.py` there rebuilds them with
OpenSSL, independently of the server code.

### Peers and pairing

Trust between two companions is established by their owners, one invite at a
time, and never implied by a shared host, OS account, profile name, port, or
loopback address. `federation/peers.json` (mode `0600`) holds one record per
peer:

```json
{
  "version": 1,
  "peers": [
    {
      "identity": { "version": 1, "companion_id": "…", "public_key": "…", "created_at": 1789862400, "signature": "…" },
      "state": "paired",
      "role": "issuer",
      "pairing_id": "9f1c0b7e2a6d4c31",
      "approved_origins": ["https://molinka.example"],
      "created_at": 1789862400,
      "updated_at": 1789862460,
      "last_seen_at": 1789862460,
      "rotation_history": []
    }
  ]
}
```

That is the whole record: the peer's self-signed identity document (its key
and id), the handshake `state`, which side minted the invite (`role`), the id
of the invite that started the pairing, the base URLs the owner approved for
reaching the peer (`approved_origins`, plus a `pending_origin` the peer
reported but the owner has not approved yet), timestamps, and the key
rotation history. `updated_at` moves with the trust state; `last_seen_at`
(absent on a record written before it existed) is when something signed by
the peer last verified here, a pairing step, a notice or its
acknowledgement, or a transport envelope, and never moves for anything the
owner does locally or for a message that failed verification. No display name, hostname, port,
profile name, or address is stored, because none of them is trusted. Every
document is re-verified when the file is loaded; a record whose document no
longer verifies is dropped. A file of another version or shape is never
repaired and never overwritten: the server warns, trusts no peer, and refuses
every pairing write until the file is repaired or moved aside and the server
restarted.

States: `invited` (an invite the owner minted; it lives in memory only, as a
domain-separated SHA-256 of its one-time secret, and a restart forgets it),
`pending` (keys exchanged, one owner still has to confirm: the issuer's owner
on both sides), `paired` (both owners confirmed; the only state in which a
peer's messages verify), `revoked` (trust withdrawn; the record stays so a
stale confirmation cannot revive it, and only a new invite pairs the companion
again). An invite expires after ten minutes, is redeemed at most once, and is
shown to the issuing owner exactly once: it never appears in a URL, a log
line, a chat, or any later listing. Its secret is 32 random bytes, so wrong
guesses are not counted: a failure counter on a public route would only let
a stranger lock the owner out of a legitimate redemption.

The handshake runs over the owner routes `/api/federation/*` (behind the
normal API authentication) and the public peer routes under
`/federation/v1/pair`, which take a signed envelope in a POST body and verify
its signature and nothing else:

1. Owner A: `POST /api/federation/invites` returns the invite id, its secret,
   A's base URL, and A's identity document, plus the same three packed into
   one line as `invite` (`nolune-invite-v1.` followed by the base64url of
   the accept body), to hand to owner B out of band. The line is not a URL
   and is refused wherever it looks like one.
2. Owner B: `POST /api/federation/accept` with that origin, secret, and
   document, or with `{ "invite": "<the line>" }`. B pins A's document,
   then posts a signed `pair_request` (the
   secret, B's own document, B's base URL) to `{origin}/federation/v1/pair`.
   A verifies the document and the signature, redeems the secret, records B as
   `pending`, and answers with a signed `pair_response`; B verifies it against
   the pinned document and records A as `pending`.
3. Owner A: `POST /api/federation/peers/{companion_id}/confirm` marks B
   `paired`, approves the origin B reported, and posts a signed `pair_confirm`
   to it; B marks A `paired` and acknowledges.
4. Either owner: `POST /api/federation/peers/{companion_id}/revoke` marks the
   peer `revoked` and posts a signed `pair_revoke`; the peer does the same.
   `GET /api/federation/peers` lists the identity, outstanding invites, and
   peers; `DELETE /api/federation/invites/{id}` withdraws an invite.

Every notice names its pairing id, so a notice about an earlier pairing is
stale, and a body of one kind is never read as another. Every transition is
checked and applied under the store's lock against the record as it is at
that moment, so a confirmation that races a revocation can never leave a
revoked peer paired. Two profiles on one host go through exactly these steps
over their own ports. The owner drives them with `nolune federation …` or
the Companions section of Settings → Connections; [federation.md](federation.md)
is the guide.

### Transport envelope

After pairing, companions talk through the transport envelope, version 1:

```json
{
  "version": 1,
  "sender": "<companion_id of the signer>",
  "recipient": "<companion_id the envelope is for>",
  "nonce": "<base64url, 16 random bytes>",
  "issued_at": 1789862400,
  "expires_at": 1789862460,
  "body_hash": "<base64url sha256 of the body>",
  "body": "<base64url body>",
  "signature": "<base64url Ed25519 signature>"
}
```

The signature covers the canonical bytes of every field but the body, which
is bound to them by `body_hash`. The recipient checks, in this order, the
version (a downgrade is refused before anything else), that it is the
recipient, that the sender is a paired peer (its current key, or a key it
rotated away from inside the grace window below), the signature with that
key, the peer's state, the body hash, the times, and last the nonce. A body
altered after signing fails on the hash, an altered header on the signature,
a sender that is not paired, pending, or revoked with its own error, and an
envelope for someone else on the recipient. Times are judged by the
recipient's clock with a two-minute skew allowance in either direction: an
envelope is refused as issued in the future beyond that, as expired once
`expires_at` plus the allowance has passed, and outright when it claims a
lifetime over five minutes (this server seals envelopes for sixty seconds).
Each nonce is accepted once per sender and remembered until the envelope
could no longer be accepted anyway, in a bounded replay set of 65 536
entries that refuses new envelopes rather than forgetting old nonces; nothing
that failed an earlier check consumes a nonce, so a stranger cannot fill the
set. The set lives in memory: a restart forgets it, which is bounded by the
same lifetime plus allowance. Message semantics beyond a ping arrive with
later work; `POST /federation/v1/ping` takes an envelope from a paired peer
and answers with one, addressed to the sender. Wire fixtures live beside the
identity ones in `server/tests/fixtures/federation/`.

### Key rotation

`POST /api/federation/rotate` replaces this companion's key. A new key is
generated, the rotation proof is appended to `federation/rotations.json`
(mode `0600`) first, then `signing_key.json` and `identity.json` are replaced
through temporary files and renames, outstanding invites (which carried the
old document) are withdrawn, pending pairings are revoked, and every paired
peer is posted a `key_rotation` notice at its approved origins, inside a
transport envelope signed by the retiring key. The proof is the new
self-signed document with two signatures over the same canonical bytes (both
ids, both keys, and `rotated_at`): the old key's `endorsement` and the new
key's `signature`. The response reports the new identity, the proof, and
which peers acknowledged; a peer that could not be reached still needs the
proof, which the history keeps. A pending pairing was started under the
retired identity and cannot finish under the new one (this side would sign
the confirmation with a key the peer does not know, and the peer would
address its confirmation to an id this side no longer has), so the rotation
marks every pending record `revoked`, whichever side issued the invite, and
both owners pair again with a new invite; the peer is not told and keeps
its pending record until then. The owner's confirmation and the rotation
take the same lock, so a confirmation cannot slip in between the key change
and the revocation. A proof whose new key equals the old, whose documents do
not verify, or whose signatures fail is refused.

A peer receives the notice at `POST /federation/v1/rotate`, verifies the
envelope with the retiring key, checks that the notice comes from the key it
retires, verifies the proof, and re-keys its record under the store's lock:
the peer must be paired and its identity on record must still be the
previous one, the record takes the new identity, and the rotation is appended
to its `rotation_history` with the time it was accepted (the newest 32 are
kept). Nothing else about the record changes: the pairing id, role, and
approved origins stay. The retiring key keeps verifying for fifteen minutes
after acceptance, so envelopes already in flight still land, and is then
refused as retired; a rotation notice resent inside that window is
acknowledged again without being applied twice. The peer answers with a
`rotation_ack` addressed to the new identity. Companion ids are derived from
keys, so a rotated companion has a new id; the transition record ties the
two together, and both histories can be re-verified at any time.

### Policy and audit

A paired peer has no implicit access to anything (#109). Every verified
envelope is classified into an intent (`ping`, `message`, `availability`,
`reminder`, `proposal`) and a disclosure class (`none`, `availability`,
`personal`, `sensitive`: what an answer would reveal about this owner),
judged against `federation/policy.json` (mode `0600`, beside `peers.json`),
and recorded before anything is dispatched. Memory and tool access have no
intent class: there is nothing to grant. A kind or class the server does
not know is denied and recorded as `unknown`, with the name the peer used
reduced to `[a-z0-9_]` and bounded.

```json
{
  "version": 1,
  "quiet_hours": { "start_hour": 22, "end_hour": 7, "timezone": "Europe/Berlin" },
  "rate_limit": { "max_requests": 60, "window_secs": 60 },
  "peers": {
    "<companion_id>": {
      "rules": [
        { "intent": "message", "disclosure": "none", "access": "allow", "granted_at": 1789862400, "expires_at": 1790467200 }
      ],
      "rate_limit": { "max_requests": 10, "window_secs": 60 }
    }
  }
}
```

A rule is `allow`, `ask` (the owner decides each time), or `deny` for one
intent at one disclosure class, and matches exactly: a grant at one class
says nothing about another. From `expires_at` on the rule no longer applies
and the default does. Where no rule applies, the defaults are closed: only a
`ping` at `none` is allowed (pairing is the consent to be reachable; a ping
discloses nothing more); a `message`, a `reminder`, a `proposal`, and an
`availability` query at `availability` ask the owner; the `sensitive` class
and any combination an intent cannot disclose at are denied. The checks run
in a fixed order and each one short-circuits: a revoked or unpaired peer is
denied whatever the rules say; past the rate limit (the document's, or the
peer's own) the answer is a denial with a retry-after and nothing further is
consulted; then the rules; and inside quiet hours (read in the given IANA
zone, UTC when unset) anything that would land in front of the owner, or
would ask them, is deferred until they end. Owners revoke a rule or a peer
by removing it, and the very next evaluation sees the change. Rules, the
rate window, and the refusal window are keyed by the peer's companion id;
when a peer rotates its key they move to its new id in the same step that
applies the rotation, under the policy lock, so a rotation never sheds a
denial or refills a budget, and a rotation is refused while the policy
cannot be loaded rather than applied without the rules that go with it.
The rate limit bounds decisions, not verification: the transport checks
the signature and reserves the nonce before the policy runs, so a peer past
its budget still costs one signature check and one replay-guard slot per
request until the transport lets the gate limit a verified sender before
its nonce is reserved. Quiet hours are hours (0-23); a document that says
otherwise is refused on write and unloadable on read. A missing file is
the default document and is not written until the owner changes something;
a file of another version or shape is never repaired and never
overwritten: nothing is judged until it is repaired or moved aside.

`federation/audit.jsonl` (mode `0600`) keeps one receipt per line for every
decision, on both sides: the requesting companion records what it asked and
what came back, the answering companion records what it was asked and what
it decided. A receipt names the requester and responder ids, the pairing
(which a key rotation does not change), the intent and disclosure class,
the decision (verdict, reason, retry-after or deferred-until) and the time,
plus a one-line summary built from those names. Receipts never contain what
the peer sent: no body, message, or text field exists in the shape, and
unknown fields are refused. Retention is bounded like proactive run
records: the newest 1000 overall, the newest 200 per pairing under every id
the peer has had (so one chatty peer cannot push the others out, and cannot
start over by rotating its key), and nothing older than 30 days; past a
bound the file is compacted through a temporary file and a rename.
Repeated refusals of one kind from one peer inside a minute are recorded
once. A log this build cannot load or write refuses every intent: a
decision is not made without its receipt.

Over the wire a refusal is `403` with `policy_denied`, `approval_required`,
or `deferred`, or `429 rate_limited`, each carrying the decision in its wire
shape: a rate limit says how long the peer's own window has left (as a
`Retry-After` header too), while a deferral says only that it was deferred,
because when the owner's quiet hours end is the owner's schedule and stays
in the owner's receipt; a revoked or unpaired sender keeps its own code and
is recorded too, because its signature was checked before its state. Key rotation notices are trust maintenance rather than intents: they
are gated by the peer's state and recorded as `key_rotation` once accepted.
`GET /api/federation/policy` lists the document and the defaults table;
`GET /api/federation/receipts` lists the receipts, newest first; both are
read-only. Peer text is data, never instructions: it has no accessor and
can only be rendered inside a delimited block that names it as untrusted
content from a named companion, framed by a boundary the text cannot
predict, and it never becomes a tool argument.

`federation/approvals.json` (mode `0600`) is the owner's queue: when the
engine answers `ask`, the gate keeps one entry per peer, intent, and
disclosure class (`id`, `pairing_id`, `requester`, `intent`, `disclosure`,
`status`, `requested_at`, `decided_at`, `expires_at`, `summary`; no body,
text, or payload field exists, and unknown fields are refused) and tells
the peer `approval_required`, the same way on every retry, so nothing about
the owner's decision or its timing crosses the wire until an intent is
allowed. A pending entry lapses after 24 hours. `GET /api/federation/approvals`
lists the live entries newest first; `POST …/approvals/{id}/approve` and
`…/deny` take `{"scope": "once"}` (the next matching intent consumes an
approval, which lapses unused after an hour; a denial holds until the
request would have lapsed and the peer is not queued again meanwhile),
`{"scope": "until", "expires_at": …}` or `{"scope": "class"}` (both become
a rule in `policy.json` and drop the entry); `DELETE …/approvals/{id}`
withdraws an entry whatever it stands at. `POST
/api/federation/peers/{id}/rules` writes one rule (`intent`, `disclosure`,
`access`, optional `expires_at` in the future; a pair the intent cannot
disclose at is refused), replacing the rule for that pair, and `DELETE
…/rules/{intent}/{disclosure}` revokes one capability; the next evaluation
sees either. Revoking a peer, by this owner or by the peer's own notice,
drops its rules, its rate-limit override, and every entry of its pairing:
a revoked peer keeps nothing, and pairing it again starts from the
defaults. Every owner decision is a receipt on the `owner` side (reasons
`owner_approved`, `owner_denied`, `owner_revoked`, `rule`, and
`peer_revoked` under the intent name `revocation`), written before the
change is applied. The queue moves with the rules when a peer rotates its
key, a rotation is refused while the queue cannot be loaded, and a queue
file this build cannot load refuses every intent that would ask the owner
(a ping never consults it) while being neither repaired nor overwritten.

## Changing this format

Bump `format_version` whenever the directory layout, the marker shape, or the
export root changes, and add a read test for the previous version before
shipping the writer. Server constants live in `server/src/domain/companion.rs`;
the filesystem boundary is `server/src/services/companion.rs`, the only module
allowed to enumerate `instances/`.
