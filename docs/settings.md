# Navigation and settings (#98)

Nolune reads as a companion, not an admin dashboard. The client has five
primary destinations, and Settings is split into small pages by who owns each
control.

## Primary navigation

Defined once in `client/src/lib/companion/navigation.js` and rendered by
`client/src/routes/[slug]/+layout.svelte`.

| Tab | Route | Answers |
|-----|-------|---------|
| Chat | `/{slug}/chat` | Talk with your companion. |
| Activity | `/{slug}/activity` | What it did on its own, why, and what it made. Drops live here (`/{slug}/drops`), not as a tab. |
| Memory | `/{slug}/memory` | What it remembers; inspect, correct, forget. |
| Computers | `/{slug}/computers` | Connected spaces (#80): the server home (the server machine itself when it has a Cua driver, [computer-use.md](computer-use.md)) and every desktop that connected through the desktop app, from `GET /api/instances/{slug}/machines`, with state, permissions, capabilities, hints, an inline rename (`PUT …/machines/{id}`) and Forget on offline rows (`DELETE …/machines/{id}`). Computer use on any of them is explicit and permissioned: a screenshot is a one-shot capture of one window taken during an action, never a recording ([computer-use.md#privacy](computer-use.md#privacy)). |
| Settings | `/{slug}/settings` | How it behaves and what it may use. |

There is no Agents, Thoughts, Stats, Skills, or Drops tab. Skills are a
capability under Settings.

## Settings sections

Ownership is data in `client/src/lib/settings/sections.js`, covered by
`client/tests/settings-sections.test.mjs`. Every retained setting has exactly
one section and one scope:

- **server-global** (`scope: "server"`): applies to this whole Nolune server.
- **companion-specific** (`scope: "companion"`): describes the one companion it hosts.

`/{slug}/settings` redirects to the first section.

| Section | Route | Owns | Scope |
|---------|-------|------|-------|
| Companion | `settings/companion` | Little Moon presence, Learn my rhythm, Initiative (check-in, quiet hours, daily budget, reflection), Resume my work (#83: on/off, break, cooldown, snooze, Suggest now), Timezone, Scheduled messages | companion |
| Connections | `settings/connections` | Model presets and slots (#156) with capability chips and a connection test per preset (#28), API keys, the Codex login (#27: binary state against the pinned release, who codex is logged in as, Log in, Use a device code, Log out), Connected computers (the compact Connected Spaces list, #80), Companions (peer companions paired through federation, #108: rows with Confirm and Revoke, Invite a companion, Accept an invite, Rotate signing key; #109: pending approvals with Allow and Deny within one scope, and per-peer capability rows with one rule per request kind; #110: the inbox of what companions delivered and who they said they speak for; see [federation.md](federation.md)), Paired devices (browsers and desktop apps, #112) | server |
| Capabilities | `settings/capabilities` | Skills (registry), Extensions (curated MCP catalog, per-tool grants, custom servers behind the #97 acknowledgement) | server |
| Data | `settings/data` | What the companion keeps, Export, Import (replaces the companion after a confirmation dialog) | companion |
| Advanced | `settings/advanced` | Server port and API token, Updates and release channel, ElevenLabs voice ID, Email (SMTP/IMAP), GitHub token | mixed; each control carries an owner badge |

## Companions: approvals and capabilities (#109)

A paired companion may only check that it can reach this one. Anything
else asks the owner first, and the Companions section under Settings →
Connections is where they answer:

- **Requests waiting for you** sit above the rows, one per companion,
  request kind, and disclosure class ("wants to send you a message", "wants
  to ask whether you are free"), with when it asked and when it lapses (24
  hours). A native select picks one bounded scope, Once (the next matching
  request goes through and uses it up; unused, it lapses after an hour),
  For a day, For a week, or Always for this kind of request, and **Allow**
  or **Deny** applies it: once as a decision on that request, otherwise as
  a rule for the companion. A denial once holds until the request would
  have lapsed, so the companion is not queued again meanwhile; it keeps
  hearing that approval is required, exactly as before you looked, so it
  learns neither that you decided nor when. Decisions
  still standing (Allowed once, Denied) are listed beneath with Withdraw.
  Nothing a companion sent is shown or kept: the queue holds the request
  kind and the class, never a text.
- **What it may do** folds out under each paired row: one line per request
  kind and disclosure class the server could ever allow, saying what
  applies now (Allowed, Asks you, Denied) and whether that is the default,
  a rule set by you (with its deadline), or the default again after a rule
  lapsed. The Rule select writes one rule for that pair (Allowed, Asks you,
  Denied) or takes it back (Default); the very next request is judged by
  it. Revoking the companion drops every rule and pending request it had.
- Every decision is an audit receipt (`GET /api/federation/receipts`, side
  `owner`). Routes: `GET /api/federation/approvals`, `POST
  …/approvals/{id}/approve` and `…/deny` with `{scope}`, `DELETE
  …/approvals/{id}`, `POST /api/federation/peers/{id}/rules`, `POST
  …/rules/revoke` with `{intent, disclosure}`; shapes in
  [companion-storage.md](companion-storage.md) "Policy and audit".
- **The inbox** (#110) sits under the requests: one row per structured
  intent a paired companion delivered, saying who wants to do what
  ("wants to send you a message") and, quoted as the companion's own
  unverified words, who it said it speaks for and why. A row that needs
  you sits on the selected surface and points at the Allow and Deny of its
  request above; once you have allowed or denied it, the row says so
  ("You denied it once; it is refused when the companion asks again")
  until the companion asks again; a settled row says Delivered or Denied,
  when, and why ("Refused by your policy" or "You denied this once"). A
  delivered message itself is in your conversation, framed as that
  companion's untrusted content. Route: `GET /api/federation/inbox`.

## Model presets (#156)

There is no cheap, fast, or heavy tier and no per-message classifier. Users
name the models Nolune may call as **presets** (`[[llm.presets]]` in
`config.toml`: `id`, `name`, `provider`, `model`), and two slots say which
preset does which job:

- **Chat** (`chat_preset`): conversations, unless a chat pins its own preset
  from the composer picker (`GET/PUT /api/chat/{slug}/{chat_id}/preset`,
  stored in that chat's `meta.json`). A pinned preset that was deleted falls
  back to the Chat slot.
- **Background** (`background_preset`): memory extraction, chat titles,
  check-ins, and reflection. It never falls back to the chat preset; if it is
  unavailable, background work is skipped and logged.

Presets carry their own provider, so Anthropic, OpenAI and OpenRouter presets
coexist; API keys stay per provider. `GET/PUT /api/config/models` reads and
replaces presets plus slots atomically (validated: unique ids, known provider,
non-empty model, slots pointing at presets whose provider has a key). Nothing
is seeded: a fresh config has no presets, and a preset is a model someone
picked from what the provider lists or typed
([providers.md](providers.md#where-keys-and-model-choices-live) says what
each listing offers and how a picked model's preset id is made). A new key is
checked with its provider before it is saved (`PUT /api/config/llm`,
one-token completion); a rejected key answers 401 and stores nothing.

Two routes serve the pick:

- `GET /api/config/models/available?provider=<p>` answers
  `{provider, models: [{id, name, description?}]}`, the provider's listing
  in the order to offer it, and saves nothing. A key provider needs its key
  (409 `setup_required`), an unknown provider is 400 `unknown_provider`,
  and a provider that refuses or does not answer is typed like the
  connection test below (`authentication`, `rate_limited`, `unreachable`,
  `timeout`, …), with the key scrubbed.
- `POST /api/config/models/choose` with `{provider, model, name?}` tests the
  model as the preset it would become, on a copy of the config, and saves
  nothing unless it answers. Then the model becomes a preset (the one
  already naming it, if any) in the Chat slot; the Background slot follows
  unless it points at a ready preset on another provider than the one the
  Chat slot is leaving. `config.toml` is written, the backends are rebuilt,
  and the answer is the test's `ok` body plus `preset` (the id) and `models`
  (the `GET /api/config/models` body). A model that does not answer gets
  the test's typed outcome; no key is 409 `setup_required`, and an unknown
  provider or an invalid preset is 400 (`unknown_provider`,
  `invalid_presets`).

In Settings › Connections the preset editor has **Add preset** and no
per-provider defaults; with no presets it says "No presets yet. Add one and
pick a model your provider lists." Its Model id field suggests (a datalist)
what the row's provider lists for this account, fetched once per provider
the first time a model field on it is focused, and only when that provider
has a key or is Codex. Any other id is still accepted, and a suggested model
picked into a preset with no name names it after the model.

### Connection test and capability warnings (#28)

Every preset row under Settings › Connections has a **Test** button (once
the row is saved): `POST /api/config/models/{id}/test` sends one short
completion through the preset's adapter and answers with a typed outcome.
It creates no chat message and writes nothing under the workspace, so a test
never shows up in a conversation. A real answer is
`{ok: true, preset, provider, model, usage: {input_tokens, output_tokens},
capabilities}`; a failure is `{ok: false, error, message}` where `error`
names what to fix, matched on the adapter's `LlmError` variant:
`setup_required` (503, no key for the provider), `authentication` (401, the
provider rejected the key), `rate_limited` (429, the key works;
`retry_after_seconds` when the provider said), `model_not_found` (404),
`provider_rejected` (422, any other 4xx such as OpenRouter's "Insufficient
credits"), `provider_unavailable` (502, a 5xx), `unreachable` (502),
`timeout` (504, no answer within the probe deadline of 30 seconds, which
also bounds the key probe before a save), `invalid_response` (502),
`unsupported` (422) and `unknown_preset` (404). Messages name the provider
and arrive redacted from the adapters; the `authentication` answer is the
sentence "`<provider> rejected the API key.`" alone, because a provider's
own 401 text quotes the key it refused (OpenAI masks it as
`sk-revie******-key`), and that text goes to the server log instead. Every
other message is scrubbed of the configured key, whole or masked, so a
saved key never appears in any answer.

`GET /api/config/models` also carries `capabilities`, keyed by preset id:
what the provider offers for that model (`vision`, `documents`, `tools`,
`streaming`, `reasoning_controls`, `model_discovery`, `token_counting`).
The client turns the three that change what a conversation can do into
chips on the preset row (`no vision`, `no documents`, `no tools`, from
`capabilityWarnings` in `client/src/lib/models/presets.js`), appends them
to each option of the Chat and Background pickers, and repeats the sentence
under the picker as soon as a preset with a limitation is selected, before
Save. The composer's model picker does the same (`pickerPresets`): each
option carries its chips after the model id, and the sentence sits under
the composer while such a model is the conversation's. OpenRouter presets
report the catalog's answer once it has loaded, and the adapter defaults
until then.

Onboarding gates on the same test, run by `POST /api/config/models/choose`
when a model is picked (helpers in
`client/src/lib/components/onboarding/provider.js`). The companion is named
Nolune and gets the `moon` skin after the intro lines, with no language,
name or skin step, and the first message is sent as typed. After the soul
template and the provider, a key provider asks for its key
(`saveOnboardingKey`, probed before it is saved) unless the server already
has one (`configured_keys` from `/api/config/status`, such as an environment
variable; `stepAfterProvider`), and Codex goes through its login (below).
The model step then lists the provider's models (`listOnboardingModels`) in
one scrolling column: the name, the model id in mono when it differs, and
Codex's description, with a "Find a model" filter once there are nine or
more, and a note that more models or a separate background model can be
added later in Settings. A pick goes to `chooseOnboardingModel`, and
"<model name>. connected." is typed only after the model answered. A model
that does not answer keeps the list open with the outcome sentence above
it, so onboarding cannot finish with a model that does not reply; a
listing that fails shows its sentence with "try again" and "another
provider"; "choose another provider" leads back to the provider step from
the key, login and model steps. A model can answer once and still stop
later (no credits, a rate limit, a retired id), and `llm_configured` then
reads true on a reload; onboarding therefore tests the Chat preset again
whenever the status says a provider is configured (`resumeOnboarding`) and
skips to the first message only on an answer, otherwise typing the outcome
and returning to the provider step. A provider that was already working
stays as it was until something new answers: a new key is stored only when
its probe passes, and a picked model is saved only when it answers.

### OpenRouter (#26)

The `openrouter` provider sends OpenAI-style chat completions to
`openrouter.ai` with the `OPENROUTER` token (`OPENROUTER_API_KEY` overrides
it), streams answers and tool calls like the other adapters, and records the
usage and cost OpenRouter returns. Model ids are `vendor/model`, for example
`anthropic/claude-sonnet-4.6` or `openai/gpt-5.6-luna`; onboarding offers
the curated top models the live catalog still lists
([providers.md](providers.md#where-keys-and-model-choices-live)). The model
catalog (`GET /api/v1/models`) is read once an hour and a preset whose model
lacks tools, image input or reasoning controls is refused before the request
goes out.

Attribution and routing are off until `config.toml` names them; the
server's `public_url` is never sent:

```toml
[llm.openrouter]
site_url = "https://nolune.example"   # HTTP-Referer, listed on openrouter.ai app rankings
app_name = "Nolune"                   # X-Title
[llm.openrouter.routing]              # OpenRouter's `provider` object, sent as is
order = ["anthropic", "google"]
allow_fallbacks = false
```

### Codex (#27)

A ChatGPT login through the local `codex` binary is not an OpenAI API key
and never becomes one: `tokens.OPEN_AI` stays empty, and the login lives in
codex's own home, where Nolune never reads it. `GET /api/config/codex/status`
says whether the binary is installed and the pinned release (`binary.state`:
`ready`, `not_installed`, `incompatible` with both versions, `unusable`),
whether codex holds a login and its label (`account.kind`, `email`, `plan`),
and the login in flight. `POST /api/config/codex/login` starts one: the
ChatGPT managed flow when this host has a display (`auth_url` to open in a
browser on the same machine, since codex takes the callback on its own
localhost port) and a device code on a headless server (`verification_url`
to open anywhere plus `user_code` to type there); the body may force either
with `{"method": "browser" | "device_code"}`. The answer is only that and
the login `id` to poll the status with; `login.state` goes `pending` →
`completed` or `failed` (with the app-server's reason, or after fifteen
minutes without an answer). `POST /api/config/codex/logout` forgets the
login. A missing or mismatched binary answers 503 with `codex_not_installed`,
`codex_incompatible` or `codex_unusable` and the message names the path and
both versions; an app-server that could not answer is 502
`codex_unavailable`. No response, log line or state type carries a token;
`server/tests/codex_auth.rs` keeps it so, and it scans the client's Codex
module, tile and API blocks for the same token-bearing names.

The Codex section of Settings › Connections
(`client/src/lib/components/settings/CodexLogin.svelte`, copy from the pure
`client/src/lib/models/codex.js`) sits under the API keys and holds no key
field. One row names the release and its path (or the release Nolune
supports when the binary is not there), then one state line: not installed,
another release (both versions and the path), cannot run, could not answer,
not logged in, a pending login, a failed login with codex's reason, or
logged in as the email and plan codex reports. Log in starts the server's
`auto` flow and Use a device code forces one; while the login is pending the
tile shows the URL to open and the code to type in the pairing panel's
shape, polls the status every two seconds and updates itself when codex has
the login; the account decides, not the login record (`loginProgress` in
`codex.js` answers `completed` once `logged_in` is true), so a `codex login`
run in a terminal on the server ends the wait too. Cancel (a logout) ends a
pending login, Log out forgets the login. A typed refusal (`codex_not_installed`, `codex_incompatible`,
`codex_unusable`, `codex_unavailable`, `codex_refused`) reads as one
sentence with what to do. Model presets treat `codex` as a login provider
(`PROVIDERS` in `client/src/lib/models/presets.js`, `auth: "login"`): a
slot on a Codex preset needs no key, the editor suggests the models codex's
`model/list` answers, and the capability chips (`no vision`,
`no documents`) come from the server's `capabilities` like any other
preset's. A connection test on a Codex preset answers the server's own
setup sentence (no binary, another release, no login) instead of asking for
a key.

Onboarding offers Codex as its own choice ("your ChatGPT login"). The gate
is the login (`connectOnboardingCodex` in
`client/src/lib/components/onboarding/provider.js`), then a model that
answers, as for any provider: the status is read first, a binary that is
missing or another release stops there with the reason and "choose another
provider", no login starts one and shows the URL and code until the poll
sees it completed, and only then does the model step list what codex
offers; a failed login offers "try again", "use a device code" and another
provider. While the browser flow waits (the server's `auto` picks it
on a host with a display, and its URL only works on that machine), the
login step offers "on another device? use a device code", which starts a
device-code login in its place (the server cancels the pending one) and
shows its URL and code instead; a login finished outside the record
(`codex login` on the server) is picked up on the next poll, since the
status reports the account before the record ends. On a reload with a Codex chat preset, the same test runs
and a lost login returns to the provider step with the login sentence.

## Layout (#153)

Each section page renders inside one `.settings-panel` from
`client/src/lib/settings/settings.css`. A page is a stack of
`.settings-section` rows divided by 1px borders; each row is a two-column grid
with the `.section-header` (Fraunces title, one-line description, owner badge on
Advanced) on the left and every other child in the controls column on the
right. Below 768px the header stacks above the controls. There is no card grid
and no `settings-wide` span helper, so sections of different heights cannot
leave empty space. The section nav under the `Settings` heading is a segmented
control with a lavender active pill and `aria-current`.
`server/tests/navigation_settings_split.rs` guards the panel, the divider, and
the retired icon and grid markup.

## Data: export and import (#74)

Export (`GET /api/instances/{slug}/export`) downloads `companion.tar.gz`, the
versioned archive described in [companion-storage.md](companion-storage.md)
under *Archive format*. Import replaces the companion with such an archive:
afterwards its memory, personality, drops, and chat history are the
archive's, and what it kept before is not kept. Because of that the Data
page asks once before anything leaves the browser, in the shadcn
AlertDialog the design system prescribes for destructive companion
confirmation: the title asks, the description names the file and its size
and says what is replaced, Keep current is focused first, Replace is the
destructive action, and Escape or a click outside keeps the current data
and returns focus to the Import button. It then shows the upload as it
streams and a restoring line while the server validates the archive and
rebuilds the search index, and ends with what was restored (files, size,
whether the index was rebuilt or is pending until the next start). A
refused archive, a busy companion (`409`: an agent is still running, or a
request or background routine is still writing), or an interrupted upload
is announced with what it means, and in every one of those cases nothing
was changed.

The same restore is reachable in two other ways, both through the server's
validating import: the `restore_backup` tool takes only the upload id of an
archive attached to the chat (never a path) and refuses without the user's
explicit confirmation in that conversation, and `nolune restore <archive>`
sends an operator-chosen local file to the running server with the API
token from `config.toml` (`--profile` for a named profile, `--yes` to skip
the question when no terminal is attached). `POST
/api/instances/{slug}/import` is the multipart endpoint behind all three.

## Raw fields stay on Advanced

Settings flagged `raw` in `sections.js` are server or protocol wiring: ports,
tokens, hosts, voice IDs, SMTP/IMAP and GitHub credentials.
They appear only on the Advanced page, so common companion setup (pick a
provider, add a key, set a timezone, tune initiative) never shows them.
`server/tests/navigation_settings_split.rs` scans the four consumer pages for
these fields and fails if one leaks, and fails if Advanced stops offering
them.

Self-hosting controls that have no UI (local command MCP servers, the public
URL, embedding endpoints) live in `~/.nolune/config.toml`; the README lists
the environment overrides.

## Computer-use driver (#20)

Computer use runs on a [Cua Driver](https://github.com/trycua/cua) that is
pinned per Nolune release in `cua_protocol::cua_driver_pin` (version, release
tag, one verified asset per target; `cua-protocol/cua-driver.pin` is the copy
scripts read). There is no setting for the driver version and no auto-update:
Nolune never runs the driver's self-updater, and a new pin ships with a new
Nolune release.

- The desktop app installs the driver on the computer it runs on: its
  Settings window, under Computer use, has an **Install driver** button
  (#231) that runs the same verified installer into the same
  `~/.nolune/cua-driver/`. It exists because a desktop user has no
  `nolune` on their `PATH`: the app's in-app server install puts the
  binary in `~/.nolune/bin` without touching `PATH`, and a desktop bound
  to a server elsewhere has no binary at all. Everything below about the
  download, the checks and the mirror applies to it unchanged.
- `nolune cua install` downloads the pinned asset for this host, checks its
  size and sha256 before writing anything, extracts it under
  `~/.nolune/cua-driver/releases/<version>/` (a profile's own data root with
  `--profile`), asks the extracted binary for its version and refuses any
  other than the pin, then records the install in
  `~/.nolune/cua-driver/install.json`. A failed step leaves nothing behind.
  The download itself is bounded by the pin: a mirror that announces another
  size is refused before the body is read, a body that grows past the pinned
  size is abandoned, and a stalled mirror fails on a read deadline (60 s of
  silence) or the overall one (30 min) instead of hanging; the release
  script's curl carries `--max-filesize` and `--max-time` for the same
  reason. `NOLUNE_CUA_RELEASE_URL` points it at a mirror; the checksum is
  enforced either way. `NOLUNE_INSTALL_CUA_DRIVER=1` makes the one-line
  installer run it; it is off by default.
- `nolune cua status` prints the pin, the platform state, the session state,
  the installed driver checked against the pin, the driver the server would
  run (an explicit `NOLUNE_CUA_DRIVER`, then the workspace install, then
  `PATH`), and, when the host can run it, the driver's own health report: its
  version against the pin (a mismatch is reported and exits 1) and the
  Accessibility and Screen Recording state under the driver's bundle
  identity. When the driver cannot report, the status line repeats what the
  driver said on stderr (its permission hint, for one) instead of a bare
  handshake timeout.
- The macOS daemon: `cua-driver mcp` on macOS is a proxy to the login
  session's `CuaDriver.app` daemon, and on its own it would start one *by
  name* through LaunchServices, which picks whatever `CuaDriver.app` the
  system knows (none on a fresh Mac, `/Applications/CuaDriver.app` after the
  upstream installer), never the copy Nolune verified. So when the driver
  runs from a genuine `CuaDriver.app` (its `Info.plist` says
  `com.trycua.driver`) and no daemon is running, `nolune cua status` starts
  that bundle by path (`open -n -g <bundle> --args serve`, which is what
  makes macOS attribute the permissions to the bundle) and reports
  `daemon: started … by path`; the daemon keeps running, `<driver> stop`
  stops it. The health report names the executable that answered
  (`answered by: …`): when it is not the driver above, another
  `CuaDriver.app` owns the session's daemon (the driver keeps one per login
  session), the line says so and how to stop it, and the version check still
  applies to it. A foreign daemon of another release can refuse the driver
  above outright (`incompatible daemon: contract version … does not match
  SDK …`) before any report, so the `daemon:` line names it too. The CLI
  never stops or replaces a daemon it did not start; the desktop app's
  settings window does, but only when its button is pressed (see
  [computer-use.md](computer-use.md)).
- Platform status: macOS is supported. Linux and Windows report
  `platform: unsupported`; the pinned driver still installs there so a later
  release can turn computer use on without moving the pin. A host without a
  graphical session reports `headless` and the driver is never started: on
  Linux without `DISPLAY` or `WAYLAND_DISPLAY`, on macOS when `launchctl
  managername` is not `Aqua` (an SSH or background session), on Windows
  without `SESSIONNAME`; headless installs need nothing from this section.
- Releases: the desktop job of `release.yml` fetches the pinned asset for each
  desktop target with `scripts/cua-driver.sh`, verifies it against the pin,
  and uploads the verified archive beside the desktop bundle, so a pin that
  no longer matches upstream fails the release instead of a user's install.
- The desktop app: its Settings window shows Accessibility and Screen
  Recording as macOS granted them to the driver's own bundle
  (`com.trycua.driver`), read from the driver's report, beside the workspace
  install and the reported version against the pin; Grant runs the driver's
  own `permissions grant` and opens the System Settings pane. Linux, Windows
  and headless sessions are named as such with nothing to grant. See
  [computer-use.md](computer-use.md#permissions-20).

## Companions (#108)

The Companions section of Connections lists the peer companions this
server's owner paired with, from `GET /api/federation/peers` viewed by the
pure helpers in `client/src/lib/federation/companions.js`
(`client/tests/federation-companions.test.mjs`) and rendered by
`CompanionRow.svelte`; `Companions.svelte` loads, polls, and acts. A row is
the peer's id, its state beside a color (Paired, Waiting for you, Waiting
for its owner, Revoked), who invited whom, the approved origins, and when
something it signed last verified here. **Invite a companion** mints an
invite and shows its one line once, in a panel that keeps nothing after
Done; **Accept an invite** takes a pasted line, refuses anything URL-shaped
before sending, and posts the line in a JSON body; **Confirm** pairs a
pending peer this server invited; **Revoke** asks once, inline; **Rotate
signing key** asks once and reports which peers were told. The same actions
exist as `nolune federation …` on the command line; both are described in
[federation.md](federation.md).

## Verifying

- `cd client && pnpm check && pnpm test && pnpm build`
- `cd server && cargo test --test navigation_settings_split` and the
  `connected_computers_are_listed_for_the_one_companion_only` router test.
- `cargo test --manifest-path server/Cargo.toml --test federation_cli` for
  the Companions section guards and the `nolune federation` flow between
  two profiles on one host.
- `cargo test --manifest-path server/Cargo.toml --test cli_cua_driver` for
  the driver install and status paths, and `bash scripts/tests/install.sh
  && bash scripts/tests/release-workflow.sh` for the installer step and the
  release fetch.
- Open each section at phone width: the settings nav scrolls horizontally, the
  grid collapses to one column, and no page scrolls sideways.
