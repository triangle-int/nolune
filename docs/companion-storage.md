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
│   └── signing_key.json         private Ed25519 seed, mode 0600
├── skills/                      installed skills (global)
├── vectors/                     derived vector index, keyed by slug
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
        ├── proactive_policy.json quiet hours, budget, routine intervals
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
work the user asked for, and the API below. Nothing is ever inferred from
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
| `PUT /api/instances/companion/continuity/{id}` | apply `goal`, `state`, `completed_step`, `blocker`, `clear_blockers`, `next_step`, `machine_ids`, `resources` with a required `note` |
| `POST /api/instances/companion/continuity/{id}/complete` | mark done (optional `note`) |
| `POST /api/instances/companion/continuity/{id}/dismiss` | dismiss (optional `note`) |

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
touches `instances/companion/` itself. Publishing the staged tree under the
lifecycle gate and rebuilding derived state is the remaining import work
tracked by #74; until it lands, `POST /api/instances/companion/import` answers
`501` and the `restore_backup` tool stays disabled.

Exporting skips symlinks, special files, and retired layouts (they are
counted, never followed), so a fresh export always imports. An export that
fails part-way never completes the archive: the tar end-of-archive blocks and
the gzip trailer are withheld, so whatever a client kept of the download is
refused as truncated rather than restored with files missing.

## Federation identity

Federation peers (#108) are companion signing identities, never machines,
profile names, ports, or hostnames. The identity lives at the workspace root,
outside `instances/companion/`:

| File | Contents | Mode |
| --- | --- | --- |
| `federation/identity.json` | public, self-signed identity document | `0600` |
| `federation/signing_key.json` | private Ed25519 seed: `{"version":1,"algorithm":"ed25519","secret_key":"…"}` | `0600` |

Both files are created together on first use and never rewritten; a key
without its document (or the reverse) fails closed rather than being repaired
silently. The export archive is rooted at `companion/`, so it never contains
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

## Changing this format

Bump `format_version` whenever the directory layout, the marker shape, or the
export root changes, and add a read test for the previous version before
shipping the writer. Server constants live in `server/src/domain/companion.rs`;
the filesystem boundary is `server/src/services/companion.rs`, the only module
allowed to enumerate `instances/`.
