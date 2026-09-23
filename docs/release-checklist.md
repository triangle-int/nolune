# Release checklist: model ids and the Codex pin

`scripts/bump-version.sh` moves the version number; it knows nothing about
which model ids the source names or which `codex` release Nolune speaks
to. Those are source constants with tests and docs around them, so a
release that changes either walks this list before the tag.
[providers.md](providers.md) is the page the entries below keep true.

## Model ids in the source

Nothing is seeded (#156): a preset is a model someone picked from what the
provider lists, so no release has to follow a model that retires upstream.
What still names model ids:

- `TOP_MODELS` in `server/src/services/llm/openrouter.rs`: the OpenRouter
  models onboarding offers, the most-used tool-calling models on
  openrouter.ai's rankings. An id the live catalog drops, or lists without
  tool calling, is hidden automatically, so the list can go stale but never
  offers a model that is gone.
- `first_key_probe_model` in `server/src/services/llm/mod.rs`: the model a
  first key's probe names before any preset exists. It is a probe only,
  never saved or offered, and any id the provider authenticates before
  checking is fine.
- The commented example preset in `server/src/onboard.rs` (the
  `config.toml` template `nolune onboard` writes) and the example presets
  in `server/config.example.toml`.
- `modelPlaceholder` in `client/src/lib/models/presets.js` (the Model id
  placeholder in the Connections settings page) and
  `client/src/lib/components/chat/ChatExample.svelte`: display only.
- `config::test_presets` in `server/src/config.rs`: a test fixture that
  stands in for presets someone picked; the `network_*` tests call its
  models.

Each release, or sooner when the rankings have moved:

1. Refresh `TOP_MODELS` from <https://openrouter.ai/rankings>: the most-used
   models that call tools, best first, as the `vendor/model` ids the
   catalog (`GET /api/v1/models`) lists with `tools` in
   `supported_parameters`. Update the month in its doc comment.
2. Run the ignored network test for each provider with a real key so the
   fixture ids still answer: `ANTHROPIC_API_KEY=... cargo test
   --manifest-path server/Cargo.toml --bin nolune -- --ignored
   network_anthropic`, and the same for `network_openai` /
   `network_openrouter` with their keys. A fixture id retired upstream is a
   change to `config::test_presets`, not to anyone's config.
3. Update the OpenRouter top models and the per-provider setup sections in
   `docs/providers.md`; `server/tests/provider_docs.rs` fails until every
   `TOP_MODELS` id appears there.
4. Existing installs are not touched: a preset keeps its model id until
   someone edits it. Say so in the release notes when a widely picked model
   is retired upstream, so people know to pick another (the connection test
   in Settings › Connections names a missing model).

## The pinned Codex release

Nolune speaks to one `codex` release, `0.155.0`, pinned as `CODEX_VERSION`
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
4. If `model/list` changed its models, nothing needs to follow: nothing is
   seeded, and onboarding and the preset editor list the live answer. If
   it changed its shape, `listed_models` in
   `server/src/services/llm/codex/adapter.rs` reads it (`id`,
   `displayName`, `description`, `hidden`, `isDefault`) and changes first.
5. Run the fixture-driven suite, then the live tests against the new
   binary: `cargo test --locked --manifest-path server/Cargo.toml -- codex`
   without credentials; the smoke test with a scratch home,
   `cargo test --locked --manifest-path server/Cargo.toml --bin nolune --
   --ignored services::llm::conformance::tests::codex_smoke` (needs only
   the binary on `PATH`); and `NOLUNE_CODEX_LIVE=1 cargo test --locked
   --manifest-path server/Cargo.toml --bin nolune -- --ignored
   services::llm::codex` for discovery, the handshake and, with a login in
   codex's home, the tool round trip through the real app-server. Check
   that no `codex app-server` child is left running afterwards.
6. Update the pinned version wherever the docs name it: the Codex setup
   step and the process section in `docs/providers.md`, and this page;
   `server/tests/provider_docs.rs` fails when either page names another
   release. `docs/settings.md` describes the login routes and needs a change
   only when their shapes did.
7. Smoke the Codex section of Settings → Connections and the Codex
   choice in onboarding with the real binary (`docs/settings.md` lists the
   states), and the incompatible state with the previous release still
   installed.
8. Say in the release notes which codex release is now required: people
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
