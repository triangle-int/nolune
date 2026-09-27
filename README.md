# btw

A small agent that lives on your computer and does things for your family. One gateway serves a
web chat; family members share profiles with their own conversations, skills and memory. The agent
has one tool, `run_command`, with full access to the computer. See [DESIGN.md](DESIGN.md) for how
it works and why.

> **It can do anything your user account can.** Everyone you add can ask it to read, change or
> delete your files. Only add people you trust, and keep the web address behind a login you control.

## Install

Needs macOS (Linux works without the background service), Node 22.18+ and an
[Anthropic API key](https://console.anthropic.com/), an
[OpenAI API key](https://platform.openai.com/api-keys) (chats run on Claude or on OpenAI's GPT
models; you can have both), or a Claude Pro or Max plan signed in to
[Claude Code](https://claude.com/claude-code) on the same computer (see below).

```sh
npm install -g btw-agent
btw setup                      # API key, your account, default model, public URL
                               # (btw setup --provider openai to start with GPT)
btw key set openai             # optional: GPT models for chats, and pictures (Images page)
btw service install            # run in the background, start at login
btw user create Anna anna@example.com   # add family members (prints their password)
```

Then open the address `btw setup` printed and sign in. As the admin you can also add or replace
API keys and models on the web, under Models & keys in your account menu.

**On your Claude plan instead of an API key.** Run `btw setup --provider claude-plan`. Chats then
run through [Claude Code](https://claude.com/claude-code) on this computer, unmodified, signed in
to your Claude account, which uses your plan's limits; btw never sees the sign-in. Without Claude
Code, setup offers to install it with Anthropic's installer and then to sign it in (Claude Code's
own sign-in, in your browser), asking before each. `btw claude-plan setup` does the same later, for
a preset added with `btw preset add claude-opus-5-5 --provider claude-plan` (or on Models & keys),
and `btw claude-plan status` shows who Claude Code is signed in as. Anthropic
[counts this](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan)
as Agent SDK use of your subscription, and plan limits assume one person's ordinary use: keep busy
automations and subagents on an API key preset, and see [DESIGN.md](DESIGN.md#the-claude-plan)
for what works differently.

**Reaching it from outside your home.** The gateway listens on `127.0.0.1:5780`. Put a tunnel in
front of it, e.g. [Tailscale Funnel](https://tailscale.com/kb/1223/funnel),
[Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/),
or your own VPS, and tell btw the public URL:

```sh
btw config set origin https://btw.example.com
btw service restart
```

**Files in Documents, Desktop, Photos, Mail.** macOS blocks background processes from these until
you give the `node` binary Full Disk Access (System Settings > Privacy & Security > Full Disk
Access). `btw setup` prints the exact path.

**Keep the Mac awake** if people should reach it at any time (System Settings > Energy).

## Using it

- `btw help` lists every command: users, model presets, profiles, skills, settings. Admins can also
  ask btw in a chat to change these ("use Sonnet for new chats", "add an account for Grandma"); the
  built-in `btw-agent` skill tells it how, and it restarts itself only when asked.
- The chat keeps what btw did folded under each reply ("Worked for 12s"), with plain-language
  steps. To see the exact commands, token usage and prompt caching, turn on **Show technical
  details** in Settings (click your name at the bottom of the sidebar).
- **Models.** The chip in the message box picks the model and how long it thinks, in a new chat or
  an existing one. After a change the next reply reads the whole chat again (it isn't cached for
  the new setting yet), so it's slower and costs more once; the chat asks before that happens.
- Attach files to a message with the paperclip, by pasting, or by dropping them on the message box.
  btw sees pictures and PDFs, and every file is saved in the profile's `attachments` folder for it
  to work with.
- btw can look at photos, screenshots and scans on the computer (with `btw view`, which converts
  HEIC and shrinks big photos for it), and it can show pictures and hand over files in the chat
  ("show me the beach photos from August", "fill in this form and give it to me"). It keeps its
  own copy of each file it shows, so they stay in the chat even if the original moves.
- **Images.** The Images page in the sidebar has templates for the family (party invitations,
  storybook pages, wanted posters, trip postcards, photo-booth strips, sticker packs, bouquets, nail
  art...) and for making games (sprite sheets, textures, app icons, concept art): tap one and take
  or choose a photo, draw something, or press Try it; many ask for a few choices first. btw starts a
  new chat with the picture and the template's prompt, makes the picture with `btw generate image`
  and shows it there, where you can ask for changes. You can also just describe a picture, there or
  in any chat. It needs an OpenAI key (Models & keys, or `btw key set openai`); the model is
  `openai/gpt-image-2.5-flare` unless you pick another with `btw config set image-model`. Templates
  are folders with a `TEMPLATE.md`; add your own in `~/.btw-agent/image-templates` or a profile's
  `image-templates` folder (see DESIGN.md).
- **Folders.** Keep related chats together, like projects in ChatGPT: make one with **New
  folder** in the sidebar, give it instructions and files on its page, and every chat in it gets
  them (files as paths on the computer, which btw opens when they matter). Start a chat in a
  folder from its page or the folder chip in the composer, or drag chats onto a folder in the
  sidebar. Moving a chat makes its next reply re-read the conversation once. Files:
  `~/.btw-agent/profiles/<profile>/folders`.
- Skills live in `~/.btw-agent/profiles/<profile>/skills` and `~/.agents/skills`
  ([Agent Skills](https://agentskills.io) format). The agent creates its own with `btw skill new`.
  Each skill's name and description go into every new chat, so turn off the ones a profile doesn't
  need on its **Skills** page (or `btw skill disable <name> --profile <slug>`).
- **Memory.** btw keeps what it learns about the family (preferences, who's who, where things are)
  in small notes per topic, and reads the ones it needs when a chat starts. The pinned `core` note
  (who's who, languages, allergies, anything you want it to always keep in mind) is in every chat
  from the start, so keep it short: at most 4,000 characters. The profile's Memory
  page shows every fact as a dot, darker the newer it is, and lets you fix or delete a note. Files:
  `~/.btw-agent/profiles/<profile>/memories`.
- **Soul.** Each profile can tell btw who to be for its family: its character, what it cares about,
  how it talks. Write it under **People & profile**, or just ask btw to be different and it updates
  its own soul (`~/.btw-agent/profiles/<profile>/soul.md`). Every chat starts with it.
- **Automations.** Ask btw in a conversation ("every weekday at 7:30, tell us if we need umbrellas",
  "check my email every 10 minutes and tell me when the school writes"). It sets up a trigger that
  runs in the background, and what it finds shows up under the bell at the top; open a notification
  to continue it as a conversation. Each profile's Automations page lists them. From the terminal:
  `btw trigger list`.
- **Background work and subagents.** btw can run a long command in the background and carry on;
  its output comes back to the chat when it's done. For big or parallel jobs it starts subagents
  with `btw agent run` (the built-in `subagents` skill explains when): each works in a hidden
  conversation of its own that starts with only its task, and reports back to the chat. While they
  work, the chat lists them under "Working in the background", where you can open a subagent's own
  chat or stop everything. Logs: `~/.btw-agent/profiles/<profile>/agents`.
- Extra environment variables for the agent's commands, e.g. for a firecrawl web-search skill:
  `btw env set FIRECRAWL_API_KEY fc-...`
- Logs: `btw service logs -f`. Data: `~/.btw-agent` (override with `BTW_HOME`).
- Update: `npm install -g btw-agent@latest && btw service restart`.

## Development

Requires pnpm. Node runs the TypeScript in `packages/` directly (type stripping), so no build step
is needed for the CLI while developing.

```sh
pnpm install
pnpm btw setup          # same CLI, from source
cp .env.example .env    # ORIGIN=http://localhost:5173
pnpm dev
```

```sh
pnpm check                          # svelte-check + tsc for packages/core and packages/cli
pnpm lint                           # prettier --check + eslint (pnpm format to fix formatting)
pnpm test                           # vitest: *.test.ts next to the code in src/ and packages/
pnpm db:generate --name <change>    # after editing packages/core/src/db/schema.ts
pnpm build                          # web build + dist/cli.js (what the npm package ships)
pnpm start                          # run the built gateway with the settings from btw config
```

Migrations are applied automatically when the gateway or the CLI opens the database.

Every test starts with an empty `BTW_HOME` in a temp folder (`packages/core/src/test/setup.ts`), so
tests of the database code run against a fresh, migrated SQLite file and never touch your data.

CI (`.github/workflows/ci.yml`) runs format, lint, types, tests and the build on every pull request
and on pushes to `main`.

### Publishing

`npm pack` builds and packs `build/`, `dist/cli.js` and the migrations. Before the first
`npm publish`: pick a license, add `license` and `repository` to `package.json`, and remove
`"private": true`.
