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
| Companion | `settings/companion` | Little Moon presence, Learn my rhythm, Initiative (check-in, quiet hours, daily budget, reflection), Timezone, Scheduled messages | companion |
| Connections | `settings/connections` | Model presets and slots (#156) with capability chips and a connection test per preset (#28), API keys, Connected computers (the compact Connected Spaces list, #80), Paired browsers | server |
| Capabilities | `settings/capabilities` | Skills (registry), Extensions (curated MCP catalog, per-tool grants, custom servers behind the #97 acknowledgement) | server |
| Data | `settings/data` | What the companion keeps, Export, Import | companion |
| Advanced | `settings/advanced` | Server port and API token, Updates and release channel, ElevenLabs voice ID, Email (SMTP/IMAP), GitHub token | mixed; each control carries an owner badge |

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
`timeout` (504), `invalid_response` (502), `unsupported` (422) and
`unknown_preset` (404). Messages name the provider and arrive redacted
from the adapters; saved keys never appear in any answer.

`GET /api/config/models` also carries `capabilities`, keyed by preset id:
what the provider offers for that model (`vision`, `documents`, `tools`,
`streaming`, `reasoning_controls`, `model_discovery`, `token_counting`).
The client turns the three that change what a conversation can do into
chips on the preset row (`no vision`, `no documents`, `no tools`, from
`capabilityWarnings` in `client/src/lib/models/presets.js`), appends them
to each option of the Chat and Background pickers, and repeats the sentence
under the picker as soon as a preset with a limitation is selected, before
Save. OpenRouter presets report the catalog's answer once it has loaded,
and the adapter defaults until then.

Onboarding runs the same test after the first key is saved and its
presets are seeded (`saveOnboardingProvider` in
`client/src/lib/components/onboarding/provider.js`, on the Chat slot when it
runs on that provider, else the provider's first preset). "connected." is
typed only after the model answered; a failure keeps the key step open
with the outcome sentence, so onboarding cannot finish with a provider
that does not reply. A provider that was already working is untouched: a
new key is stored only when its probe passes, seeding never moves a slot
that points at a usable preset, and the test itself saves nothing.

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

## Verifying

- `cd client && pnpm check && pnpm test && pnpm build`
- `cd server && cargo test --test navigation_settings_split` and the
  `connected_computers_are_listed_for_the_one_companion_only` router test.
- `cargo test --manifest-path server/Cargo.toml --test cli_cua_driver` for
  the driver install and status paths, and `bash scripts/tests/install.sh
  && bash scripts/tests/release-workflow.sh` for the installer step and the
  release fetch.
- Open each section at phone width: the settings nav scrolls horizontally, the
  grid collapses to one column, and no page scrolls sideways.
