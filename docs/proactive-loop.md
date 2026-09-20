# Proactive loop

Everything the companion starts on its own passes through one loop (#92):
hourly check-ins, explicit schedules, connected-computer events, manual
triggers, commitment checks (#85), and the future handoff (#82) trigger.
There is one execution record, one policy, and one place where duplicates,
quiet hours, cooldowns, and the attention budget are enforced.

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
| `POST /api/instances/companion/activity/{id}/retry` | new attempt of a retryable failure or a cancelled run; `409` otherwise. A commitment check is retried through its evaluator (#85, below) |
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
runs the check-in once with a connection task, and a commitment whose check
has come up runs it once with the commitment and its trigger condition as
the task (#85, below).

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
writer at a time: a lock, shared by every store opened over the same
directory in the process (the API, the chat tools, the evaluator), is held
across every read-modify-write (edit, snooze, complete together with the
dependents it settles, cancel, the evaluator's check), and each write goes
to a temp file of its own before it is renamed over the record, so
concurrent writers can neither interleave on one record nor share a temp
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
| `last_check` | what the last evaluation concluded (`unchanged`, `triggered`, `failed` with `retryable`, or `observed` with the event) and the run it produced; a failed or denied check also keeps the observation it was for as `pending_event` |

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
record.

### Client

The Activity tab lists commitments above the activity receipts
(`client/src/lib/components/commitments/CommitmentsSection.svelte`, helpers
in `client/src/lib/commitments/commitments.js`): open ones first (due,
then active, waiting, blocked, each by next check) with a history filter
for completed, cancelled, and failed ones. Every card says who promised,
where it came from, when it is due or what it waits on, whether it is
snoozed, when the next check is, and what the last check concluded, with a
link to that check-in's activity record. The user can inspect the record's
details, edit the promise and deadline, snooze it (presets or a chosen
time, in the future), complete it, or cancel it. Completing is never
silent: the control asks for the user's confirmation or evidence text and
sends exactly that as the evidence, so the server's rule above holds from
the client as well. A commitment check-in in the activity list states its
trigger condition in plain words ("Its deadline arrived", "A commitment it
depended on was completed") and links back to the commitment, which is
revealed even when it sits under the history filter. Records update live
from `commitment_updated`. The quiet hours and messages-per-day controls
on the Companion settings page are the ones commitment check-ins obey.

### Evaluation

The evaluator (`services/commitment_evaluator.rs`) runs on the scheduler's
30-second tick under the canonical companion. It keeps no schedule of its
own: each tick asks the store for the open, unsnoozed commitments whose
`next_check` has passed and offers each one to the loop with
`begin(Trigger::Commitment {commitment_id})`, target `companion`, and a
stated reason of the form `deadline arrived: <promise>`. Admission never
moves `next_check`. A commitment whose check is still running (the loop
reports the run under its dedupe key, `commitment:<id>`) is not offered
again and nothing is recorded for it, so a check that takes a few minutes
does not leave a skipped record per tick; the dedupe key stays the backstop,
and anything else that offers the commitment while its check runs is
recorded as `skipped/duplicate` and starts nothing. A restart cannot create
a second schedule because the record is the only one. A run left `running`
by a dead process is marked failed on startup like any other, and the
record's unchanged `next_check` has the next tick check the commitment once.

Checks are held, not spent, when contact is not possible: while `enabled`
is off, during quiet hours, and once the rolling `daily_reach_out_budget` is
used up (the loop's `reach_out_allowed` answers without consuming anything).
Nothing is recorded for a hold; the commitment stays `due` and is checked on
the first tick after contact becomes possible, and the check-in's own
`reach_out` still goes through `approve_side_effect`. A denied reach-out is
recorded on the run and the commitment is looked at again after a
10-minute backoff.

The check runs the companion check-in (its curated tools; `reach_out`
gated by the run) with a task that states the commitment (promise, owner,
status, deadline, what it waits on, dependencies, snoozes) and, under
"what changed and why now", the exact trigger condition:

| Condition | When | Stated as |
| --- | --- | --- |
| observed | `last_check` holds an observation (below) | `the event it waited for was observed: machine_connected:mac`, or `the commitment it depended on (<id>) was completed: "<promise>"` |
| snooze ended | `snoozed_until` passed since the last check | `the snooze until <time> ended` |
| deadline arrived | the deadline or window start has passed | `its deadline arrived at <time>` |
| wait ended | a timed wait has passed | `the wait until <time> ended` |
| scheduled check | only `next_check` came up | `the check scheduled for <time> came up; nothing else changed` |

The check-in is told it may reach out once, saying which commitment, what
changed, and why now, and that it can never complete, snooze, or cancel a
commitment: only the user does that, in a conversation or through the API.

When the check ends, what it concluded is written on the record as
`last_check` (`triggered`, `unchanged` for a cancelled run, or `failed` with
`retryable: true` and the error) together with the run id, so a failed
evaluation stays inspectable on both the commitment and the activity record.
`next_check` moves to the earliest of the record's own next moment (a snooze
end, a timed wait, the deadline start) and, for a failure or a denied
reach-out, `now + 600`; a deadline with nothing later is therefore checked
exactly once. A failed check, or one whose reach-out was denied, keeps the
observation it was for as `last_check.pending_event`, so the next look still
states the event or the completed dependency as its condition instead of
"nothing else changed"; a check that goes through clears it.

A failed or cancelled check can be retried at once through
`POST .../activity/{id}/retry` (the Activity tab's Retry). The route hands a
commitment run to the evaluator (`commitment_evaluator::retry`): the loop
admits a linked attempt (`retry_of`, `attempt` + 1) and the evaluator runs it
the way a tick would, so the check-in runs with the same commitment and
condition and its `reach_out` still goes through `approve_side_effect`; the
route never leaves a commitment run behind for nobody to finish. The retry's
conclusion is written on the record like any other check and its
`next_check` replaces the backoff look, so a retried failure is checked
exactly once more. The retry is refused (`409`, nothing admitted, the record
keeps its backoff) for a run that is not retryable, a commitment that is
gone or closed, and while the companion is not onboarded or no background
model is configured; a retry while a check is already running is recorded
`skipped/duplicate` like any other. An explicit retry runs during quiet
hours or with the budget spent, since the user asked, but its message is
still denied then and the commitment is looked at again after the backoff.

Events are named observations. `machine_connected:<machine_id>` is observed
when a computer connects: every open commitment waiting on that event stops
waiting, notes `last_check: observed`, and asks for a check now.
Completing a commitment observes `commitment_completed:<id>` on every open
commitment that depended on it and changed status, so an unblocked
dependent is checked once with the completion as its stated condition. A
`user_reply` wait is cleared from the conversation (`commitment_update` with
`clear_wait`). An observation that arrives while a check is running is left
in place and gets a check of its own: the store compares, under its writer
lock, the observation on the record with the `last_check` the check was
admitted for, and refuses the check's conclusion (`superseded`) when they
differ, so the observation and the check it asks for stand.

### Chat tools

Six bounded tools, registered for conversations only (the check-in and
reflection routines never carry them), write through the same store as the
API: `commitment_create` (promise, owner, deadline or window, a wait on a
moment, a named event, or the user's reply, dependencies, `next_check`),
`commitment_update`, `commitment_complete` (refused without
`confirmed_by_user` or a `summary`, and a `run_id` must name an activity
record), `commitment_cancel`, `commitment_snooze` (`until`, in the future),
and `commitment_list` (`open` by default, `closed`, or `all`; at most 50).
Moments are RFC 3339 or a local date and time in the companion's timezone;
a date alone is the start of that day. Every write is broadcast as
`commitment_updated` like a write through the API.

## Migration hooks

#85 adds the commitment trigger and its evaluator; #82 adds the handoff
trigger.
