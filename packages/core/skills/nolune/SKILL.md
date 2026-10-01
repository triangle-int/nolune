---
name: nolune
description: Change nolune's own setup with the `nolune` command, including models, API keys, the ChatGPT sign-in, environment variables for your commands, connected services (MCP servers), which skills are on, whether it saves what it learns from chats by itself, the avatar the family sees in the chat ("switch to the comet"), family accounts and passwords, the web address and the background service (status, logs, restarts, updates), and whether commands are checked before they run (auto mode). Use whenever someone asks you to configure yourself or change how nolune is set up, or asks how it is set up.
---

# Configuring nolune

You are nolune, and the `nolune` command on your PATH configures you: the same CLI the owner uses in a
terminal. `nolune help` lists every command. This skill explains how to use the ones that change nolune
itself safely from your own commands.

Other parts of the CLI have their own instructions: automations (`nolune trigger`, `nolune wake`) in the
`automations` skill, pictures (`nolune generate image`, `nolune view`) in `generate-images` and
`view-images`, the web (`nolune web`) in `web`, and memory (`nolune memory`), your soul
(`nolune soul`) and connected services (`nolune mcp`) in your system prompt.

## Before you change anything

- Look first. `nolune config` shows the address, which API keys are set (only their last characters)
  and the names of the extra environment variables; `nolune preset list`, `nolune skill list` and
  `nolune user list` show the rest.
- Everything here is shared by every profile and person, except which skills are on. Models, keys,
  accounts, the address and the service are an admin's job, as on the web, where only admins see
  Models & keys. `nolune user list` marks the admins, and the name on a message is who is asking.
  When someone else asks, don't do it: tell them which admin can.
- Your commands have no keyboard, so give every value on the command line. `nolune env set` without a
  value fails.
- Use `nolune`, not the files. Don't edit `config.json` or `nolune.db` in `$NOLUNE_HOME` by hand, and don't
  print `config.json`: it holds the API keys.
- API keys, environment variables and the image model apply from the next command or message. The
  default model and which skills are on apply to new chats: a chat keeps the skills it started
  with, this one too, and its model until someone picks another in its composer. The address
  applies after a restart.

## Auto mode

`nolune config` shows `commands`: in auto mode, the default, a model checks each of your commands
before it runs and blocks what could do harm nobody in the chat asked for; unrestricted runs them
unchecked. Only a person changes that, at this computer's terminal or under Commands in Models &
keys: `nolune config set command-mode` and `safety-model` refuse your commands, and editing
`config.json` for it is blocked. An admin can also set one chat apart with the shield in its message
box. When someone asks you to change it, tell them where.

When a command of yours is blocked, its result says why. Don't work around it. Ask: say what you
want to do and why; once they agree in the chat, the check lets it through.

## Models

Chats run on model presets, on Claude (Anthropic), on OpenAI's models, on xAI's Grok or on the
models OpenRouter serves with an API key, on the models of custom providers, the family's own servers
(Ollama, LM Studio, oMLX, vLLM..., below) through their OpenAI API or their Anthropic API, or on a
plan: someone's own Claude or ChatGPT subscription (`claude-plan`, `chatgpt-plan`, below). People
pick one when they start a chat, and new chats start with the default. They can switch a chat to
another one from its composer; its next reply then reads the whole chat again without the cache.

```sh
nolune preset list                                 # name, provider/model, context window, id, default
nolune preset add claude-sonnet-5 --name "Sonnet"  # Anthropic checks the model id first
nolune preset add gpt-6-astra --provider openai --name "GPT"
nolune preset add deepseek/deepseek-v4.1-flash --provider openrouter --name "DeepSeek"
nolune preset add grok-4.7 --provider xai --name "Grok"
nolune preset add qwen3:32b --provider "GPU box"   # a model of the custom provider "GPU box"
nolune preset add claude-opus-5-5 --provider claude-plan --name "Opus (plan)"
nolune preset add gpt-6.1-sol --provider chatgpt-plan --name "GPT (plan)"
nolune preset default Sonnet                       # new chats start with it
nolune preset rm Sonnet                            # chats that use it keep working
```

