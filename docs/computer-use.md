# Computer use on the server machine (#16)

The machine the Nolune server runs on can be one of the computers the
companion uses, without the desktop app. The server drives it through a
local [Cua Driver](https://github.com/trycua/cua) process and registers it as
a machine target beside the desktops connected through the app. Everything
goes through the shared typed protocol in `cua-protocol/`, the same boundary
a desktop target will use, so one policy decides what the companion may do
on either kind of machine.

## When the target exists

At startup the gateway looks for a driver and a desktop session, in this
order:

1. `[cua].enabled = false` in `config.toml` never looks for a driver.
2. `[cua].driver_path`, then the `NOLUNE_CUA_DRIVER` environment variable,
   then an executable `cua-driver` on `PATH`. A path that is named
   explicitly but cannot run is logged as an error; it is never treated as
   "no driver", so a typo does not silently remove the target.
3. A process inside a container (`/.dockerenv`, `/run/.containerenv`,
   `container=`, `KUBERNETES_SERVICE_HOST`) has no desktop session.
4. A Linux session without `DISPLAY` or `WAYLAND_DISPLAY` is headless.
5. No driver anywhere means no target.

On a headless host the server logs one line saying why, registers nothing,
and stays healthy: `list_machines` and the Computers page simply show no
server-local entry, and nothing claims GUI control the host does not have.
macOS and Windows sessions do not announce a display, so there the driver's
own health report decides.

When a driver is found the server spawns one persistent `cua-driver mcp`
child, completes the MCP handshake within `handshake_timeout_secs`, and asks
it for a health report. The report becomes the descriptor the target
advertises: platform, driver version, health (`healthy`, `degraded` when a
permission is missing, `unavailable` when a core check failed), the
accessibility and screen-capture permissions as the driver sees them, and
the capabilities those permissions allow. A driver that does not answer the
handshake or the health report is stopped and reported; the server runs on
without a server-local target.

## Identity

The target registers as `server-local:<hostname>`, the hostname reduced to
the protocol's identifier grammar (letters, digits, `-`, `_`, `.`), at most
128 bytes. Desktops register under a UUID they persist (or under the bare
hostname before #80), so the server and a desktop app on the same physical
machine never share an id, and a request that names one of them can never
mean the other. A desktop registration that claims the `server-local:`
prefix does not shadow the real target in the listing.

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

## Configuration

```toml
[cua]
enabled = true                 # false never looks for a driver
driver_path = ""               # empty: NOLUNE_CUA_DRIVER, then cua-driver on PATH
handshake_timeout_secs = 10    # MCP handshake at startup
call_timeout_secs = 30         # one driver call; a slow driver is cancelled, not waited on
run_timeout_secs = 900         # one run's session; ended and reported as timed out after this
```

Unknown keys in `[cua]` are refused at load. A zero timeout keeps the
default. The environment variable `NOLUNE_CUA_DRIVER` names the driver
binary when `driver_path` is empty.

## Related

- [`docs/companion-storage.md`](companion-storage.md) — the known machines
  file the desktop records live in.
- `cua-protocol/` — the machine protocol, `CheckedCuaAdapter`, and the driver
  wire mapping in `driver_mcp`.
- `server/src/services/cua/` — discovery, host probe, transport, runtime and
  sessions.
