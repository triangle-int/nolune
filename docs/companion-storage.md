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
├── skills/                      installed skills (global)
├── vectors/                     derived vector index, keyed by slug
└── instances/
    └── companion/               the one companion
        ├── companion.json       identity marker (see above)
        ├── soul.md              personality definition
        ├── project_state.json   name, timezone, and other settings
        ├── instance.toml        per-companion configuration
        ├── memory/              long-term memory library (source of truth)
        ├── chats/{chat_id}/     conversation history and agent markers
        ├── scheduled/*.json     scheduled tasks
        ├── activity/*.json      proactive run records (docs/proactive-loop.md)
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
any other root, slug, or version.

## Changing this format

Bump `format_version` whenever the directory layout, the marker shape, or the
export root changes, and add a read test for the previous version before
shipping the writer. Server constants live in `server/src/domain/companion.rs`;
the filesystem boundary is `server/src/services/companion.rs`, the only module
allowed to enumerate `instances/`.
