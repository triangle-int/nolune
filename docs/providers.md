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
| codex app-server (#27, in progress) | a ChatGPT login held by the local `codex` binary | `services/llm/codex/` |

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

What ships today is this process layer and the login behind it:
`GET /api/config/codex/status`, `POST /api/config/codex/login` (the managed
browser flow, or a device code for a headless server) and
`POST /api/config/codex/logout`, described in [settings.md](settings.md);
they ask the app-server over the same protocol and hand out an account's
label and what a person needs to finish a login, never a token. The
provider itself, with presets naming a codex model and a login tile in
Settings → Connections, follows in later slices of #27.
