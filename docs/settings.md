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
| Computers | `/{slug}/computers` | Connected spaces (#80): the server home (the server machine itself when it has a Cua driver, [computer-use.md](computer-use.md)) and every desktop that connected through the desktop app, from `GET /api/instances/{slug}/machines`, with state, permissions, capabilities, hints, an inline rename (`PUT …/machines/{id}`) and Forget on offline rows (`DELETE …/machines/{id}`). |
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
| Connections | `settings/connections` | Model presets and slots (#156) with capability chips and a connection test per preset (#28), API keys, Connected computers (the compact Connected Spaces list, #80), Companions (peer companions paired through federation, #108: rows with Confirm and Revoke, Invite a companion, Accept an invite, Rotate signing key; #109: pending approvals with Allow and Deny within one scope, and per-peer capability rows with one rule per request kind; see [federation.md](federation.md)), Paired browsers | server |
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
non-empty model, slots pointing at presets whose provider has a key), and
`POST /api/config/models/seed` adds a provider's defaults, which onboarding
calls after saving the first key. A new key is checked with its provider
before it is saved (`PUT /api/config/llm`, one-token completion); a rejected
key answers 401 and stores nothing.

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

Onboarding runs the same test after the first key is saved and its
presets are seeded (`saveOnboardingProvider` in
`client/src/lib/components/onboarding/provider.js`, on the Chat slot when it
runs on that provider, else the provider's first preset). "connected." is
typed only after the model answered; a failure keeps the key step open
with the outcome sentence, so onboarding cannot finish with a provider
that does not reply, and "choose another provider" leads back to the
provider step. A key can pass the probe and still have no usable model (no
credits, a rate limit, a retired id), and `llm_configured` then reads true
on a reload; onboarding therefore tests the Chat preset again whenever the
status says a provider is configured (`resumeOnboarding`) and skips to the
first message only on an answer, otherwise typing the outcome and
returning to the provider step. Once a preset answers, the slots follow it
(`slotsAfterOnboardingTest`): the Chat slot moves to that preset, and the
Background slot to the provider's second preset when it pointed at the
provider being left, so the first message never goes through a provider
that did not answer. A provider that was already working is untouched: a
new key is stored only when its probe passes, its own presets are the ones
tested, and the test itself saves nothing.

### OpenRouter (#26)

The `openrouter` provider sends OpenAI-style chat completions to
`openrouter.ai` with the `OPENROUTER` token (`OPENROUTER_API_KEY` overrides
it), streams answers and tool calls like the other adapters, and records the
usage and cost OpenRouter returns. Model ids are `vendor/model`, for example
`anthropic/claude-sonnet-4.6` or `openai/gpt-5.4-mini`; the seeded presets
name both. The model catalog (`GET /api/v1/models`) is read once an hour and a
preset whose model lacks tools, image input or reasoning controls is refused
before the request goes out.

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
  applies to it. Nolune never stops or replaces a daemon it did not start.
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
