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
| Computers | `/{slug}/computers` | Connected spaces (#80): the server home and every desktop that connected through the desktop app, from `GET /api/instances/{slug}/machines`, with state, permissions, capabilities, hints, an inline rename (`PUT …/machines/{id}`) and Forget on offline rows (`DELETE …/machines/{id}`). |
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
| Connections | `settings/connections` | Model presets and slots (#156), API keys, Connected computers (the compact Connected Spaces list, #80), Paired browsers | server |
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

Presets carry their own provider, so Anthropic and OpenAI presets coexist;
API keys stay per provider. `GET/PUT /api/config/models` reads and replaces
presets plus slots atomically (validated: unique ids, known provider, non-empty
model, slots pointing at presets whose provider has a key), and
`POST /api/config/models/seed` adds a provider's defaults, which onboarding
calls after saving the first key.

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

## Verifying

- `cd client && pnpm check && pnpm test && pnpm build`
- `cd server && cargo test --test navigation_settings_split` and the
  `connected_computers_are_listed_for_the_one_companion_only` router test.
- Open each section at phone width: the settings nav scrolls horizontally, the
  grid collapses to one column, and no page scrolls sideways.
