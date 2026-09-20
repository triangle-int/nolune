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
        ├── chats/{chat_id}/     conversation history and agent markers
        ├── scheduled/*.json     scheduled tasks
        ├── activity/*.json      proactive run records (docs/proactive-loop.md)
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
