# Model providers

Nolune talks to a model provider through one adapter per provider under
`server/src/services/llm/`. Presets pick the provider and model per
conversation, and a key is entered once per provider in Settings →
Connections; [settings.md](settings.md) describes presets, slots, the
connection test and the OpenRouter options. This page is the setup guide
for all four providers, where their secrets live, what each one can and
cannot do, how Codex runs as a local process, what happens to a
configuration from before presets, and how the adapters are tested.
[release-checklist.md](release-checklist.md) says what to check when a
model default or the pinned Codex release changes.

| Provider | Authenticates with | Model ids | Wire format lives in |
| --- | --- | --- | --- |
| Anthropic | API key | `claude-<family>-<version>` | `services/llm/anthropic.rs` |
| OpenAI | API key | `gpt-<version>`, `o<n>` | `services/llm/openai.rs` |
| OpenRouter (#26) | API key | `vendor/model[:variant]` | `services/llm/openrouter.rs` |
| Codex (#27) | a ChatGPT login held by the local `codex` binary | the ids the pinned release lists | `services/llm/codex/` |

## Where keys and model choices live

Everything is in `config.toml` under the data directory: `~/.nolune` by
default, `NOLUNE_HOME` when set, `~/.nolune-profiles/<name>/` for a
`--profile`. The keys sit in the `[llm.tokens]` table; the environment
variable of the same purpose overrides the file's value for that process
and is never written back by the override itself.

| `[llm.tokens]` key | Environment override | Used by |
| --- | --- | --- |
| `ANTHROPIC` | `ANTHROPIC_API_KEY` | the Anthropic provider |
| `OPEN_AI` | `OPENAI_API_KEY` | the OpenAI provider (never Codex) |
| `OPENROUTER` | `OPENROUTER_API_KEY` | the OpenRouter provider |
| `BRAVE_SEARCH` | `BRAVE_SEARCH_API_KEY` | the web search tool, not a model provider |
| `ELEVENLABS` | `ELEVENLABS_API_KEY` | voice, not a model provider |

```toml
[llm]
chat_preset = "sonnet"
background_preset = "haiku"

[llm.tokens]
ANTHROPIC = "sk-ant-..."
OPEN_AI = ""
OPENROUTER = ""

[[llm.presets]]
id = "sonnet"
name = "Claude Sonnet"
provider = "anthropic"
model = "claude-sonnet-4-6"
```

The keys are read by the server only: the API answers which providers
have a key (`configured_keys`, `keyed_providers`), never the key, and a
new key is checked with its provider (one short completion) before it is
saved, so a rejected key is never stored. Codex has no row in the table:
its login lives in codex's own home directory and Nolune never reads it
(see [Codex](#codex)).

Model choices are `[[llm.presets]]` entries (`id`, `name`, `provider`,
`model`) and the two slots, `chat_preset` for conversations and
`background_preset` for memory extraction, titles, check-ins and
reflection; a chat may pin its own preset from the composer. Setting up a
provider for the first time seeds its defaults, which are ordinary
presets you can rename, edit or delete:

| Provider | Seeded preset ids | Models | Slots filled |
| --- | --- | --- | --- |
| Anthropic | `sonnet`, `opus`, `haiku` | `claude-sonnet-4-6`, `claude-opus-4-6`, `claude-haiku-4-5-20251001` | chat `sonnet`, background `haiku` |
| OpenAI | `gpt-sol`, `gpt-luna` | `gpt-6-sol`, `gpt-6-luna` | chat `gpt-sol`, background `gpt-luna` |
| OpenRouter | `openrouter-sonnet`, `openrouter-gpt-luna` | `anthropic/claude-sonnet-4.6`, `openai/gpt-5.6-luna` | chat `openrouter-sonnet`, background `openrouter-gpt-luna` |
| Codex | `codex-sol`, `codex-luna` | `gpt-6-sol`, `gpt-6-luna` | chat `codex-sol`, background `codex-luna` |

The defaults are `default_presets` in `server/src/config.rs`; the
onboarding template in `server/src/onboard.rs` and
`server/config.example.toml` repeat the Anthropic ones. A slot is only
valid when its preset's provider is ready: a key provider needs its key,
Codex needs nothing in the config (its login is runtime state). Presets
of different providers coexist; a preset's model id is not validated
against the provider beyond its shape (OpenRouter's `vendor/model`), so
the connection test in Settings → Connections is how a typo is found.

### Configurations from before presets

Until #156 the `[llm]` table chose one provider and a model mode. Those
keys are retired and `load_config` handles them the same way every time:

- `provider`, `model_mode`, `profiles`, `model` and `heavy_multiplier`
  under `[llm]` are dropped on load and never written back; the server
  logs one warning naming them (`config.llm.<key> (model presets replaced
  model modes, #156)`). A `provider` value is read once before it is
  dropped: when the config has no presets yet, `seed_for_keys` seeds the
  defaults of every provider that has a key and puts the named provider
  (`api`, `cli` and `claude_cli` still mean Anthropic) in the slots, so a
  pre-presets install boots into a working chat rather than into setup.
- `GOOGLE_AI`, `google_ai` and `gemini` under `[llm.tokens]` powered the
  retired video analysis (#91); they are dropped on load and on save and
  reported the same way.
- Anything else under `[llm]` that the server does not know is kept as is
  and written back unchanged, so a key from a newer version survives a
  round trip through an older one.

## Anthropic

1. Create a key at <https://console.anthropic.com/settings/keys>.
2. Enter it under Settings → Connections (or set `ANTHROPIC` in
   `[llm.tokens]`, or `ANTHROPIC_API_KEY` in the environment). Saving
   seeds `sonnet`, `opus` and `haiku`.
3. Model ids are the Messages API ids: `claude-sonnet-4-6`,
   `claude-opus-4-6`, `claude-haiku-4-5-20251001`.

The adapter speaks the Messages API (`/v1/messages`, streaming SSE) and
is the only place that does. It sends PDFs and images, reports the
provider's own token count for the context meter
(`/v1/messages/count_tokens`), and asks for prompt caching by execution
scope: one-hour cache breakpoints on the conversation path, five-minute
ones for one-shots and companion routines (#137). Reasoning effort is not
a parameter here; the model catalog is not discoverable, so a model id is
checked by the connection test. Anthropic's 529 `overloaded_error`, in a
status or mid-stream as an SSE `error` event, is reported as a rate limit.

## OpenAI

1. Create a key at <https://platform.openai.com/api-keys>.
2. Enter it under Settings → Connections (or `OPEN_AI` in `[llm.tokens]`,
   or `OPENAI_API_KEY`). Saving seeds `gpt-sol` and `gpt-luna`.
3. Model ids are the Responses API ids: `gpt-6-sol`, `gpt-6-luna`,
   `gpt-5.6-sol`, `o3`.

The adapter speaks the Responses API (`/v1/responses`, `store: false`,
streaming SSE). Images go out as `input_image` and PDFs as `input_file`:
inline as `file_data`, or by `file_url` when the public URL is reachable
by the provider, including PDFs a tool returns. `reasoning.effort` is forwarded for GPT-5 and every later
generation and the o-series, and refused before the network for other models. There is
no token-counting endpoint, so the context meter is a local estimate for
OpenAI presets. An OpenAI key is OpenAI's only: it is never used for
Codex, and a Codex login never fills `OPEN_AI`.

## OpenRouter

1. Create a key at <https://openrouter.ai/settings/keys>.
2. Enter it under Settings → Connections (or `OPENROUTER` in
   `[llm.tokens]`, or `OPENROUTER_API_KEY`). Saving seeds
   `openrouter-sonnet` and `openrouter-gpt-luna`.
3. Model ids are `vendor/model`, optionally with a `:variant` suffix:
   `anthropic/claude-sonnet-4.6`, `openai/gpt-5.6-luna`,
   `meta-llama/llama-4:free`. A bare id is refused when the preset is
   saved.

The adapter speaks OpenAI-style chat completions at `openrouter.ai`
(`/api/v1/chat/completions`, streaming SSE) with `usage.include` on every
request, so the cost OpenRouter charged rides along with the token counts.
The model catalog (`/api/v1/models`) is read once an hour: a preset whose
model lacks tools, image input or reasoning controls is refused before
the request goes out, and a catalog that cannot be fetched is logged and
the request proceeds with the defaults. Attribution (`HTTP-Referer`,
`X-Title`) and provider routing are off until `[llm.openrouter]` names
them, and the server's own `public_url` is never sent; the example lives
in [settings.md](settings.md#openrouter-26). Documents are not sent
through OpenRouter.

## Codex

A ChatGPT/Codex login is not the OpenAI API, and Nolune does not pretend it
is. Instead the server runs one local `codex app-server` child process and
speaks its stdio protocol: one JSON object per line, JSON-RPC 2.0 shapes
without the `jsonrpc` member. The login stays inside codex's own home
directory; Nolune never reads, copies or logs its tokens.

### Setup

1. Install the Codex CLI at the release Nolune is pinned to, `0.156.1`
   (`npm install -g @openai/codex@0.156.1`, or the release's binary), on
   the machine that runs the Nolune server. Discovery runs
   `codex --version` and refuses any other release with an error that
   names both versions; `NOLUNE_CODEX_BIN` points at a binary that is not
   on `PATH`.
2. Sign in: run `codex login` on that machine, or start a login through
   `POST /api/config/codex/login` (the managed browser flow on a machine
   with a display, a device code on a headless server); the Settings →
   Connections tile that drives it is the remaining slice of #27. Nothing
   is entered in Nolune and nothing is written to `config.toml`.
3. Seed the Codex presets (`POST /api/config/models/seed` with
   `{"provider": "codex"}`: `codex-sol` on `gpt-6-sol`, `codex-luna`
   on `gpt-6-luna`) or write `provider = "codex"` presets in
   `config.toml`; the model ids are what `model/list` of the pinned
   release returns.

`GET /api/config/codex/status` says whether the binary is installed at the
pin, whether codex holds a login and its label (`kind`, `email`, `plan`),
and the login in flight; nothing in that answer is a token. Without a
login a turn fails with a setup error that says to sign in.

### The local process

The protocol is experimental, so it is pinned:

- `CODEX_VERSION` in `server/src/services/llm/codex/mod.rs` names the one
  release Nolune speaks to. Discovery runs `codex --version` and refuses any
  other release with an error that names both versions, rather than a best
  effort against a protocol it was not tested with. The binary comes from
  `NOLUNE_CODEX_BIN` when that is set, else from `PATH`.
- `server/src/services/llm/fixtures/codex-<version>.jsonl` records that
  release's answers and events. The tests run a fake app-server that plays
  the fixture, so CI never needs the real binary; bumping the pin means
  re-recording the fixture ([release-checklist.md](release-checklist.md)).

The supervisor keeps exactly one child. It completes the `initialize`
handshake within a deadline, matches answers to requests by id, streams
notifications (turn items, message deltas) to subscribers, and forwards
`turn/interrupt`. Every exchange, the write included, is held to one
deadline. A child that crashes, or that stops reading its input while it
keeps its output open, fails the requests in flight with a transport error
and is started again on the next request after a bounded exponential
backoff; a request the app-server sends while nobody is listening for it is
refused rather than left waiting; shutting the gateway down kills the
child. Nothing in codex's own tool surface (shell, file edits, MCP
servers) is exposed: only tools from Nolune's capability and approval
layer run, as the next sections say.

### The provider

`LlmProvider::Codex` has no API key: `key_for` is `None`, the config's
`auth_state_for` answers `login`, and a Codex preset is complete as far as
the config is concerned. Whether a login is there is runtime state, read
from the app-server (`account/read`) before every turn and never stored;
without one a turn fails with a typed setup error that says to log in.
`codex-sol` and `codex-luna` are the seeded presets, on the models the
pinned release lists.

Each conversation runs in one app-server thread. The first turn starts it
(`thread/start`) and the id is kept in the chat's `meta.json`
(`codex_thread_id`), so a restart of Nolune, or of the child, resumes the
same thread (`thread/resume`) and the conversation continues where it was;
a thread codex no longer has is started again, and the new one hears the
conversation so far. One-shot runs (titles, memory extraction, the
connection test) use ephemeral threads.

### Sandbox and safety

A thread is started read-only (`sandbox: read-only`), with no approvals
(`approvalPolicy: never`), with no environment (`environments: []`,
repeated on every `turn/start` because `thread/resume` does not keep it),
with codex's own shell, file, web search, browser, plugin, hook, goal,
skill, user-input and sub-agent surfaces switched off in the thread's
config, and with Nolune's tool definitions as `dynamicTools`: the only
tools the model can call. That is checked against the pinned release by
the tool list the model is actually sent (the release checklist says how),
because some switches are not where their names suggest:
`tools.web_search = false` parses and is dropped, which leaves cached web
search on, so the thread sets the top-level `web_search = "disabled"`;
`apply_patch` comes with the model rather than a feature flag, and only
the empty environment list takes it away; the skills catalog is both a
block of instructions (`skills.include_instructions`) and, on a thread
without an environment, a tool namespace (`orchestrator.skills`). The MCP servers in the user's own codex config need more than a
switch: the app-server merges the thread's config overrides into
`config.toml` per key, so an empty `mcp_servers` table disables nothing
(verified against the pinned release: every configured server still
started for the thread). Before a start or a resume the adapter therefore
reads the effective config (`config/read`) and disables every server it
lists by name (`mcp_servers.<name>.enabled = false`), and before every
turn it lists the thread's servers (`mcpServerStatus/list`) and refuses
the thread if any stands other than disabled; a server the app-server
announces for the thread mid-turn (`mcpServer/startupStatus/updated`)
interrupts and fails the turn. A refused conversation thread is attached
anew on the next turn, overrides re-applied and checked again. Threads run
in an empty directory of Nolune's own (`<workspace>/codex/cwd` for a
conversation, a scratch directory of the runtime's for a one-shot), never
in the workspace itself, so nothing rooted at the thread's directory (a
project doc, a skill, a file mention) reaches Nolune's config, chats or
memory.

What this does not do: a `read-only` sandbox still lets codex read the
filesystem at large, as it would for any codex user on that machine. The
feature flags, the disabled servers and the item guard below keep codex
from running anything; they do not narrow what the model may be shown by
a tool Nolune itself grants.

### Tool bridge, cancellation and errors

When codex asks `item/tool/call`, the adapter hands the call to the agent
loop as an ordinary tool call and leaves the turn open; the loop runs the
tool through Nolune's capability and approval layer and the adapter
answers codex with the result. While the turn waits, a task of its own
keeps reading the app-server's event stream and queues only that thread's
events, so a second conversation or a background routine streaming
meanwhile never overruns it. Codex itself executes nothing: any approval
it asks for is declined, and a command, file change, web search, image
read or MCP call it starts on its own fails the turn. Dynamic tools are
fixed when a thread starts, so a conversation whose tool set changes
continues in a fresh thread. A one-shot's ephemeral thread is
unsubscribed (`thread/unsubscribe`) once its turn is over, and the
app-server unloads it after its own delay; `thread/archive` and
`thread/delete` do not apply to an ephemeral thread (it has no rollout).

Cancellation sends `turn/interrupt`. A child that dies mid-turn fails that
turn with a transport error and the next turn resumes the thread in the
replaced child. A failed turn maps its `codexErrorInfo` to the same typed
errors the other adapters produce (rate limits, context length,
authentication, upstream status). The Codex model cannot be sent images or
documents through Nolune yet (its capabilities say so), and codex keeps its
own conversation history, so only the new message goes out per turn.

The login routes, `GET /api/config/codex/status`,
`POST /api/config/codex/login` (the managed browser flow, or a device code
for a headless server) and `POST /api/config/codex/logout`, are described
in [settings.md](settings.md); they ask the same app-server child the
provider's turns run on, over the same protocol, and hand out an account's
label and what a person needs to finish a login, never a token. The Codex
section of Settings → Connections and the Codex choice in onboarding are
built on them (settings.md says what each state shows and how the
onboarding gate works). Moving the pin is a release step: the
[Release checklist](release-checklist.md) says how to re-record the fixture
and which tests to run before `CODEX_VERSION` changes.

## Capabilities per provider

`Capabilities` in `services/llm/contract.rs` is what an adapter reports
and what a request is validated against before the network: a request
that needs something the preset's model lacks is refused with
`UnsupportedCapability`, and the settings page shows the same flags as
chips. This table is checked against the adapters' constants by
`server/tests/provider_docs.rs`.

| Capability | Anthropic | OpenAI | OpenRouter | Codex |
| --- | --- | --- | --- | --- |
| `vision` | yes | yes | per model from the catalog (yes until it says otherwise) | no |
| `documents` | yes (PDF) | yes (PDF) | no | no |
| `tools` | yes | yes | per model from the catalog (yes until it says otherwise) | yes |
| `streaming` | yes | yes | yes | yes |
| `reasoning_controls` | no | per model: the GPT-5 family and the o-series | per model from the catalog (yes until it says otherwise) | no |
| `model_discovery` | no | no | yes (`/api/v1/models`, cached an hour) | no |
| `token_counting` | yes (`count_tokens`) | no (local estimate) | no (local estimate) | no (local estimate) |

Differences that are not capability flags:

| Behaviour | Anthropic | OpenAI | OpenRouter | Codex |
| --- | --- | --- | --- | --- |
| Prompt caching | by scope: 1h conversation, 5m one-shots | the API's own, no lifetime to choose | each vendor's own | none to choose |
| Cost per turn | not reported | not reported | `usage.cost` in USD | not reported |
| Retry hint on a rate limit | `Retry-After` header | `Retry-After` header | `Retry-After` header | none in the pinned protocol; the backoff applies |
| Conversation history | sent in full each turn, compacted by Nolune | sent in full each turn | sent in full each turn | kept by codex; the new message only |
| Runs where | api.anthropic.com | api.openai.com | openrouter.ai | a local process on the server machine |

## Errors every adapter reports the same way

Callers match on `LlmError` variants, never on status strings
(`server/tests/provider_boundary.rs` keeps it so):

| Variant | Anthropic | OpenAI | OpenRouter | Codex |
| --- | --- | --- | --- | --- |
| `Authentication` | 401, 403, `authentication_error`, `permission_error` | 401, 403 | 401, 403 | `unauthorized`, an upstream 401 or 403 |
| `RateLimited { retry_after }` | 429, 529, `rate_limit_error`, `overloaded_error` | 429 | 429 | `usageLimitExceeded`, `rateLimitExceeded`, `serverOverloaded`, an upstream 429 |
| `ContextLength` | 400 "prompt is too long" | 400 `context_length_exceeded` | 400 naming the context length | `contextWindowExceeded`, `sessionBudgetExceeded` |
| `Http { status }` | any other status | any other status | any other status, including "not a valid model ID" and "Insufficient credits" | `badRequest`, `internalServerError`, any other upstream status |
| `Transport` | connection failures | connection failures | connection failures | a child that died, a stream that closed |
| `Timeout` | a probe past its deadline | same | same | a turn with no event for ten minutes |
| `InvalidResponse` | a body or event that is not the protocol, a malformed tool call | same | same | an out-of-protocol turn end, a malformed `item/tool/call`, an item codex ran on its own |
| `SetupRequired` | a slot on a provider without a key is refused before a backend exists (`setup_required`) | same | same | no login, read before every turn |

A rate limit is retried by `retry_on_rate_limit` with the provider's
`Retry-After` when it is at most sixty seconds, otherwise 2 s, 4 s, 8 s;
nothing else is retried. A conversation shows a friendly rate-limit reply
and names the variant for the others.

## How the adapters are tested

Every provider is exercised in CI without credentials and without the
network, through the conformance harness in
`server/test-support/llm_conformance.rs`: one set of cases, run for every
provider. The HTTP adapters answer in-process mock servers that return
the provider's own wire shapes (SSE for streams, JSON for completions,
status codes and headers for errors); Codex answers a fake app-server that
plays `fixtures/codex-0.156.1.jsonl`, the answers and events recorded from
the pinned release. Each case runs once per provider it applies to:

- completion with a tool call and usage, structured output, and the
  canonical stream events (text deltas, tool call start and argument
  deltas, usage);
- the tool-call round trip through the agent loop, with every result
  next to its call in the provider's own shape;
- malformed tool calls (missing or wrong-typed id, name or arguments) in
  both modes, refused before any tool runs;
- malformed events: a body or a stream event that is not the protocol;
- authentication (401, 403), rate limits with and without a `Retry-After`,
  context overflow, and the remainder as `Http`;
- cancellation before the network and mid-stream, the key probe, the
  connection test and its deadline.

Adding a provider means an adapter, its fixtures in the harness, one line
in `PROVIDERS`, a row in the tables above and a section on this page; the
guard tests fail until each is there. Tests that spend real credentials
are `#[ignore]`: the `network_*` tests need a key in the environment, the
Codex live tests need `NOLUNE_CODEX_LIVE=1`, a `codex` on `PATH` at the pin
and, for a real turn, a login in codex's home; the Codex smoke test
(`codex_smoke_speaks_the_pinned_protocol_with_the_installed_binary`) uses
an empty scratch `CODEX_HOME` and so never touches yours.
