# nolune

A small agent that lives on your computer and does things for your family. One gateway serves a
web chat; family members share profiles with their own conversations, skills and memory. The agent
has one tool, `run_command`, with full access to the computer. See [DESIGN.md](DESIGN.md) for how
it works and why.

> **It can do anything your user account can.** Everyone you add can ask it to read, change or
> delete your files. Only add people you trust, and keep the web address behind a login you control.

## Install

Needs macOS (Linux works without the background service), Node 22.18+ and an
[Anthropic API key](https://console.anthropic.com/), an
[OpenAI API key](https://platform.openai.com/api-keys), an
[OpenRouter API key](https://openrouter.ai/settings/keys) (chats run on Claude, on OpenAI's GPT
models, or on any model OpenRouter serves that can call tools; you can have all three), a model
server of your own (Ollama, LM Studio, oMLX, vLLM...) with a model that can call tools, a Claude
Pro or Max plan signed in to [Claude Code](https://claude.com/claude-code) on the same computer, or
a ChatGPT Plus, Pro or Business plan signed in to OpenAI's
[Codex](https://developers.openai.com/codex/cli) there (see below).

```sh
npm install -g nolune
nolune setup                   # your account and the public URL
nolune key set openai          # optional: GPT models for chats, and pictures (Images page)
nolune service install         # run in the background, start at login
nolune user create Anna anna@example.com   # add family members (prints their password)
```

Then open the address `nolune setup` printed, sign in and make a profile: its welcome asks for a
model, on an API key or a plan. As the admin you can also add or replace API keys, sign in with
ChatGPT, and add models on the web, under Models & keys in your account menu.

**Through OpenRouter.** With `nolune key set openrouter`, a preset can run any model
[OpenRouter](https://openrouter.ai/models) serves that can call tools:
`nolune preset add deepseek/deepseek-v4.1-flash --provider openrouter`. OpenRouter's ids name the
model's maker. Pictures and PDFs go only to models that take them; for the others, they're saved
for the agent and named in the message, like any other file.

**On your own servers.** Chats can run on model servers of your own, like
[Ollama](https://ollama.com) or [LM Studio](https://lmstudio.ai) on this computer, or vLLM on a
machine with GPUs. Add each one as a custom provider, with the name you want to see, under Models &
keys (Add custom provider, under the API keys) or with
`nolune provider add Ollama http://localhost:11434` (`--key` if it wants one). It speaks OpenAI's API
unless you pass `--api anthropic` (Ollama, LM Studio and oMLX have both; llama.cpp's server only
Anthropic's). Then it's a provider like the others: `nolune preset add qwen3:8b --provider Ollama`,
or its chip in Add a model. nolune asks it for its models to check it. The model must be able to call
tools; pictures and PDFs reach it as their paths.

**On your own plan instead of an API key.** Chats can run on a subscription someone in the family
already has. Pick one in a new profile's welcome, or with `nolune <plan> setup` and a preset on
that provider (`nolune preset add claude-opus-5-5 --provider claude-plan`, or on Models &
keys). `nolune <plan> status` says who the plan is signed in as. Plan limits assume one person's
ordinary use: keep busy automations and subagents on an API key preset. See
[DESIGN.md](DESIGN.md#plans) for what works differently.

Either way, nolune runs the plan maker's own agent on this computer, unmodified, which keeps the
sign-in and uses the plan's limits; nolune never sees the sign-in. Without the agent, setup offers
to install it, asking first, so run it in a terminal on this computer.

- **`claude-plan`: Claude Pro or Max.** Chats run through
  [Claude Code](https://claude.com/claude-code), signed in to your Claude account. Setup installs
  it with Anthropic's installer and starts Claude Code's own sign-in, in your browser. Anthropic
  [counts this](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan)
  as Agent SDK use of your subscription.
- **`chatgpt-plan`: ChatGPT Plus, Pro or Business.** Chats run through OpenAI's
  [Codex](https://developers.openai.com/codex/cli) (`npm install -g @openai/codex`), through the
  [app server](https://developers.openai.com/codex/app-server) Codex's own IDE extension uses, on
  the plan's Codex limits. Setup, or Sign in with ChatGPT under Models & keys, has Codex show a
  link and a one-time code: open the link on any device, sign in to ChatGPT and enter the code.
  Codex keeps that sign-in in a home of its own for nolune (`~/.nolune/codex`), apart from yours
  in `~/.codex`. `nolune chatgpt-plan models` lists what the plan offers, and
  `nolune chatgpt-plan logout` signs Codex out. Codex takes no PDFs, so the model gets their path,
  which it opens with commands.

**Reaching it from outside your home.** The gateway listens on `127.0.0.1:5780`. Put a tunnel in
front of it, e.g. [Tailscale Funnel](https://tailscale.com/kb/1223/funnel),
[Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/),
or your own VPS, and tell nolune the public URL:

```sh
nolune config set origin https://nolune.example.com
nolune service restart
```

**Files in Documents, Desktop, Photos, Mail.** macOS blocks background processes from these until
you give the `node` binary Full Disk Access (System Settings > Privacy & Security > Full Disk
Access). `nolune setup` prints the exact path.

**Keep the Mac awake** if people should reach it at any time (System Settings > Energy).

## Using it

- `nolune help` lists every command: users, model presets, profiles, skills, settings. Admins can also
  ask nolune in a chat to change these ("use Sonnet for new chats", "add an account for Grandma"); the
  built-in `nolune` skill tells it how, and it restarts itself only when asked.
- The chat keeps what nolune did folded under each reply ("Worked for 12s"), with plain-language
  steps. To see the exact commands, token usage and prompt caching, turn on **Show technical
  details** in Settings (click your name at the bottom of the sidebar).
- **Models.** The chip in the message box picks the model and how long it thinks, in a new chat or
  an existing one. After a change the next reply reads the whole chat again (it isn't cached for
  the new setting yet), so it's slower and costs more once; the chat asks before that happens.
- Attach files to a message with the paperclip, by pasting, or by dropping them on the message box.
  nolune sees pictures and PDFs, and every file is saved in the profile's `attachments` folder for it
  to work with.
- nolune can look at photos, screenshots and scans on the computer (with `nolune view`, which converts
  HEIC and shrinks big photos for it), and it can show pictures and hand over files in the chat
  ("show me the beach photos from August", "fill in this form and give it to me"). It keeps its
  own copy of each file it shows, so they stay in the chat even if the original moves.
- **Images.** The Images page in the sidebar has templates for the family (party invitations,
  storybook pages, wanted posters, trip postcards, photo-booth strips, sticker packs, bouquets, nail
  art...) and for making games (sprite sheets, textures, app icons, concept art): tap one and take
  or choose a photo, draw something, or press Try it; many ask for a few choices first. nolune starts a
  new chat with the picture and the template's prompt, makes the picture with `nolune generate image`
  and shows it there, where you can ask for changes. You can also just describe a picture, there or
  in any chat. It needs an OpenAI key (Models & keys, or `nolune key set openai`); the model is
  `openai/gpt-image-2.5-flare` unless you pick another with `nolune config set image-model`. Templates
  are folders with a `TEMPLATE.md`; add your own in `~/.nolune/image-templates` or a profile's
  `image-templates` folder (see DESIGN.md).
- **Folders.** Keep related chats together, like projects in ChatGPT: make one with **New
  folder** in the sidebar, give it instructions and files on its page, and every chat in it gets
  them (files as paths on the computer, which nolune opens when they matter). Start a chat in a
  folder from its page or the folder chip in the composer, or drag chats onto a folder in the
  sidebar. Moving a chat makes its next reply re-read the conversation once. Files:
  `~/.nolune/profiles/<profile>/folders`.
- Skills live in `~/.nolune/profiles/<profile>/skills` and `~/.agents/skills`
  ([Agent Skills](https://agentskills.io) format). The agent creates its own with `nolune skill new`.
  Each skill's name and description go into every new chat, so turn off the ones a profile doesn't
  need on its **Skills** page (or `nolune skill disable <name> --profile <slug>`).
- **A new profile** starts with a short welcome: pick the assistant's avatar, and bring over what
  ChatGPT, Claude or Gemini already knows about you (copy a prompt there, paste the answer
  back). On a fresh install without a model, an admin picks one there too. Its sounds can be
  turned off in Settings.
- **Memory.** nolune keeps what it learns about the family (preferences, who's who, where things are)
  in small notes, in the same categories in every profile: core, people (a note per person, in
  the family or not), home, health, plans, routines, pets, places, projects and other. Each member
  has their note under people, so nolune knows who "I" is; when you add someone memory may know
  already (grandma, before she got an account), you're asked which note is theirs. Each message comes with the facts from memory that match it, and nolune
  searches for more when a request needs them (`nolune memory search wifi`). With an OpenAI or
  OpenRouter key, it also finds facts by meaning ("where's the other key for the car?" finds the
  spare key, a question in Russian finds notes in English): each fact is embedded once with that
  provider's `text-embedding-3-small`. An admin can turn that off, or use a model of one of your
  servers instead (Ollama, LM Studio, oMLX on your computer), under Memory search in Models &
  keys or with `nolune config set embeddings custom-openai/local/nomic-embed-text`. The pinned
  `core` note (who's who, languages, allergies, anything you want it to always keep in mind) is in
  every chat from the start, so keep it short: at most 4,000 characters. Besides what nolune saves
  as it goes, it looks over each chat once it has been quiet for a couple of minutes and saves
  what it missed, with one short request to the chat's model; turn that off on the Memory page
  (**Learn from chats**) or with `nolune memory learning off`. What it saves shows in the chat
  ("Saved 3 memories") and at the top of the Memory page, each with Undo. The Memory page shows
  every fact as a dot, darker the newer it is, and lets you fix, move, merge or delete a note
  (`nolune memory merge people/grandma people/olga` when two are about one person). Files:
  `~/.nolune/profiles/<profile>/memories`.
- **Languages.** The web interface comes in English, Russian, German, Spanish and French. It
  follows the browser's language, or pick one in Settings (per device). Only menus, buttons and
  pages change: nolune answers in whatever language you write in, whatever the setting.
- **Soul.** Each profile can tell nolune who to be for its family: its character, what it cares about,
  how it talks. Write it under **People & profile**, or just ask nolune to be different and it updates
  its own soul (`~/.nolune/profiles/<profile>/soul.md`). Every chat starts with it.
- **Automations.** Ask nolune in a conversation ("every weekday at 7:30, tell us if we need umbrellas",
  "check my email every 10 minutes and tell me when the school writes"). It sets up a trigger that
  runs in the background, and what it finds shows up under the bell at the top; open a notification
  to continue it as a conversation. Each profile's Automations page lists them. From the terminal:
  `nolune trigger list`.
- **Background work and subagents.** nolune can run a long command in the background and carry on;
  its output comes back to the chat when it's done. For big or parallel jobs it starts subagents
  with `nolune agent run` (the built-in `subagents` skill explains when): each works in a hidden
  conversation of its own that starts with only its task, and reports back to the chat. While they
  work, the chat lists them under "Working in the background", where you can open a subagent's own
  chat or stop everything. Logs: `~/.nolune/profiles/<profile>/agents`.
- Extra environment variables for the agent's commands, e.g. for a firecrawl web-search skill:
  `nolune env set FIRECRAWL_API_KEY fc-...`
- Logs: `nolune service logs -f`. Data: `~/.nolune` (override with `NOLUNE_HOME`).
- Update: `npm install -g nolune@latest && nolune service restart`.

## Development

Requires pnpm. Node runs the TypeScript in `packages/` directly (type stripping), so no build step
is needed for the CLI while developing.

```sh
pnpm install
pnpm nolune setup          # same CLI, from source
cp .env.example .env    # ORIGIN=http://localhost:5173
pnpm dev
```

```sh
pnpm check                          # svelte-check + tsc for packages/core and packages/cli
pnpm lint                           # prettier --check + eslint (pnpm format to fix formatting)
pnpm test                           # vitest: *.test.ts next to the code in src/ and packages/
pnpm db:generate --name <change>    # after editing packages/core/src/db/schema.ts
pnpm build                          # web build + dist/cli.js (what the npm package ships)
pnpm start                          # run the built gateway with the settings from nolune config
```

Migrations are applied automatically when the gateway or the CLI opens the database.

Every test starts with an empty `NOLUNE_HOME` in a temp folder (`packages/core/src/test/setup.ts`), so
tests of the database code run against a fresh, migrated SQLite file and never touch your data.

CI (`.github/workflows/ci.yml`) runs format, lint, types, tests and the build on every pull request
and on pushes to `main`.

### Publishing

`npm pack` builds and packs `build/` (without source maps), `dist/cli.js` and the migrations.

To release, set `version` in `package.json`, merge it to `main`, then push a matching tag:
`git tag v0.1.0 && git push origin v0.1.0`. The Publish workflow (`.github/workflows/publish.yml`)
checks the tag against `package.json`, runs format, lint, types and tests, and publishes with
`npm publish`. It signs in to npm with
[trusted publishing](https://docs.npmjs.com/trusted-publishers/), so there's no npm token in the
repository's secrets, and npm adds provenance on its own.

Once, by hand: publish the first version from your computer (`npm login`, then `npm publish`), since
a trusted publisher is set on a package that exists. Then, on npmjs.com under the package's
Settings, add a trusted publisher (GitHub Actions, this repository, `publish.yml`) that may publish
with `npm publish`, and under Publishing access require two-factor authentication and disallow
tokens.

## License

[MIT](LICENSE)
