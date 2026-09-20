# Proactive loop

Everything the companion starts on its own passes through one loop (#92):
hourly check-ins, explicit schedules, connected-computer events, manual
triggers, and the future commitment (#85) and handoff (#82) triggers. There is
one execution record, one policy, and one place where duplicates, quiet hours,
cooldowns, and the attention budget are enforced.

## Execution record

Each run is one JSON file under `instances/companion/activity/{id}.json`,
format version 1:

| Field | Meaning |
| --- | --- |
| `id` | `run_<unix seconds>_<8 hex>` |
| `trigger` | `heartbeat`, `schedule`, `machine_connected`, `manual`, `commitment`, `handoff` with their identifying field |
| `reason` | stated, user-readable reason, at most 200 characters |
| `target` | `companion`, `chat` (with `chat_id`), or `machine` (with `machine_id`) |
| `dedupe_key` | derived from the trigger; runs sharing a key never execute concurrently |
| `status` | `running`, `completed`, `failed` (`error`, `retryable`), `cancelled`, or `skipped` (`quiet_hours`, `cooldown`, `duplicate`, `disabled`) |
| `attempt`, `retry_of` | attempt number and the id this attempt retries |
| `approvals` | each side-effect decision: `reach_out`, allowed or not, reason, time |
| `outcome` | receipts only: tool names with short summaries, `messages_sent`, `tokens` |

Records never contain model text, hidden reasoning, or tool traces. Skips are
recorded too, so a quiet or rate-limited period stays explainable.

## Lifecycle

1. A trigger calls `begin`. The loop admits or skips it under the policy:
   disabled → `skipped/disabled`; same `dedupe_key` already running →
   `skipped/duplicate`; spontaneous trigger inside quiet hours →
   `skipped/quiet_hours`; event trigger within `cooldown_secs` of its last
   finish → `skipped/cooldown`.
2. The worker holds a handle with a cancellation token and ends the run with
   exactly one of `complete`, `fail`, or `cancel`.
3. Side effects that leave companion storage (today: `reach_out`) call
   `approve_side_effect`, which denies during quiet hours or once the rolling
   24-hour `daily_reach_out_budget` is spent, and records the decision on the
   run. Denials are returned to the model as tool errors.
4. On startup, runs left `running` by a dead process are marked
   `failed` (`interrupted by server restart`, retryable), then retention
   removes finished runs beyond `retention_max` or older than
   `retention_days`. Running runs are never removed.

Explicit schedules ignore quiet hours and cooldown (the user or companion
asked for that time) but still cannot message the user during quiet hours.

## Policy

`instances/companion/proactive_policy.json`, editable through
`GET/PUT /api/instances/companion/proactive`:

| Field | Default | Effect |
| --- | --- | --- |
| `enabled` | `true` | master switch for spontaneous behavior |
| `quiet_hours` | none | `{start_hour, end_hour}` in the companion's timezone; may wrap midnight |
| `cooldown_secs` | 600 | minimum gap between runs of the same event trigger |
| `daily_reach_out_budget` | 6 | spontaneous messages per rolling 24 hours (the attention budget) |
| `retention_max` | 200 | finished records kept |
| `retention_days` | 30 | finished records older than this are removed |
| `check_in_interval_hours` | 1 | hours between check-ins (0.25 to 720) |
| `reflection_enabled` | `false` | opt in to the reflection routine |
| `reflection_interval_hours` | 72 | hours between reflections when enabled |

## API

| Route | Purpose |
| --- | --- |
| `GET /api/instances/companion/activity?limit=` | newest records first |
| `GET /api/instances/companion/activity/{id}` | one record |
| `POST /api/instances/companion/activity/{id}/cancel` | signal a running run; `409` when nothing is running |
| `POST /api/instances/companion/activity/{id}/retry` | new attempt of a retryable failure or a cancelled run; `409` otherwise |
| `GET/PUT /api/instances/companion/proactive` | policy |

## Routines

The companion has two routines (#93). There is no configurable agent set, no
agent TOML, no tool groups, and no `call_agent` tool.

| Routine | Trigger `agent` | Default interval | Tools |
| --- | --- | --- | --- |
| Check-in | `companion` | `check_in_interval_hours` = 1 | `reach_out`, `create_drop`, `memory_write`, `memory_read`, `memory_list`, `memory_search`, `read_email` (only with a configured account) |
| Reflection (opt-in) | `reflection` | `reflection_interval_hours` = 72 | `memory_write`, `memory_read`, `memory_list`, `memory_search`, `memory_connect` |

Neither routine can run commands, touch files or connected computers, send
email, or call `memory_forget`; bulk memory rewriting by a background job is
gone with the retired night-maintenance agent. Reflection is off until
`reflection_enabled` is set, and it can add or connect memories but never
delete them. A user-written `instances/companion/heartbeat.md` is appended to
the check-in prompt as guidance.

Routines run in the subagent execution scope (#137): with the Anthropic
provider their prompt-cache entries live five minutes, long enough to chain
the tool calls of one run. A conversation runs in the conversation scope and
caches for one hour, so a person can pause mid-chat and come back without
the whole prefix being sent uncached again.

The next run of a routine is derived from its last finished activity record,
so schedules survive restarts without marker files. A machine-connect event
runs the check-in once with a connection task.

Retired child-agent state (`agents/`, `agent_runs/`) is removed from the
companion directory once at startup; it is never executed.

## Receipts instead of thoughts (#94)

The raw Thoughts store, its route, tab, and `heartbeat_thought` event are
gone. Routines return only their tool trace, which the loop reduces to
receipts; the model's private text is dropped in memory and never written
anywhere. Historical `thoughts/` files are removed from the companion
directory once at startup and cannot influence behavior. Mood remains
bounded companion state in `mood.json`.

The client's **Activity tab** lists the records newest first (trigger, stated
reason, status, receipts, held messages) with cancel and retry, and updates
live from the `activity_updated` server event, which carries the same
record shape as the API. Initiative controls (on/off, check-in interval,
quiet hours, daily message budget, reflection) live under Settings →
Companion.

## Commitments (#85)

A commitment is a promise the companion tracks as first-class state, so
following through never collapses into a timer. Each one is one JSON file
under `instances/companion/commitments/{id}.json`, format version 1. The
record is the source of truth: the evaluator reads it and keeps no schedule
of its own, so a restart cannot create a duplicate one. The store has one
writer at a time: a lock is held across every read-modify-write (edit,
snooze, complete together with the dependents it settles, cancel), and each
write goes to a temp file of its own before it is renamed over the record,
so concurrent writers can neither interleave on one record nor share a temp
file. A record that no longer parses is logged and skipped, not hidden.

| Field | Meaning |
| --- | --- |
| `id` | `cmt_<unix seconds>_<8 hex>` |
| `promise` | what was promised, at most 500 characters |
| `owner` | `companion` (it promised the user) or `user` (they asked it to hold them to it) |
| `status` | `active`, `waiting`, `blocked`, `due`, `completed`, `dismissed`, or `failed` |
| `deadline` | `{kind: at, at}` or `{kind: window, start, end}`; optional |
| `dependencies` | ids of commitments that must complete first |
| `waiting_on` | `{kind: until, until}`, `{kind: event, event}`, or `{kind: user_reply}`; optional |
| `next_check` | when the evaluator next looks at it: the one schedule the record owns |
| `continuity_ids` | linked continuity record ids (#81), plain strings |
| `provenance` | `manual`, `chat` (`chat_id`, `message_id`), or `run` (`run_id`) |
| `completion` | evidence it was done: `confirmed_by_user`, `summary`, `run_id`, `at` |
| `snoozed_until`, `snooze_count` | not surfaced before this moment; how often it was deferred |
| `last_check` | what the last evaluation concluded (`unchanged`, `triggered`, or `failed` with `retryable`) and the run it produced |

Status is derived from the record's own fields: a started deadline makes it
`due` (unless snoozed), an unfinished dependency makes it `blocked`, an unmet
waiting condition makes it `waiting`, otherwise it is `active`. Completing a
commitment re-derives every open commitment that depended on it; a dismissed
dependency never finishes. On creation `next_check` defaults to the earliest
of a timed wait and the deadline start; moving either moves it along unless
the edit sets `next_check` itself. A snooze sets `next_check` to its end and
turns a `due` commitment back to `active` until then. Every read (`get`,
`list`, and the routes) re-derives an open status against the clock, so a
passed deadline, an ended timed wait, or an expired snooze reads as `due` or
`active` at once; reading persists nothing, `status_changed_at` is the last
transition a write recorded, and closed statuses are never re-derived.

A commitment cannot be marked complete without explicit user confirmation
(`confirmed_by_user`) or recorded evidence (a non-empty `summary`, or the
`run_id` of the activity record that shows the work): the store answers
`evidence_required` and changes nothing. A `run_id` only counts when that
activity record exists (`activity/{run_id}.json`): the store asks the
proactive loop, and a made-up run answers `invalid` and changes nothing.
Completed, dismissed, and failed commitments are history; edits, snoozes,
and further completion answer `closed`.

| Route | Purpose |
| --- | --- |
| `GET /api/instances/companion/commitments?status=` | newest first; `open` (default), `closed`, or `all` |
| `POST /api/instances/companion/commitments` | create; `201` with the record |
| `GET /api/instances/companion/commitments/{id}` | one record |
| `PATCH /api/instances/companion/commitments/{id}` | edit fields; `clear_deadline`, `clear_waiting_on`, `clear_next_check` remove optional ones |
| `POST /api/instances/companion/commitments/{id}/snooze` | `{until}`, which must be in the future |
| `POST /api/instances/companion/commitments/{id}/complete` | body is the evidence; `422 evidence_required` without confirmation or evidence, `400 invalid` for a `run_id` with no activity record |
| `POST /api/instances/companion/commitments/{id}/cancel` | dismiss |

Refusals are JSON `{error, message}` with `not_found` (404), `invalid`
(400), `closed` (409), `evidence_required` (422), or `storage_error` (500).
Every write is broadcast as a `commitment_updated` server event carrying the
record. The evaluator that turns due commitments into `commitment`-triggered
runs through this loop, the chat tools, and the client controls follow in
later changes.

## Migration hooks

#85 and #82 add the commitment and handoff triggers.