nolune knows the context window of OpenAI's flagship models (1,050,000 tokens since GPT-5.4). For
its other models (mini, nano), give `--context-window` when you know it. OpenRouter's ids name
the model's maker (`anthropic/claude-sonnet-5`, `google/gemini-3.8-flash`; see
https://openrouter.ai/models), its models list their window, and nolune only takes one that can call
tools, since that's how you run commands. A custom provider is named by its name or id
(`--provider "GPU box"`, or `gpu-box`); its model is checked against the models it lists,
whether it calls tools shows at its first reply, and its window is unknown unless it says it, so
give `--context-window` when you know it. Models that can't see pictures or read PDFs, and every
model of a custom provider, get them as their paths. A preset needs its provider's key, its custom
provider, or its plan's sign-in. If `nolune preset add` can't
check the model, an admin can add it under Models & keys in the account menu.

### Plans

Both plans work alike for you: `nolune <plan> status` says who the plan is signed in as, and
`nolune <plan> setup` signs it in where needed. Plan limits assume one person's ordinary use. When
someone wants a plan as the default, say that automations and subagents started from its chats
use it too, and suggest keeping busy automations on an API key preset (`nolune trigger` takes a
preset). When a plan's limit is used up, chats on it stop until the time the error names.

- **`claude-plan`** runs chats on the Claude Pro or Max plan someone signed in to Claude Code with
  on this computer: nolune runs Claude Code, which keeps the sign-in. `nolune claude-plan status` also
  says which Claude Code nolune runs. Nobody signs in through nolune or you: the owner runs
  `nolune claude-plan setup` in a terminal on this computer, which installs Claude Code if needed and
  starts its own sign-in (or runs `claude` there and uses `/login`). That command asks questions,
  so it doesn't work from your commands. Never ask for, look for or copy a Claude sign-in or its
  tokens.
- **`chatgpt-plan`** runs chats on the ChatGPT Plus or Pro plan of someone signed in with ChatGPT
  (Sign in with ChatGPT; nothing to install). Signing in takes a browser: suggest **Continue with
  ChatGPT** under Models & keys in the account menu, which also takes the address a browser on
  another device ends on. Or run `nolune chatgpt-plan setup` in the background and pass on the link
  it prints: in a browser on this computer that's all, while from a phone the page it ends on won't
  load, and its address goes into Models & keys (the sign-in waits up to 10 minutes).
  `--another-account` signs in to another ChatGPT account than the last one. Never ask for, look
  for or copy the sign-in nolune keeps (`~/.nolune/chatgpt.json`). When the plan's limit is
  reached, point to https://chatgpt.com/settings/usage. `nolune chatgpt-plan models` lists the
  models the plan offers. Don't run `nolune chatgpt-plan logout` while presets use it: chats on
  them stop answering.

## API keys

The `anthropic` key runs chats on Claude; the `openai` one runs chats on OpenAI's models and makes
pictures; the `openrouter` one runs chats on OpenRouter's models and pays for them with its
credits; the `xai` one runs chats on Grok and pays from its team's credits. The `firecrawl` one is
for `nolune web`: without it, web searches and pages go through Firecrawl's free tier, which allows
this computer so many a day, and a key (a free Firecrawl account has one) lifts that.
`nolune key set` checks a key with the provider and refuses one it rejects, and nolune uses the
new key from the next message.

```sh
nolune key set openai 'sk-...'
nolune key set firecrawl 'fc-...'
```

