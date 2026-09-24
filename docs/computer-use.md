# Computer use

Nolune can see and act in the windows of two kinds of computer: the machine
the server runs on (the server-local target, #16) and a remote desktop that
runs the Nolune desktop app (a desktop target, #17). Both are driven by a
pinned [Cua Driver](https://github.com/trycua/cua) through the one typed
protocol in `cua-protocol/`, so one policy decides what the companion may do
on either kind of machine, and the companion reaches both through the same
four typed tools and the same loop rules ([Typed machine
tools](#typed-machine-tools-18)). This page is the whole of it: the privacy
model first, where computer use is supported, how the driver is pinned, each
kind of target, the permissions, how a computer is chosen, what the tools
enforce, and the manual release check.

## Privacy

Computer use is explicit and permissioned, on the server-local target and on
a remote desktop alike, and Nolune does not continuously record the screen.
What each of those means, and what the code enforces:

- Explicit. The companion sees or acts in a window only when it calls one of
  the typed machine tools in a turn of the agent loop, on the computer the
  user chose for that conversation ([Choosing a
  computer](#choosing-a-computer)). Nothing picks a computer for the user,
  nothing observes a window outside a tool call, and every call is one line
  in the activity trail ("observing a window on studio", "click on the
  server home") beside the message it belongs to. On the server machine
  every call is one run with a session that ends with it; on a desktop the
  app's overlay shows every action as it happens.
- Permissioned. Every action is authorized against the target's descriptor
  before it is sent: the Cua Driver's own Accessibility and Screen Recording
  grants (macOS attributes them to `CuaDriver.app`, see
  [Permissions](#permissions-20)), the capabilities those grants allow and
  the driver's health. A grant that is missing refuses the action with
  `permission_denied` before a frame is sent or a session opens, on both
  kinds of target, and a desktop checks the same descriptor again on its own
  side before its driver sees a request. Nothing is granted through Nolune:
  the grants are made by the user, in System Settings, to the driver's
  bundle, and the Computers page shows them as the driver reports them.
- One-shot screenshots, during an action only. The only image the companion
  ever gets is a one-shot screenshot of one window, taken by the driver
  during an action the model asked for: `get_window_state` with
  `include_screenshot: true`, which is off unless the model asks. It is the
  only capture there is. The image reaches the model once, beside the
  window's elements, and is saved among the companion's uploads so the user
  can open what the companion saw (the result names the upload and a
  link); it travels to the model provider the way any other image does, as
  a provider-reachable URL or inlined on a local install ([Typed machine
  tools](#typed-machine-tools-18)).
- Never continuous. Nolune does not continuously record the screen, on
  either kind of target: there is no continuous capture, no recording, no
  frame stream and no watcher, neither in the server nor in the desktop
  app, and neither runtime ever starts the driver's own recorder. The
  protocol names no recording or replay action, the tools offer none, a
  descriptor that claims one is refused at registration, and
  `scripts/tests/no-continuous-screen-recording.py` together with
  `server/tests/cua_privacy_docs.rs` fail the build when a Cua file or any
  shipped copy says otherwise.
- Background only, and verified. Every action is delivered in the
  background (`delivery_mode: background` is the only value the tools
  accept), so the companion never fronts a window or moves the user's
  focus, and an action is reported as done only once it is verified ([the
  loop](#the-loop-the-orchestrator-enforces)).
- A pinned driver. The Cua Driver behind all of this is one fixed release,
  verified by checksum and never updated on its own ([The Cua Driver
  pin](#the-cua-driver-pin)).

`server/test-support/cua_end_to_end.rs` walks every one of these through
the tool layer with a fake driver and a fake desktop, refusals included.

## Platform status

- macOS is supported, for the server machine and for the desktop app, with
  the two grants above. The manual [release check](#release-check) runs
  there.
- Linux and Windows are unsupported for now. `nolune cua install` installs
  the pinned driver on them all the same, so a later release can turn
  computer use on without a new download, but `nolune cua status` reports
  the host as unsupported, the server registers no target, the desktop app
  registers legacy-only, and its Settings window says there is nothing to
  install or grant there. `remote_bash` and `remote_files` work there as
  they always did.
- A headless host registers no target and never starts the driver, whatever
  the platform: a Linux session without `DISPLAY` or `WAYLAND_DISPLAY`, a
  macOS process outside an Aqua login (over SSH, as a daemon), a Windows
  session without `SESSIONNAME`, or a process inside a container. The server
  keeps serving, `list_machines` and the Computers page simply show no
  server-local entry, and a remote desktop can still register and be driven
  ([The server machine](#the-server-machine-16)).

## The Cua Driver pin

One Cua Driver release is what Nolune ships, verifies and accepts. The pin
lives in `cua_protocol::cua_driver_pin` (`cua-protocol/src/cua_driver_pin.rs`:
the version, the release tag and commit, and one asset name, sha256 and size
per target) with a shell-readable copy in `cua-protocol/cua-driver.pin` that
`scripts/cua-driver.sh` reads for the release workflow; a cua-protocol test
fails whenever the two disagree, so the pin moves in both places at once.

- One installer does the install, wherever it is started from
  (`cua_protocol::cua_driver_install`, behind the crate's `install`
  feature): the server's `nolune cua install` and the desktop app's
  **Install driver** button (#231) are the same code. It downloads the
  asset the pin names for this host, checks its size and sha256 before a
  byte is kept, asks the extracted binary for its version and refuses
  anything but the pin, then records the install under the workspace
  (`cua-driver/install.json`). A failure at any step leaves nothing
  installed.
- On macOS `cua-driver mcp` is a proxy to the `CuaDriver.app` daemon that
  owns the login session's socket, and it would start one *by name* through
  LaunchServices — whichever `CuaDriver.app` the system knows, which on a
  Mac that has only ever had Nolune's copy is none. So both runtimes bring
  the daemon up themselves, by path, from the bundle the located driver
  runs from, and only when none is running: `cua_protocol::cua_driver_daemon`,
  used by `nolune cua status` and by the desktop app's driver spawn
  (`ensure_daemon` in `desktop/src-tauri/src/cua_runtime.rs`). That is what
  lets the Install driver button work on its own: the driver it just
  installed is the daemon that answers, and macOS attributes Accessibility
  and Screen Recording to that bundle.
- A login session has one daemon, so another `CuaDriver.app` that is
  already running one (the upstream installer's `/Applications` copy, say)
  keeps answering whatever Nolune installed, and one of another release
  refuses Nolune's driver outright (`incompatible daemon: contract version
  … does not match SDK …`). Nothing starts or stops on its own over this.
  The settings window names that daemon, and its button (`Use Nolune's
  driver` then, `Install driver` otherwise) installs as before, stops the
  foreign daemon with that daemon's own `stop`, and starts Nolune's by
  path (`take_over_daemon` in `desktop/src-tauri/src/cua_runtime.rs`).
  `nolune cua status` names it and prints the `stop` to run.
- Which entry point to point a user at follows from who they are. A server
  host has a shell and `nolune` on its `PATH`, so it runs
  `nolune cua install`. A desktop user has neither: the app's in-app server
  install (#128) puts the binary in `~/.nolune/bin` without touching
  `PATH`, and a desktop bound to a server elsewhere has no binary at all.
  So every surface that can name a desktop — the Computers tab, the
  handoff checks, the `no_cua_driver` refusal, the companion's own prompt —
  answers a missing driver with Settings › Computer use › Install driver,
  and never with a command. Tests in `desktop/tests/cua-permissions.test.mjs`
  and `desktop/src-tauri/src/cua_permissions.rs` fail if any sentence the
  settings window can show names `nolune cua`.
- `nolune cua status` prints the pin, the installed driver checked against
  it (verified, not the pinned version, checksum mismatch, binary missing),
  the driver the server would run, and the driver's own health and grants.
- The driver is never updated on its own: not by Nolune, and not through
  the driver's self-updater, which nothing in Nolune ever invokes. A driver
  that reports any other version is incompatible, named as such with the
  install command, and never fixed silently. A new pin ships with a new
  Nolune release (`docs/release-checklist.md`), whose workflow fetches
  and verifies the driver archive for every desktop target and carries
  it beside the desktop bundle; `nolune cua install` then installs that
  one, on the server machine and on a desktop alike.

## The server machine (#16)

The server machine itself can be one of the computers the companion uses,
without the desktop app. The server drives it through a local `cua-driver`
process and registers it as a machine target beside the desktops connected
through the app, under the reserved `server-local:` prefix. To the
composer, the prompt, the tool results and the trail it is "the server
home", whatever its hostname: the place `run_command` and the file tools
already act.

### When the target exists

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

### While it runs

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

### Identity

The target registers as `server-local:<hostname>`, the hostname reduced to
the protocol's identifier grammar (letters, digits, `-`, `_`, `.`), at most
128 bytes. Desktops register under a UUID they persist (or under the bare
hostname before #80), so the server and a desktop app on the same physical
machine never share an id, and a request that names one of them can never
mean the other. A desktop registration that claims the `server-local:`
prefix does not shadow the real target in the listing, and a desktop Cua
descriptor under that prefix is refused outright (below), so the target
registers whether the desktop connected before or after it.

### How it appears

- `list_machines` (the companion's tool) lists it with
  `location: "server_local"`, its `os`, `driver_version`, `health`,
  `permissions` and `capabilities`.
- `GET /api/instances/companion/machines` and the Computers page list it as
  an online row with the same fields, `driver_version` and `cua_health`
  filled in, and the hostname as its name. It is live state, not a record:
  nothing about it is written to `machines.json`, it disappears when the
  driver stops, and it cannot be renamed or forgotten like a desktop record.

### Sessions

Every piece of work on the target is one run. The first action of a run
calls the driver's `start_session` with the run's label (`nolune-run-<n>`),
every later action of that run carries the same label, and `end_session` is
called when the run completes, fails, or exceeds `run_timeout_secs`. A run
that executes nothing opens nothing. On shutdown the gateway ends every
session that is still open, unregisters the target, and stops the driver
child before connections drain. Runs cannot open or close sessions
themselves; the runtime refuses `start_session` and `end_session` from a
run so cleanup stays deterministic.

### Policy

Every action is authorized against the advertised descriptor before it is
sent: an action whose capability the target does not advertise, whose
permission is not granted, or that targets an unavailable machine is refused
without opening a session or reaching the driver. Accessibility denied, for
example, drops pointer, keyboard, window management, element value, menu and
verification capabilities, and a click is refused while listing apps still
works. Driver answers pass through the same size, depth, shape and
correlation checks as any remote target's answers.

Computer use here is explicit and bounded ([Privacy](#privacy)): the
companion takes a one-shot screenshot of one window when it asks for one
(`get_window_state` with `include_screenshot`), inside a session that ends
with the run. There is no continuous capture, no recording, and no
observation outside a run.

## Desktop targets (#17)

A desktop app with its own Cua driver is the other kind of target. It
registers over the authenticated machine WebSocket
(`/api/agents/ws/machine`) exactly as before, with one more field on the
`register` message:

```json
{"type": "register", "machine_id": "<stable id>", "os": "macos", "hostname": "studio",
 "capabilities": ["bash", "file_read", "file_write", "file_list", "upload_file"],
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
computer: `remote_bash` and `remote_files` work as they always did, it never
sees a typed frame, and the companion cannot see or act in its windows (see
[Typed machine tools](#typed-machine-tools-18) and
[What the desktop app executes](#what-the-desktop-app-executes-19)). The ack
`{"type": "registered", "machine_id": ..., "cua": true|false}` says which.

A registered descriptor makes the desktop a Cua target under its own id,
beside the server-local one: `list_machines` lists it with
`location: "desktop"` and the driver's `driver_version`, `health`,
`permissions` and `capabilities` (the legacy entry with `hostname` and
`last_seen` stays), and its row in `GET /api/instances/companion/machines`
carries `driver_version` and `cua_health` while it is connected; clients
hear that row as `machine_updated` once the target is attached, after the
one the registration itself announces. The `permissions` the record keeps,
and the row reports, are the descriptor's: the driver's own grants, which
are the ones that decide what it can see and do. A desktop without a
descriptor records none. The desktop app sends no grants of its own since
#19, and a `permissions` field an older desktop still sends on the
registration is ignored.
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

## Permissions (#20)

Computer use on macOS needs two grants, Accessibility (pointer, keyboard,
reading windows) and Screen Recording (window snapshots), and macOS
attributes each grant to the process that asks for it. The Cua Driver runs
from its own app bundle (`CuaDriver.app`, `com.trycua.driver`), so the
grants belong to that bundle: not to the Nolune server, and not to the
desktop app. Both runtimes therefore read the state from the driver itself.
Its health report carries `tcc_accessibility` and `tcc_screen_recording`
under the driver's bundle identity, and `driver_mcp::permissions_from_health`
maps them to `granted`, `denied` or `prompt_required` (never asked). Those
are the grants the machine record keeps, the Computers tab shows, and a
handoff or the resume ritual checks a destination on (below). The desktop
app's own grants (`AXIsProcessTrusted`, `CGPreflightScreenCaptureAccess`)
are shown in its Settings window only: since #19 nothing inside the app
uses them (see [What the desktop app executes](#what-the-desktop-app-executes-19)),
they say nothing about what the driver can do, and they are not reported
to the companion.

The desktop app's Settings window (`desktop/src/routes/settings/+page.svelte`,
fed by the `cua_permissions` command in
`desktop/src-tauri/src/cua_permissions.rs`) shows that state in one place:

- The host. macOS is supported. Linux and Windows are named as unsupported,
  with the note that there is nothing to install or grant there and that a
  later release can turn computer use on without moving the pin. A session
  without a display (an SSH or background login on macOS, `DISPLAY` and
  `WAYLAND_DISPLAY` unset on Linux, no `SESSIONNAME` on Windows) is named
  as headless: the driver is never started and headless installs need
  nothing from the page. Neither offers the install button.
- The workspace install (`cua-driver/install.json`) checked against the
  pin: pinned, stale, a binary that is gone, or none, each pointing at the
  **Install driver** button below it.
- That button (the `cua_install_driver` command in
  `desktop/src-tauri/src/cua_install.rs`) runs the shared installer into
  this computer's workspace and narrates each step on the
  `cua-install-progress` event, so a 70 MB download is not a silent
  minute. A stale, broken or unreadable install is reinstalled over;
  a first install is not forced. One install runs at a time. When it
  finishes it does two things the install would otherwise be invisible
  without. It lets go of the driver child that is running
  (`CuaRuntime::replace_driver`, which ends the sessions that child held
  and closes it, without stopping the runtime), because that child is the
  driver from before the install — after a reinstall over a stale version,
  exactly the version being replaced. Then it asks the machine socket to
  register this computer again (`computer_use_bridge::reannounce`, which
  closes the socket so the retry loop opens another), because the Cua
  descriptor is built once per connection and the companion would
  otherwise keep seeing a computer with no driver until the next
  reconnect. Nothing is ever updated on its own: an install happens
  because the button was pressed.
- The driver the app runs, asked for its own report through the same
  runtime the machine socket uses (`CuaRuntime::probe` starts the driver
  when none runs and asks the one that does again, so a grant made since
  shows). The probe is a read, never a reset: a re-read that fails is shown
  on the page and the driver the socket registered stays up for the
  server's requests. Its version is checked against the pin: a mismatch is
  the headline, pointing at **Install driver**, and nothing is granted
  through a driver that is not the pinned one. The bundle its report names
  is checked the same way: the grants are attributed to that bundle, and a
  driver (from `NOLUNE_CUA_DRIVER` or `PATH`) that holds them as anything
  but `com.trycua.driver` is named as such, pointing at the same button and
  with nothing to grant through it. Its health and every failed check come with
  the driver's own hints. A driver that cannot report shows what it said on
  stderr, and a Retry.
- The two rows, Accessibility and Screen recording, each Granted, Denied,
  Not asked yet or Unavailable as the driver's bundle holds them.

Grant runs the driver's own flow, `cua-driver permissions grant`, detached
and without a terminal: the driver launches its app through LaunchServices
so the macOS prompts and the pane entries name CuaDriver, then asks for the
grants. The app also opens the System Settings pane for that permission,
where CuaDriver is enabled when no prompt appears (macOS does not prompt
again after a denial). The status is re-read a few seconds later and on
Refresh. The pure views in `desktop/src/lib/cua-permissions.js` hold the
copy (`desktop/tests/cua-permissions.test.mjs`).

The wording is deliberate, on the page and here: every screenshot is a
one-shot window snapshot taken during an action the user asked for, inside
a session that ends with the run. There is no continuous capture, nothing is
recorded, and nothing in the onboarding starts the driver's own recorder;
`scripts/tests/no-continuous-screen-recording.py` keeps the module to
`permissions grant`, checks the copy, and checks this section. Nolune never
updates the driver on its own: a wrong version is reported with the install
command, never fixed silently.

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

Every machine tool (`remote_bash`, `remote_files`, and the typed tools
below) acts on the computer the user chose for the conversation (#80),
never on one the model picked between several. The composer's computer
selector lists the server home and every known desktop with its state
word; the choice is remembered per conversation in the browser and travels
with each message as `machine_id` on `POST /api/chat`: a known machine's
stable id, `server-home` (the synthesized home row, or any `server-local:`
id) or nothing. An id that is not shaped like a registered one
(`validate_machine_id`) is refused with 400 before it reaches the prompt or
the log. Until the browser's first listing of the computers arrives, a
remembered choice is sent as it is, so a computer that turns out to be
offline is refused by name rather than replaced by the only connected one;
a computer a listing no longer has reads as no choice. The agent loop
resolves the choice once per turn, states it in the turn context at the
head of the user's message (never the system prompt, which stays a stable
cached prefix; see [providers.md](providers.md#prompt-cache)), and builds the
tools around it.

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
  `remote_bash` and `remote_files` answer `server_home` until a desktop is
  chosen. The typed machine tools (below) drive its Cua target instead,
  when the server machine has one.
- The trail names the computer: each machine tool's activity entry reads
  "<action> on <name>" (the user's name for it, else its hostname; the
  server machine is "the server home" whether the user chose it or the
  model named its `server-local:` id; with nothing chosen, the only
  desktop connected when the turn started for `remote_bash` and
  `remote_files`, and the only computer with a Cua driver for the typed
  tools, which may be the server home; with a computer chosen, that
  computer whatever `machine_id` the model passed, since the call acts
  there or is refused), the
  chat bar shows "working on <name>" while the companion works, and an
  Activity run that acted on a computer says "On <name>". The line is
  persisted beside the tool call in the conversation's history (`tool_trail`,
  by tool-call id; the model replays only the message itself), so the trail
  reads the same after a reload, and a call recorded without one still names
  the computer its arguments name. "on the connected computer" appears only
  while the choice is genuinely open (none or several to choose from),
  where the call itself is refused.
- A handoff continued on a computer (#82) targets that computer: an
  acceptance that starts the conversation's loop starts it with the
  destination, and one queued on a conversation that is already running
  queues the destination as the computer its next turn acts on (the loop
  keeps the computer it started with only for the turn in progress, and a
  queued target it never took is released with the conversation). A new
  message from the composer starts a new run with the composer's own choice.

## What the desktop app executes (#19)

The desktop app has no screen path of its own. The `enigo` pointer and
keyboard automation, the `screenshots` capture with its own scaling and
scale cache, the `computer_*` Tauri commands and the browser-side bridge
that answered the coordinate `computer_use` tool are deleted; the app links
none of those crates. Every window action, and every capture, is a typed
Cua frame the driver answers (the sections above), and a desktop without a
driver cannot see or act in a window at all.

The toolcalls the app still executes over the machine socket are the shell
and file ones behind `remote_bash` and `remote_files`: `bash`, `file_read`,
`file_write`, `file_list` and the `upload_file` that hands a file to the
companion. They are the `capabilities` the registration reports, they run
off the main thread under the connection's work permit and are cancelled
with the socket, and their `action_result` carries the output (or the
failure) in the `error` field as it always did. The server no longer reads
an image, a size or a scale from a toolcall's result, needs no desktop
permission for shell or file work (the typed tools' permission checks are
the orchestrator's, against the Cua descriptor), and neither the
registration nor the machine record carries a screen size: the old one came
from the `screenshots` crate, and a `machines.json` written before #19 is
read with its `screen_width` and `screen_height` dropped (see
[companion-storage.md](companion-storage.md#known-machines)).

The overlay still hears every action on the same events (`computer-use-action`
for each shell, file or typed action, `computer-use-idle` when the socket
closes) and hides only for the window snapshot a typed `get_window_state`
takes, the one capture the desktop takes part in. A driver on the desktop
machine is what turns window actions on there — its Settings window's
**Install driver** button, or a `cua-driver` on its `PATH`; the Computers
tab says so for a desktop without a driver.

What a computer must offer to continue a task there follows from this
(`server/src/domain/handoff.rs`, the checks behind a handoff card's
preview and acceptance and the resume ritual's choice of destination, see
[companion-storage.md](companion-storage.md#handoff-cards)): a
Cua driver (`driver_missing` otherwise; `driver_unavailable` when its
health says so, `driver_degraded` as a note), that driver's Screen
Recording and Accessibility grants (`permission_denied`, `permission_prompt`
when its report did not cover one), and the `file_read` and `file_list`
toolcalls when the task links a file on a computer (`capability_missing`).
No coordinate action name is required of a destination: the desktop app
advertises none, and a desktop that predates #19 advertising them is no
more able to see or act in a window than one that does not.

## Typed machine tools (#18)

Four tools drive any Cua target, the server machine or a desktop with a
driver, through one orchestrator per chat turn
(`server/src/services/cua/orchestrator.rs`, tools in
`server/src/services/tools/cua.rs`). `list_machines` shows one entry per
machine with its `location`, `driver_version`, `health`, `permissions` and
`capabilities`; the ones with `driver_version` are the ones these tools
reach. They are the only way the companion sees or acts in a window: the
coordinate `computer_use` tool is no longer offered to the model (#18) and
is deleted with the legacy desktop executor (#19), and the system prompt
states the loop below rule by rule.

- `discover_windows` — `list_apps`, `list_windows` (optionally one pid) or
  `launch_app` (by bundle id or name). Every window comes back with the
  `pid` and `window_id` the other three tools take as `target`.
- `get_window_state` — observe one window: the `snapshot_id`, the
  accessibility elements as a table (`element_columns` names the columns:
  `element_index`, `element_token`, role, label, value, enabled, selected,
  frame, actions; one array per element; `query` narrows large trees), the
  driver's degraded flags and background-input routes, and
  `pixel_addresses`, which says whether a point address is allowed on that
  window right now. The output stays under the tool-result bound whatever
  the window holds: labels and values are clipped, and elements past the
  bound are counted rather than shown, so the snapshot id and the pixel
  policy always reach the model. A screenshot is captured only when asked
  for (`include_screenshot`) and is then shown to the model as an image
  beside the text, through the path every other image takes: saved among
  the companion's uploads and referenced by a provider URL carrying its
  provenance when `public_url` is provider-reachable, inlined within the
  provider's bound on a local install (the result then carries the image
  and the text as separate blocks; the tool-result bound holds the text
  blocks together under the one bound a plain result has and never cuts
  through an image). The result names the upload and a link the user can
  open; the bytes never appear in the text.
- `act` — one typed action in a window: `click`, `double_click`,
  `right_click`, `drag`, `scroll`, `type_text`, `press_key`, `hotkey`,
  `set_value` or `invoke_menu`, addressed by `element_token` (preferred),
  `element_index` + `snapshot_id`, or a window-local `point`. `verify`
  carries the predicates that must hold afterwards.
- `verify_state` — the predicates alone: `window_exists`, `window_bounds`,
  or an element (by role and/or `label_contains`) that exists, is enabled or
  selected, or has a value.

### Which computer

The tools act on the computer the user chose ([above](#choosing-a-computer)),
resolved to its Cua target: a chosen desktop must have registered a driver
(`no_cua_driver` names it otherwise), the server home means the server-local
target (`no_server_local_target` when the server machine has none), and with
nothing chosen the only registered target is used while several are refused
with `choose_a_computer`, naming them, even when the model names one. A
target whose driver reports the machine unavailable is refused with
`driver_unavailable`; a call that names another computer than the chosen one
with `target_mismatch`; no target at all with `no_cua_target`. On the server
machine every tool call is one run of the runtime, sessioned and ended with
it ([Sessions](#sessions)), and the results, refusals and trail call it
the server home; a desktop's own driver keeps its session and is named
by the user's name for it, else its hostname.

### The loop the orchestrator enforces

1. Observe before acting. An element action needs a prior `get_window_state`
   of that window on that machine (`snapshot_required`), one that carried an
   accessibility tree (`no_accessibility_tree`).
2. Tokens are bound to their snapshot. Only tokens and indexes the latest
   snapshot of the window issued are forwarded; a token from a snapshot a
   newer one replaced, or one no snapshot issued, is refused with
   `stale_snapshot` before anything is sent. The driver's own stale verdict
   is believed too: it drops the window from the ledger. Snapshots are kept
   per machine and window, so a token never crosses to another window or
   computer.
3. One snapshot, one action. After an action reaches the driver the
   window's observation is consumed (`snapshot_consumed`): call
   `get_window_state` again before the next action there. Verifying does
   not consume it.
4. Pixels are the fallback, not the default. A point address (and a drag)
   is refused with `pixel_refused` unless the window's latest observation
   was degraded, its accessibility surface unresolved or empty, or the last
   verification on it was not satisfied; a verified action closes the pixel
   route again. An empty tree counts only when nothing narrowed the walk: a
   `query` that matches nothing, or a `max_depth` above every actionable
   element, is a filter on a healthy window. The coordinates come from the
   latest observation, so it is required for point addresses as well, and it
   must have captured a screenshot (`include_screenshot: true`): with none
   on record the point is refused too, and `pixel_addresses` says so. A
   verification on a window that was never observed puts nothing on record
   either way.
5. Verify after every action. The orchestrator reads the driver's outcome
   and, when `verify.expect` was given, issues `verify_state` itself right
   after the action. `act` succeeds only for a verified outcome: the
   predicates satisfied, or, without predicates, the driver confirming the
   effect by reading it back (accessibility, window, snapshot or screenshot
   evidence; a delivery receipt alone is delivery, not verification).
   `refused` outcomes are `action_refused`; `unverifiable`, `suspected_noop`
   and `partial` outcomes without satisfied predicates, and a verification
   the driver could not evaluate, are `unverified`; unsatisfied predicates
   are `verification_failed`. A failed verification, a refused delivery, a
   suspected no-op or the driver's own advice to go through pixels opens the
   pixel route on that window; an action that was delivered but not read
   back does not, because nothing showed it missing.
6. Background only. Every action is delivered with `delivery_mode:
   background`, the only value the tools accept; nothing is fronted or
   focused. When the driver recommends escalating, to foreground control
   or anything else, the recommendation is quoted in the refusal (or, for
   an observation, in its `escalation` field) and never applied.
7. The target's say comes first: a capability the descriptor does not
   advertise or a permission the driver does not hold refuses the action
   (`permission_denied`, `action_refused`) before the ledger is consulted
   and before anything is sent; the driver's own errors keep their code
   (`stale_snapshot`, `timeout`, ...).

## Release check

The scenarios above run on every CI platform against a fake driver and a
fake desktop (`server/test-support/cua_end_to_end.rs`), which proves the
policy but not a real window. Before a release, and after the Cua Driver pin
moves, a macOS machine with a graphical session walks the same steps against
the real driver with `scripts/release-check-computer-use.sh`: it checks the
prerequisites (macOS, an Aqua login, a `nolune` binary, `nolune cua status`
reporting the pinned driver healthy with both grants) and exits non-zero when
one is missing, then prints what to verify, in order: `list_machines` shows
the server machine, `get_window_state` on a background window keeps it in
the background, a verified click, a stale element token refused, a denied
permission refused before the driver, and disconnect cleanup. It runs
nothing on the user's behalf beyond `nolune cua status`; every observation
and action goes through the companion, in a conversation, the way a user's
would. `docs/release-checklist.md` names it beside the pin.

## Related

- [`docs/companion-storage.md`](companion-storage.md) — the known machines
  file the desktop records live in.
- `cua-protocol/` — the machine protocol, `CheckedCuaAdapter`, and the driver
  wire mapping in `driver_mcp`.
- `server/src/services/cua/` — discovery, host probe, transport, runtime and
  sessions; `desktop.rs` is the typed-frame link a desktop target answers
  through; `orchestrator.rs` is the loop policy the typed machine tools
  (`server/src/services/tools/cua.rs`) enforce on every target.
- `server/test-support/cua_end_to_end.rs` — the end-to-end scenarios, one
  per promise above, with their refusals; `server/tests/cua_privacy_docs.rs`
  and `scripts/tests/no-continuous-screen-recording.py` keep this page,
  the README, the settings page and every Cua file to the privacy model;
  `scripts/release-check-computer-use.sh` is the manual macOS check.
