# Release checklist: model defaults and the Codex pin

`scripts/bump-version.sh` moves the version number; it knows nothing about
which models Nolune seeds or which `codex` release it speaks to. Those are
source constants with tests and docs around them, so a release that changes
either walks this list before the tag. [providers.md](providers.md) is the
page the entries below keep true.

## Model defaults

Where a default model id lives:

- `default_presets` in `server/src/config.rs`: the presets seeded when a
  provider is first set up, and `default_slots` next to it (which seeded
  preset fills the chat and background slots).
- `server/src/onboard.rs` (the `config.toml` template `nolune onboard`
  writes) and `server/config.example.toml`: the Anthropic defaults again.
- `client/src/lib/components/chat/ChatExample.svelte` and the Model id
  placeholder in the Connections settings page: display examples only.

To change one:

1. Change the id in `default_presets` (and `default_slots` if the seeded
   preset ids change), then the onboarding template and the example config
   when the Anthropic defaults moved.
2. Run the ignored network test for that provider with a real key so the
   new id answers: `ANTHROPIC_API_KEY=... cargo test --manifest-path
   server/Cargo.toml --bin nolune -- --ignored network_anthropic`, and the
   same for `network_openai` / `network_openrouter` with their keys. The
   seeded ids are also what `probe_model` uses for a first key's probe, so
   a retired id would break onboarding, not only the chat.
3. Update the seeded-models table and the per-provider setup sections in
   `docs/providers.md`; `server/tests/provider_docs.rs` fails until every
   seeded id appears there.
4. Existing installs are not touched: seeding only runs for a provider with
   no presets, so a person's `config.toml` keeps its ids. Say so in the
   release notes when a default is retired upstream, so people know to edit
   their presets.

## The pinned Codex release

Nolune speaks to one `codex` release, `0.156.1`, pinned as `CODEX_VERSION`
in `server/src/services/llm/codex/mod.rs`, and the fake app-server the
tests use plays `server/src/services/llm/fixtures/codex-<version>.jsonl`.
A newer codex on the machine is refused by discovery until the pin moves.

To move it:

1. Install the new release locally (`codex --version` must print it) and
   read its protocol changes: `codex app-server generate-json-schema
   --experimental --out <dir>` with a scratch `CODEX_HOME` writes the v2
   schema; diff `ClientRequest`, `ServerRequest`, `ServerNotification`,
   `CodexErrorInfo` and the `thread/start` params against the previous
   release. Anything the adapter sends or reads that changed shape is an
   adapter change first.
2. Set `CODEX_VERSION`, rename the fixture to `codex-<new>.jsonl` and
   update its `pin` header line; `fake::fixture_path()` follows the
   constant.
3. Re-record the live shapes with a scratch `CODEX_HOME` and no login (the
   fixture header lists which entries are captures and which follow the
   published schema): `initialize`, `model/list`, `account/read`,
   `config/read`, `thread/start`, `thread/resume`, `turn/start`, the turn
   events, `turn/interrupt`, the failed and interrupted `turn/completed`,
   the pre-initialize and unknown-method errors. Nothing under `~/.codex`
   is read or written when `CODEX_HOME` points elsewhere.
4. Check the tool surface: give a scratch `CODEX_HOME` a `config.toml`
   whose `model_provider` is a local Responses stub (`wire_api =
   "responses"`, a `base_url` on `127.0.0.1`) that records each request
   body, start a thread with the adapter's `thread/start` params and one
   turn with its `turn/start` params, then resume the thread in a new
   app-server and run another turn. Every body's `tools` must list
   Nolune's dynamic tools and nothing else, the instructions must carry no
   skills catalog, and no `configWarning` may name one of the overrides as
   ignored; a tool that appears is a new switch in `thread_config` (or in
   the `environments` the adapter sends) before the pin moves.
5. If `model/list` changed, update the seeded Codex presets in
   `default_presets` (the Model defaults list above applies) and the
   `CODEX_MODELS` catalog in `client/src/lib/models/presets.js` that the
   preset editor and onboarding offer.
6. Run the fixture-driven suite, then the live tests against the new
   binary: `cargo test --locked --manifest-path server/Cargo.toml -- codex`
   without credentials; the smoke test with a scratch home,
   `cargo test --locked --manifest-path server/Cargo.toml --bin nolune --
   --ignored services::llm::conformance::tests::codex_smoke` (needs only
   the binary on `PATH`); and `NOLUNE_CODEX_LIVE=1 cargo test --locked
   --manifest-path server/Cargo.toml --bin nolune -- --ignored
   services::llm::codex` for discovery, the handshake and, with a login in
   codex's home, the tool round trip through the real app-server. Check
   that no `codex app-server` child is left running afterwards.
7. Update the pinned version wherever the docs name it: the Codex setup
   step and the process section in `docs/providers.md`, and this page;
   `server/tests/provider_docs.rs` fails when either page names another
   release. `docs/settings.md` describes the login routes and needs a change
   only when their shapes did.
8. Smoke the Codex section of Settings → Connections and the Codex
   choice in onboarding with the real binary (`docs/settings.md` lists the
   states), and the incompatible state with the previous release still
   installed.
9. Say in the release notes which codex release is now required: people
   who installed the previous one see `codex_incompatible` from
   `GET /api/config/codex/status` until they upgrade.

## Before the tag

- `cargo test --locked --manifest-path server/Cargo.toml --all-targets`
  green, including `tests/provider_docs.rs`, `tests/provider_boundary.rs`,
  `tests/model_presets.rs` and `tests/codex_auth.rs`.
- `./scripts/bump-version.sh <version>`, then commit, tag and push as
  `CLAUDE.md` says.
- The Cua Driver has a pin of its own (`docs/computer-use.md`); a release
  that moves it follows that page.
- Computer use is checked by hand on a macOS machine with a graphical
  session: `scripts/release-check-computer-use.sh` checks the
  prerequisites and prints the steps to verify against the real driver
  (`docs/computer-use.md`, "Release check").
