# Model providers

Nolune talks to a model provider through one adapter per provider under
`server/src/services/llm/`. Presets pick the provider and model per
conversation, and a key is entered once per provider in Settings →
Connections; [settings.md](settings.md) describes presets, slots and the
OpenRouter options.

| Provider | Authenticates with | Wire format lives in |
| --- | --- | --- |
| Anthropic | API key | `services/llm/anthropic.rs` |
| OpenAI | API key | `services/llm/openai.rs` |
| OpenRouter (#26) | API key | `services/llm/openrouter.rs` |
| Codex (#27) | a ChatGPT login held by the local `codex` binary | `services/llm/codex/` |

## codex app-server (#27)

A ChatGPT/Codex login is not the OpenAI API, and Nolune does not pretend it
is. Instead the server runs one local `codex app-server` child process and
speaks its stdio protocol: one JSON object per line, JSON-RPC 2.0 shapes
without the `jsonrpc` member. The login stays inside codex's own home
directory; Nolune never reads, copies or logs its tokens.

The protocol is experimental, so it is pinned:

- `CODEX_VERSION` in `server/src/services/llm/codex/mod.rs` names the one
  release Nolune speaks to. Discovery runs `codex --version` and refuses any
  other release with an error that names both versions, rather than a best
  effort against a protocol it was not tested with. The binary comes from
  `NOLUNE_CODEX_BIN` when that is set, else from `PATH`.
- `server/src/services/llm/fixtures/codex-<version>.jsonl` records that
  release's answers and events. The tests run a fake app-server that plays
  the fixture, so CI never needs the real binary; bumping the pin means
  re-recording the fixture.

The supervisor keeps exactly one child. It completes the `initialize`
handshake within a deadline, matches answers to requests by id, streams
notifications (turn items, message deltas) to subscribers, and forwards
`turn/interrupt`. Every exchange, the write included, is held to one
deadline. A child that crashes, or that stops reading its input while it
keeps its output open, fails the requests in flight with a transport error
and is started again on the next request after a bounded exponential
backoff; a request the app-server sends while nobody is listening for it is
refused rather than left waiting; shutting the gateway down kills the
child. Nothing in
codex's own tool surface (shell, file edits, MCP servers) is exposed: only
tools from Nolune's capability and approval layer will run, once the
provider adapter lands.

### The provider

`LlmProvider::Codex` has no API key: `key_for` is `None`, the config's
`auth_state_for` answers `login`, and a Codex preset is complete as far as
the config is concerned. Whether a login is there is runtime state, read
from the app-server (`account/read`) before every turn and never stored;
without one a turn fails with a typed setup error that says to log in.
`codex-astra` and `codex-luna` are the seeded presets, on the models the
pinned release lists.

Each conversation runs in one app-server thread. The first turn starts it
(`thread/start`) and the id is kept in the chat's `meta.json`
(`codex_thread_id`), so a restart of Nolune, or of the child, resumes the
same thread (`thread/resume`) and the conversation continues where it was;
a thread codex no longer has is started again, and the new one hears the
conversation so far. One-shot runs (titles, memory extraction, the
connection test) use ephemeral threads.

A thread is started read-only (`sandbox: read-only`), with no approvals
(`approvalPolicy: never`), with codex's own shell, file, browser, plugin,
hook and sub-agent surfaces switched off in the thread's config, and with
Nolune's tool definitions as `dynamicTools`: the only tools the model can
call. The MCP servers in the user's own codex config need more than a
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

The login routes (status, device-code login, logout) and the Settings →
Connections tile follow in the remaining slices of #27.