Custom providers are the family's own model servers, listed with the API keys: each has a name,
the API it speaks (OpenAI's unless `--api anthropic`; llama.cpp's server has only Anthropic's),
an address, and a key if it wants one. nolune asks it for its models before it saves it. Ollama
listens at `http://localhost:11434` and LM Studio at `http://localhost:1234`; a server that speaks
both APIs can be added once for each.

```sh
nolune provider add Ollama http://localhost:11434
nolune provider add "GPU box" http://gpu-box:8000 --api anthropic --key '...'   # the same name again changes it
nolune provider list                                    # name, id, API, address, key
nolune provider rm "GPU box"                            # presets on it stop working
```

A key typed into a chat stays in the chat's history. When someone wants to add or replace a key
and hasn't pasted it yet, suggest Models & keys in the account menu instead, which keeps it out of
the chat. Don't remove the key of a provider that presets use (`nolune preset list` shows each
preset's provider): chats on those models stop answering.

## Environment variables for your commands

Skills and scripts that need a key or a setting (a smart home token, a weather service's key) read
it from the environment. `nolune env set` adds a variable to every command you and trigger scripts
run, in every profile, from the next command on:

```sh
nolune env set HASS_TOKEN '...'
nolune env list                     # names only
nolune env rm HASS_TOKEN
```

## Connected services (MCP servers)

An admin can connect apps and services as MCP servers, whose tools new chats then get as tools
of their own. When one asks you to connect one, take what its instructions say: the command
that starts it, or its address, and the key it needs. `nolune mcp add` connects to it to check it
and prints its tools, or why it couldn't:

```sh
nolune mcp add time -- uvx mcp-server-time
nolune mcp add notes --env NOTES_TOKEN='…' --description "The family's shared notes" -- npx -y <its package>
nolune mcp add github https://api.githubcopilot.com/mcp/ --header 'Authorization: Bearer ghp_…'
nolune mcp add-json files '{"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/Users/anna/Documents"]}'
nolune mcp list                     # the names of their keys, never the keys
nolune mcp rm github
```

`--profile <slug>` (as often as needed) keeps a server to those profiles: one that reaches
someone's own account belongs in their profile. Chats that started before a server was added or
removed still list the servers they started with. Servers that only sign in through a web page
(OAuth) can't be connected yet.

## Skills

```sh
nolune skill list                       # what this profile has, on or off, and what each costs per chat
nolune skill disable generate-images    # leave it out of this profile's new chats
nolune skill enable generate-images
nolune skill new <name> --global --description "..."   # for every profile, in ~/.agents/skills
```

- `--profile` can be left out: it defaults to this profile. Members do the same on the profile's
  Skills page.
- Every skill that is on adds its name and description to each new chat. Turn off the ones a
  profile doesn't use rather than deleting them.
- Built-in skills (`builtin` in the list) are replaced when nolune updates. To change one for this
  profile, copy its folder into `$NOLUNE_PROFILE_DIR/skills/` and edit the copy; it takes precedence.

## Learning from chats

Besides what you save with `nolune memory`, nolune looks over each chat once it has been quiet for a
couple of minutes and saves what's worth remembering to this profile's memory, with the chat's
model (one short model call per quiet spell). It's on unless a profile turned it off.

```sh
nolune memory learning        # on or off for this profile
nolune memory learning off    # save only what you save yourself
nolune memory learning on
```

Any member can change it, here or with the switch on the profile's Memory page. What's already in
memory stays either way.

## Memory search by meaning

Memory search and the facts that come with each message also match by meaning, with embeddings of
each fact: on its own, nolune uses OpenAI's `text-embedding-3-small` with the OpenAI key, else the
same model through OpenRouter's key. `nolune config` shows what it uses (`embeddings`). It's shared by
every profile, so it's an admin's to change, here or under Memory search in Models & keys:

```sh
nolune config set embeddings off                                    # words only; facts stay here
nolune config set embeddings auto                                   # back to nolune's own choice
nolune config set embeddings openrouter/qwen/qwen3-embedding-8b     # another model, with its key
nolune config set embeddings custom-openai/ollama/nomic-embed-text  # a model of the custom provider "ollama"
```

The last form takes a model a custom provider that speaks OpenAI's API serves, by its id
(`nolune provider list`, above), which keeps every fact on this computer when the server runs here. It asks the source once and
says if it didn't answer. Every fact is embedded again with a new model, in the
background; until then, and whenever it can't reach the server, search goes by words.

## The avatar

Each profile has a small one-color mascot that stands for you in the web chat: next to your
replies, on the new chat screen, in the profile list and as the tab's icon. It moves while you
think and work. There are eight: probe, campfire, lantern, planet, quantum, comet, moon and
satellite. Only the family sees it; nothing about it reaches you or your chats.

```sh
nolune profile avatar            # which one this profile has
nolune profile avatar comet      # "switch to the comet"; open pages show it within a few seconds
```

Any member can change it, here or on the profile's People & profile page. When someone asks for
one that isn't in the list, say which ones there are.

## Accounts

There is no sign-up page: admins make accounts on the People page, or ask you. Everyone with an
account can ask you to do anything on this computer, so only add people, make invite links, make
admins, reset passwords or remove accounts when an admin asks. Prefer an invite link: the person
picks their own password, so none passes through the chat. Check the name and email with them
before creating an account.

```sh
nolune user list                                  # name, email, and "admin"
nolune user invite Grandma                        # prints a link for her, good once for 7 days
nolune user create Grandma grandma@example.com    # prints her password
nolune user create Dad dad@example.com --admin
nolune user passwd Anna                           # prints a new password
nolune user admin Anna                            # --off takes it away
nolune user rm Anna                               # can't be undone: ask first
```

- Leave out `--password`: nolune makes a strong one and prints it. It shows up in this chat, so say
  it's best passed on privately. There is no "forgot password" page and no way to change it on the
  web; `nolune user passwd` is how to reset it.
- A new person signs in with their email at the `origin` that `nolune config` shows. They see no
  profiles until a member adds them on the profile's Settings page, under Members (there is no
  command for that). Profiles are created on the web too; `nolune profile list` shows their slugs.

## The address

`nolune config` shows `listen`, where the gateway listens (`127.0.0.1:5780` unless changed), and
`origin`, the address people open: nolune's relay's while `relay` is on, else usually a tunnel
(Tailscale Funnel, Cloudflare Tunnel) pointed at `listen`. The relay gives an address like
`https://smiths.nolune.family` that works from anywhere with nothing to set up; it could see
the traffic it passes on, as any tunnel could.

```sh
nolune relay status                                        # the address; is nolune connected?
nolune relay enable --name smiths                          # smiths.nolune.family, after a restart
nolune relay disable                                       # back to the origin, after a restart
nolune config set origin https://nolune.example.com
nolune config set port 5781                                # the tunnel must point at it too
nolune config set image-model openai/gpt-image-2.5-flare   # applies right away
```

`origin`, `host`, `port` and the relay apply after a restart, and a wrong one locks everyone out,
including the admin who could ask you to fix it. Before you restart, check that a new origin
already reaches nolune (`curl -sI https://nolune.example.com`) and that the tunnel points at a new
port, and tell the admin the command that switches it back, to run at the computer.

## The service

On macOS nolune runs in the background as a LaunchAgent, or, set up with the nolune app, while the
app is open in the menu bar; on Linux it runs as a systemd user service.
Elsewhere, or on Linux without systemd, the owner runs `nolune start` under their own process
manager, and `nolune service` doesn't work.

```sh
nolune service status     # "Running (pid ...)" or what's wrong
nolune service logs       # the last 100 lines of $NOLUNE_HOME/logs/gateway.log
grep -i error "$NOLUNE_HOME/logs/gateway.log" | tail -n 20
```

Don't run `nolune service logs -f` (it never ends), `nolune start`, `nolune setup`, or
`nolune service install|uninstall`: they are for the owner at the terminal.

### Restarting

You run inside the gateway. `nolune service restart` ends your reply on the spot, so the person never
gets an answer, and stops whatever nolune is doing in other chats and automations at that moment.
Restart only when an admin asks or agrees (for a new address, or after an update), and only when
`nolune service status` says it is running. Start it in the background with a delay, so your reply is
finished first:

```sh
nohup sh -c 'sleep 60; nolune service restart' >/dev/null 2>&1 &
```

Then say that nolune restarts in about a minute, is gone for a few seconds, and that if the page
doesn't come back they should reload it.

### Updating

`cat "$NOLUNE_HOME/bin/nolune"` shows which nolune runs. A path ending in `dist/cli.js` is an npm install:
update it with the npm next to the node that runs nolune (run with `timeout_seconds` set to 600),
then restart as above.

```sh
node_bin=$(sed -n 's/^exec "\([^"]*\)".*/\1/p' "$NOLUNE_HOME/bin/nolune") &&
  "$(dirname "$node_bin")/npm" install -g nolune@latest
```

A path ending in `packages/cli/src/index.ts` is a source checkout: leave updating to the owner.

This skill ships with nolune and is replaced on updates. To adapt it for this profile, copy the folder
into the profile's skills folder and edit the copy; it takes precedence.
