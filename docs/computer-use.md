# Computer use on the server machine (#16)

The machine the Nolune server runs on can be one of the computers the
companion uses, without the desktop app. The server drives it through a
local [Cua Driver](https://github.com/trycua/cua) process and registers it as
a machine target beside the desktops connected through the app. Everything
goes through the shared typed protocol in `cua-protocol/`, the same boundary
a desktop target uses (see [Desktop targets](#desktop-targets-17)), so one
policy decides what the companion may do on either kind of machine.

## When the target exists

At startup the gateway looks for a driver and a desktop session, in this
order (the same facts `nolune cua status` prints, see [the `nolune`
command](../README.md#the-nolune-command)):

1. `[cua].enabled = false` in `config.toml` never looks for a driver.
2. `[cua].driver_path`, then the `NOLUNE_CUA_DRIVER` environment variable,
   then the driver `nolune cua install` put under the workspace
   (`cua-driver/install.json`), then an executable `cua-driver` on `PATH`.
   A path that is named explicitly but cannot run is logged as an error; it
   is never treated as "no driver", so a typo does not silently remove the
   target.
3. Nolune drives computers on macOS only for now: on Linux and Windows the
   pinned driver installs but the server registers no target until a
   release turns it on.
4. A process inside a container (`/.dockerenv`, `/run/.containerenv`,
   `container=`, `KUBERNETES_SERVICE_HOST`) has no desktop session.
5. A host without a graphical session is headless: a Linux session without
   `DISPLAY` or `WAYLAND_DISPLAY`, a macOS process outside an Aqua login
   (`launchctl managername` answers `Background` or `System` over SSH and
   for daemons), a Windows session without `SESSIONNAME`.
6. No driver anywhere means no target.

On a headless or unsupported host the server logs one line saying why,
registers nothing, and stays healthy: `list_machines` and the Computers page
simply show no server-local entry, and nothing claims GUI control the host
does not have. Where the session cannot be checked (macOS when `launchctl`
cannot be asked) the driver's own health report decides.

When a driver is found the server spawns one persistent `cua-driver mcp`
child, completes the MCP handshake within `handshake_timeout_secs`, and asks
it for a health report. The report becomes the descriptor the target
advertises: platform, driver version, health (`healthy`, `degraded` when a
permission is missing, `unavailable` when a core check failed), the
accessibility and screen-capture permissions as the driver sees them, and
the capabilities those permissions allow. A driver that does not answer the
handshake or the health report is stopped and reported; the server runs on
without a server-local target.

The driver starts in the background after the gateway has bound its port and
printed `nolune: ready`, so serving never waits on the handshake or the
health report: a driver that stalls (a wrapper script, a process blocked on a
permission prompt) costs nothing but its own deadline, `/healthz` answers at
once, and the target appears in the listings the moment it is registered.

## While it runs

The advertised descriptor stays honest for as long as the driver does:

- The server watches the child. If it exits or crashes, the target is
  unregistered at once, every session it held is lost with it (there is
  nobody left to end them), and one error line says so. The row is gone
  from `list_machines` and the Computers page; restart the gateway to
  register it again.
- Every `health_interval_secs` the server asks the running driver for a
  fresh health report. A permission granted or revoked after startup changes
  the health, permissions and capabilities the target advertises, and the
  next run authorizes against the new descriptor; a report that cannot be
  read keeps the last one. Only an exit drops the target.

## Identity

The target registers as `server-local:<hostname>`, the hostname reduced to
the protocol's identifier grammar (letters, digits, `-`, `_`, `.`), at most
128 bytes. Desktops register under a UUID they persist (or under the bare
hostname before #80), so the server and a desktop app on the same physical
machine never share an id, and a request that names one of them can never
mean the other. A desktop registration that claims the `server-local:`
prefix does not shadow the real target in the listing, and a desktop Cua
descriptor under that prefix is refused outright (below), so the target
registers whether the desktop connected before or after it.

## How it appears

- `list_machines` (the companion's tool) lists it with
  `location: "server_local"`, its `os`, `driver_version`, `health`,
  `permissions` and `capabilities`.
- `GET /api/instances/companion/machines` and the Computers page list it as
  an online row with the same fields, `driver_version` and `cua_health`
  filled in, and the hostname as its name. It is live state, not a record:
  nothing about it is written to `machines.json`, it disappears when the
  driver stops, and it cannot be renamed or forgotten like a desktop record.

## Sessions

Every piece of work on the target is one run. The first action of a run
calls the driver's `start_session` with the run's label (`nolune-run-<n>`),
every later action of that run carries the same label, and `end_session` is
called when the run completes, fails, or exceeds `run_timeout_secs`. A run
that executes nothing opens nothing. On shutdown the gateway ends every
session that is still open, unregisters the target, and stops the driver
child before connections drain. Runs cannot open or close sessions
themselves; the runtime refuses `start_session` and `end_session` from a
run so cleanup stays deterministic.

## Policy

Every action is authorized against the advertised descriptor before it is
sent: an action whose capability the target does not advertise, whose
permission is not granted, or that targets an unavailable machine is refused
without opening a session or reaching the driver. Accessibility denied, for
example, drops pointer, keyboard, window management, element value, menu and
verification capabilities, and a click is refused while listing apps still
works. Driver answers pass through the same size, depth, shape and
correlation checks as any remote target's answers.

Computer use here is explicit and bounded: the companion takes a one-shot
screenshot as part of an action it was asked to perform, inside a session
that ends with the run. There is no continuous capture, no recording, and no
observation outside a run.

## Desktop targets (#17)

A desktop app with its own Cua driver is the other kind of target. It
registers over the authenticated machine WebSocket
(`/api/agents/ws/machine`) exactly as before, with one more field on the
`register` message:

```json
{"type": "register", "machine_id": "<stable id>", "os": "macos", "hostname": "studio",
 "screen_width": 1440, "screen_height": 900, "permissions": {...}, "capabilities": [...],
 "cua": {"version": "v1", "machine": {"machine_id": "<stable id>", "location": "desktop",
         "platform": "macos", "driver_version": "0.28.2", "health": "healthy",
         "permissions": {"accessibility": "granted", "screen_capture": "granted"},
         "capabilities": ["app_discovery", "pointer", ...]}}}
```

`cua` is a `CuaRegistrationEnvelope`, decoded through the protocol's bounds.
It is accepted only when its `machine_id` is the id the socket registered
as, its `location` is `desktop`, and the id is not under the reserved
`server-local:` prefix; anything else refuses the whole registration with
`{"type": "error", "error": "invalid_cua_registration"}`, like an unusable
machine id. The prefix is refused whether or not the server-local target
has registered yet: it registers in the background after the listener is
up, and a desktop that took its id first would block it for the life of
the process. Without the field the desktop is a legacy-only
computer: `remote_bash`, `remote_files` and coordinate `computer_use` work
as they always did and it never sees a typed frame. The ack
`{"type": "registered", "machine_id": ..., "cua": true|false}` says which.

A registered descriptor makes the desktop a Cua target under its own id,
beside the server-local one: `list_machines` lists it with
`location: "desktop"` and the driver's `driver_version`, `health`,
`permissions` and `capabilities` (the legacy entry with `hostname` and
`screen` stays), and its row in `GET /api/instances/companion/machines`
carries `driver_version` and `cua_health` while it is connected; clients
hear that row as `machine_updated` once the target is attached, after the
one the registration itself announces. When the registration reports no
`permissions`, the descriptor's are recorded.
A desktop reconnecting under its stable id replaces its target; it never
becomes a second one. The ack's `"cua": false` after a descriptor was sent
means the socket was replaced between the legacy registration and the
typed one (logged); the legacy registration stands.

Every authorized request is one frame on the desktop's socket, the envelope
carried whole so the desktop decodes it with `CuaRequestEnvelope::from_json`
and authorizes it against its own allowlist before touching its driver:

```json
{"type": "cua_request", "request": {"version": "v1", "request_id": "nolune-run-3-1",
 "machine_id": "<stable id>", "action": {"tool": "click", "args": {...}}}}
```

The desktop answers with the `CuaResponseEnvelope` whole, beside the legacy
`action_result` messages:

```json
{"type": "cua_response", "response": {"version": "v1", "request_id": "nolune-run-3-1",
 "machine_id": "<stable id>", "action": "click", "response": {"status": "success", "result": {...}}}}
```

The answer is decoded through the same size, depth and shape checks as a
driver's, matched to the waiting call by `request_id`, and checked against
the request it answers (`validate_response_for`): an answer for another
request is dropped, and an answer with the right id but another action, or
one the protocol cannot read, fails the call as a `driver_failure` at once.
A desktop that does not answer within `call_timeout_secs` fails the call as
a retryable `timeout`. Structured window state, action outcomes and
verification results pass through unchanged.

Sessions the desktop confirms open for the server (`start_session` answered
`active`) are remembered per socket and forgotten when it ends them. When
the socket closes, every call still waiting fails at once as a retryable
`runtime_unavailable` instead of at its deadline, the sessions it held are
lost with it (the desktop ends them on its side; there is nobody left to
ask), and the target leaves the listing; the desktop record stays, offline,
with `driver_version` and `cua_health` back to `null`.

On the desktop side (`desktop/src-tauri/src/cua_runtime.rs`) the app runs
one persistent `cua-driver mcp` child for its whole lifetime: the driver
named by `NOLUNE_CUA_DRIVER`, else the one `nolune cua install` recorded in
`<NOLUNE_HOME>/cua-driver/install.json`, else `cua-driver` on `PATH`; with
none of them the app registers legacy-only and says so in its log. The
child is started (handshake and call deadlines as on the server) and asked
for its health report before the socket opens, and the descriptor that
report yields is what the `cua` field carries; a reconnect re-reads the
health of the same child rather than starting a second one, a child that
exits on its own is restarted on the next request, and quitting the app
ends the open sessions and kills it: the stop is terminal (nothing starts
again, and a driver still in its handshake or its first report lets go at
once) and a stop the exit grace cuts short still ends with the child
killed and waited for. Every inbound `cua_request` is decoded
with `CuaRequestEnvelope::from_json` and authorized against that descriptor
before the driver sees it: a tool the protocol does not name, a request for
another machine, or an action whose capability or permission the descriptor
lacks is answered with a `capability_denied` error without touching the
driver, whatever the server asked. Admitted requests go through the same
`driver_mcp` mapping the server uses, and the driver's structured result is
forwarded unchanged. When the socket closes, the desktop sends `end_session`
for every session it confirmed open for the server, one at a time, and
forgets each only once the driver answered.

## Configuration

```toml
[cua]
enabled = true                 # false never looks for a driver
driver_path = ""               # empty: NOLUNE_CUA_DRIVER, the `nolune cua install` driver, then cua-driver on PATH
handshake_timeout_secs = 10    # MCP handshake at startup
call_timeout_secs = 30         # one driver call; a slow driver is cancelled, not waited on
run_timeout_secs = 900         # one run's session; ended and reported as timed out after this
health_interval_secs = 60      # how often the running driver is asked for a fresh health report
```

Unknown keys in `[cua]` are refused at load. A zero timeout keeps the
default. The environment variable `NOLUNE_CUA_DRIVER` names the driver
binary when `driver_path` is empty; with neither, the driver `nolune cua
install` verified against the pin is used, and `cua-driver` on `PATH` last.

## Choosing a computer

Every desktop tool (`computer_use`, `remote_bash`, `remote_files`) acts on
the computer the user chose for the conversation (#80), never on one the
model picked between several. The composer's computer selector lists the
server home and every known desktop with its state word; the choice is
remembered per conversation in the browser and travels with each message as
`machine_id` on `POST /api/chat`: a known machine's stable id, `server-home`
(the synthesized home row, or any `server-local:` id) or nothing. An id that
is not shaped like a registered one (`validate_machine_id`) is refused with
400 before it reaches the prompt or the log. Until the browser's first
listing of the computers arrives, a remembered choice is sent as it is, so
a computer that turns out to be offline is refused by name rather than
replaced by the only connected one; a computer a listing no longer has
reads as no choice. The agent loop resolves the choice once per turn,
states it in the system prompt, and builds the tools around it.

- Nothing chosen: the only connected desktop is used. With several connected
  the tools refuse with `choose_a_computer`, naming them, and the companion
  asks the user to choose in the composer. The model naming one of them is
  not the user choosing it.
- A chosen desktop is used exactly: a call that names another computer is
  refused with `target_mismatch`, and a chosen computer that is offline
  (`machine_unavailable`), has not answered for more than 45 s
  (`machine_unhealthy`) or lacks the permission the action needs
  (`permission_denied`: Accessibility for pointer and keyboard actions,
  Screen recording for screenshots; `denied` and `not asked yet` refuse,
  `unavailable` means the platform cannot report) fails with what to do
  there. No refusal ever falls back to another computer.
- The server home is where `run_command` and the file tools already act, so
  the desktop tools answer `server_home` until a desktop is chosen. Driving
  the server-local Cua target through typed machine tools is #18.
- The trail names the computer: each desktop tool's activity entry reads
  "<action> on <name>" (the user's name for it, else its hostname; with
  nothing chosen, the only desktop connected when the turn started; with a
  computer chosen, that computer whatever `machine_id` the model passed,
  since the call acts there or is refused), the
  chat bar shows "working on <name>" while the companion works, and an
  Activity run that acted on a computer says "On <name>". The line is
  persisted beside the tool call in the conversation's history (`tool_trail`,
  by tool-call id; the model replays only the message itself), so the trail
  reads the same after a reload, and a call recorded without one still names
  the computer its arguments name. "on the connected computer" appears only
  while the choice is genuinely open (none or several connected), where the
  call itself is refused.
- A handoff continued on a computer (#82) targets that computer: an
  acceptance that starts the conversation's loop starts it with the
  destination, and one queued on a conversation that is already running
  queues the destination as the computer its next turn acts on (the loop
  keeps the computer it started with only for the turn in progress, and a
  queued target it never took is released with the conversation). A new
  message from the composer starts a new run with the composer's own choice.

## Related

- [`docs/companion-storage.md`](companion-storage.md) — the known machines
  file the desktop records live in.
- `cua-protocol/` — the machine protocol, `CheckedCuaAdapter`, and the driver
  wire mapping in `driver_mcp`.
- `server/src/services/cua/` — discovery, host probe, transport, runtime and
  sessions; `desktop.rs` is the typed-frame link a desktop target answers
  through.
