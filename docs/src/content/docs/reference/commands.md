---
title: Commands
description: Everything the nolune command does, as nolune help prints it.
---

`nolune help` prints this list for the version you have. Admins can also ask nolune in a chat to
change these ("use Sonnet for new chats", "add an account for Grandma"): the built-in `nolune`
skill tells it how, and it restarts itself only when asked.

## Getting started

- `nolune setup`: interactive first-time setup (your account, public URL); the model comes after: a
  new profile's welcome on the web asks for one, or use key set and preset add below
- `nolune start`: run the gateway in the foreground
- `nolune service install|uninstall|restart|status|logs [-f]`: run it in the background (macOS, or
  Linux with systemd)

## Settings

- `nolune config`: show address, port and what's configured
- `nolune config set <host|port|origin> <value>`: origin = the public URL people open
- `nolune config set image-model <provider/model>`: for pictures, e.g. openai/gpt-image-2.5-flare
- `nolune config set embeddings <auto|off|provider/model>`: what memory search finds meaning with:
  auto uses the OpenAI key, else OpenRouter's; the provider is openai, openrouter or custom-openai
  (a custom provider's model, like custom-openai/ollama/nomic-embed-text)
- `nolune config set claude-path <path>`, `nolune config set codex-path <path>`: the Claude Code
  that claude-plan chats run, and the Codex that chatgpt-plan chats run (found on the PATH and in
  their usual folders otherwise)
- `nolune config set command-mode <auto|unrestricted>`: auto (the default): a model checks each
  command the agent runs and blocks what could do harm nobody asked for; unrestricted runs them
  unchecked (not recommended). Not from the agent's own commands
- `nolune config set safety-model <preset|chat>`: the preset whose model does auto mode's checks, or
  chat for each chat's own model (the default)
- `nolune key set <anthropic|openai|openrouter> [key]`: store an API key (prompts if omitted) after
  checking it; OpenAI's runs GPT chats and makes pictures. Admins can also do this on the web, under
  Models & keys
- `nolune key rm <anthropic|openai|openrouter>`: remove a stored key (the environment's is used, if
  set)
- `nolune env set <NAME> <value>`: extra env var for agent commands (e.g. FIRECRAWL_API_KEY)
- `nolune env rm <NAME> | nolune env list`

## Custom providers

Model servers of your own: Ollama, LM Studio, oMLX, vLLM, llama.cpp.

- `nolune provider add <name> <url> [--api openai|anthropic] [--key K]`: add one, through its OpenAI
  API (the default) or its Anthropic API, or change the address and key of the one of that name;
  nolune asks it for its models first, and for its key at a terminal when it wants one. The address
  is the server's, like http://localhost:11434. A server that speaks both can be added once for each
- `nolune provider rm <name|id>`: presets on it stop working until they're moved
- `nolune provider list`

## Plans

Chats on your own subscription instead of an API key.

claude-plan: a Claude Pro or Max plan, through Claude Code on this computer, signed in to your
Claude account. chatgpt-plan: a ChatGPT Plus, Pro or Business plan, through OpenAI's Codex on this
computer, signed in with ChatGPT. nolune never sees either sign-in: the agent keeps it. Plan limits
assume one person's ordinary use: keep busy automations and subagents on an API key.

- `nolune <plan> status`: which Claude Code or Codex nolune runs, and who it's signed in as
- `nolune <plan> setup`: install it and sign in, where needed. Installing asks first, in a terminal;
  claude-plan signs in there too, chatgpt-plan with a link and a code for any device
- `nolune chatgpt-plan logout`: sign Codex out; chats on chatgpt-plan presets stop until someone
  signs in again
- `nolune chatgpt-plan models`: the models the plan offers, for `nolune preset add`

## Users

Web sign-up is disabled; this is the only way to add people.

- `nolune user create <name> <email> [--password P] [--admin]`
- `nolune user passwd <name|email> [--password P]`
- `nolune user admin <name|email> [--off]`
- `nolune user rm <name|email>`
- `nolune user list`

## Model presets

Shared by all profiles.

- `nolune preset add <model> [--provider
anthropic|openai|openrouter|claude-plan|chatgpt-plan|<custom>] [--name N] [--context-window
TOKENS]`: the provider checks the model id first (anthropic unless given); OpenAI models other
  than the flagships need `--context-window`. OpenRouter's ids name their maker
  (anthropic/claude-sonnet-5), and the model must be able to call tools. A custom provider (by its
  name) lists its models; its model must call tools too, which shows at its first reply, and
  pictures and PDFs reach it as paths. The plans check their agent's sign-in instead, and
  chatgpt-plan the models Codex offers
- `nolune preset edit <name|id> [--provider P] [--model M] [--name N] [--context-window
TOKENS|auto]`: change what's given; a new model is checked like add's. Chats already on the preset
  keep what they had
- `nolune preset rm <name|id>`
- `nolune preset default <name|id>`: the model new chats start with
- `nolune preset list`

## Profiles and skills

- `nolune profile list`: slug, name and avatar of every profile
- `nolune profile avatar [<name>] [--profile SLUG]`: show or change the assistant's avatar in the
  web chat: probe, campfire, lantern, planet, quantum, comet, moon, satellite
- `nolune skill new <name> [--description D] [--profile SLUG | --global]`
- `nolune skill list [--profile SLUG]`
- `nolune skill enable <name>... [--profile SLUG]`
- `nolune skill disable <name>... [--profile SLUG]`: leave out of the profile's new chats

## Automations

Results show up as notifications in the web UI.

- `nolune trigger add <name> WHEN WHAT [--summary S] [--icon I] [--preset NAME] [--effort LEVEL]
[--profile SLUG]`
  - `WHEN`: `--cron "<min hour day month weekday>"` (local time) | `--at "YYYY-MM-DD HH:MM"` | `--in
30m|2h|1d` | `--webhook`
  - `WHAT`: `--prompt "<what the agent should do>"` | `--script "<shell command, no model>"`
  - `--summary`: one plain sentence the family sees on the Automations page
  - `--icon`: a Lucide icon name for it, like umbrella
- `nolune trigger list [--profile SLUG]`
- `nolune trigger show|run|pause|resume|rm <name|id>`
- `nolune trigger edit <name|id> [--name N] [--summary S] [--icon I] [WHEN] [WHAT] [--preset NAME]
[--effort LEVEL]`
- `nolune wake <message> [--title T] [--profile SLUG]`: start a background agent run now; trigger
  scripts call this (`nolune wake -` reads stdin)

## Memory

Short notes in fixed categories; the agent reads the ones it needs.

- `nolune memory [list] [--profile SLUG]`: the notes, how many facts each holds, and whose they are
- `nolune memory search <words>...`: find facts in every note, best match first
- `nolune memory show <topic>...`: print notes (a topic is home, people/anna, …)
- `nolune memory add <topic> <fact>`: add one fact; the note is created if needed
- `nolune memory replace <topic> <old> <new>`: change text that appears once in the note
- `nolune memory forget <topic> <text>`: remove the one line that contains &lt;text&gt;
- `nolune memory write <topic> [text]`: replace the whole note (the text, or stdin)
- `nolune memory rm <topic>`
- `nolune memory mv <topic> <new-topic>`
- `nolune memory merge <topic> <into-topic>`: put one note into another about the same thing
- `nolune memory learning [on|off]`: whether nolune also saves what it learns by itself, looking
  over each chat once it goes quiet

Categories: core, people/&lt;name&gt;, home, health, plans, routines, pets, places,
projects/&lt;name&gt;, other. Facts go only into these; a note from before them can be read and
rewritten until it's moved. The note core is pinned: every new chat starts with it, so it holds at
most 4000 characters.

Each member also has a card, `cards/<name>`: a note about them that goes with them into all their
profiles (at most 2000 characters). `show`, `search`, `add`, `replace` and `forget` work on it; a
change is refused unless its owner wrote one of the messages the agent is answering.

## Cards

- `nolune card [<name|email>]`: print someone's card, or list everyone's

People edit their own card on its page in the web app (Your card, in the menu under their name).

## Soul

Who nolune is for a profile: character, values, tone; at most 4000 characters.

- `nolune soul [show] [--profile SLUG]`: print it
- `nolune soul write [text]`: replace it (the text, or stdin); every chat gets it from its next
  message
- `nolune soul rm`: remove it

## Pictures

Model openai/gpt-image-2.5-flare; change it with `nolune config set image-model`.

- `nolune generate image <PROMPT | -> [--image FILE]... [--size square|portrait|landscape|auto|WxH]
[--quality Q] [--background auto|transparent|opaque] [--format png|jpeg|webp] [--count N] [--model
PROVIDER/MODEL] [--out DIR|FILE] [--dry-run]`: make pictures from a prompt, or change the
  `--image` ones; `-` reads the prompt from stdin

## Inside agent commands

`NOLUNE_PROFILE` is set, so `--profile` can be left out.

- `nolune view <image>...`: show images to the agent: they're attached to the command's result (HEIC
  and big photos are converted)

## Subagents

In agent commands: agents that work on a task in the background, in a conversation of their own that
starts with only the task.

- `nolune agent run [<id>] --prompt "<task>" [--preset NAME] [--effort LEVEL]`: start a subagent, or
  give one that finished more work; prints its id and its log file (`--prompt -` reads stdin). It
  uses this chat's model and reasoning unless given: `--preset` takes a name from `nolune preset
list`, `--effort` one of low, medium, high, xhigh, max
- `nolune agent watch <id>`: wait until it's done and print its last message; run it with
  run_in_background to be told when it's done
- `nolune agent steer <id> --prompt "<message>"`: message a subagent while it works
- `nolune agent stop <id>`
- `nolune agent list`
