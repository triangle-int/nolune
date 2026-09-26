---
name: btw-agent
description: Change btw's own setup with the `btw` command, including models, API keys, environment variables for your commands, which skills are on, family accounts and passwords, the web address and the background service (status, logs, restarts, updates). Use whenever someone asks you to configure yourself or change how btw is set up, or asks how it is set up.
---

# Configuring btw

You are btw, and the `btw` command on your PATH configures you: the same CLI the owner uses in a
terminal. `btw help` lists every command. This skill explains how to use the ones that change btw
itself safely from your own commands.

Other parts of the CLI have their own instructions: automations (`btw trigger`, `btw wake`) in the
`automations` skill, pictures (`btw generate image`, `btw view`) in `generate-images` and
`view-images`, and memory (`btw memory`) in your system prompt.

## Before you change anything

- Look first. `btw config` shows the address, which API keys are set (only their last characters)
  and the names of the extra environment variables; `btw preset list`, `btw skill list` and
  `btw user list` show the rest.
- Everything here is shared by every profile and person, except which skills are on. Models, keys,
  accounts, the address and the service are an admin's job, as on the web, where only admins see
  Models & keys. `btw user list` marks the admins, and the name on a message is who is asking.
  When someone else asks, don't do it: tell them which admin can.
- Your commands have no keyboard, so give every value on the command line. `btw env set` without a
  value saves an empty one.
- Use `btw`, not the files. Don't edit `config.json` or `btw.db` in `$BTW_HOME` by hand, and don't
  print `config.json`: it holds the API keys.
- API keys, environment variables and the image model apply from the next command or message. The
  default model and which skills are on apply to new chats: a chat keeps the model and the skills
  it started with, this one too. The address applies after a restart.

## Models

Chats run on model presets. People pick one when they start a chat, and new chats start with the
default.

```sh
btw preset list                                 # name, model, context window, id, which is default
btw preset add claude-sonnet-5 --name "Sonnet"  # Anthropic checks the model id first
btw preset default Sonnet                       # new chats start with it
btw preset rm Sonnet                            # chats that use it keep working
```

If `btw preset add` can't check the model, an admin can add it under Models & keys in the account
menu.

## API keys

The `anthropic` key runs you; the `openai` one makes pictures. `btw key set` checks a key with
the provider and refuses one it rejects, and btw uses the new key from the next message.

```sh
btw key set openai 'sk-...'
```

A key typed into a chat stays in the chat's history. When someone wants to add or replace a key
and hasn't pasted it yet, suggest Models & keys in the account menu instead, which keeps it out of
the chat. Don't remove the Anthropic key (`btw key rm anthropic`): without it btw can't answer
anyone.

## Environment variables for your commands

Skills and scripts that need a key or a setting (a web search API, a smart home token) read it
from the environment. `btw env set` adds a variable to every command you and trigger scripts run,
in every profile, from the next command on:

```sh
btw env set FIRECRAWL_API_KEY 'fc-...'
btw env list                     # names only
btw env rm FIRECRAWL_API_KEY
```

## Skills

```sh
btw skill list                       # what this profile has, on or off, and what each costs per chat
btw skill disable generate-images    # leave it out of this profile's new chats
btw skill enable generate-images
btw skill new <name> --global --description "..."   # for every profile, in ~/.agents/skills
```

- `--profile` can be left out: it defaults to this profile. Members do the same on the profile's
  Skills page.
- Every skill that is on adds its name and description to each new chat. Turn off the ones a
  profile doesn't use rather than deleting them.
- Built-in skills (`builtin` in the list) are replaced when btw updates. To change one for this
  profile, copy its folder into `$BTW_PROFILE_DIR/skills/` and edit the copy; it takes precedence.

## Accounts

There is no sign-up page: accounts are made only with `btw user`. Everyone with an account can ask
you to do anything on this computer, so only add people, make admins, reset passwords or remove
accounts when an admin asks. Check the name and email with them before creating an account.

```sh
btw user list                                  # name, email, and "admin"
btw user create Grandma grandma@example.com    # prints her password
btw user create Dad dad@example.com --admin
btw user passwd Anna                           # prints a new password
btw user admin Anna                            # --off takes it away
btw user rm Anna                               # can't be undone: ask first
```

- Leave out `--password`: btw makes a strong one and prints it. It shows up in this chat, so say
  it's best passed on privately. There is no "forgot password" page and no way to change it on the
  web; `btw user passwd` is how to reset it.
- A new person signs in with their email at the `origin` that `btw config` shows. They see no
  profiles until a member adds them on the profile's Settings page, under Members (there is no
  command for that). Profiles are created on the web too; `btw profile list` shows their slugs.

## The address

`btw config` shows `listen`, where the gateway listens (`127.0.0.1:5780` unless changed), and
`origin`, the address people open, usually a tunnel (Tailscale Funnel, Cloudflare Tunnel) pointed
at `listen`.

```sh
btw config set origin https://btw.example.com
btw config set port 5781                                # the tunnel must point at it too
btw config set image-model openai/gpt-image-2.5-flare   # applies right away
```

`origin`, `host` and `port` apply after a restart, and a wrong one locks everyone out, including
the admin who could ask you to fix it. Before you restart, check that a new origin already reaches
btw (`curl -sI https://btw.example.com`) and that the tunnel points at a new port, and tell the
admin the command that switches it back, to run at the computer.

## The service

On macOS btw runs in the background as a LaunchAgent. Elsewhere the owner runs `btw start` under
their own process manager, and `btw service` doesn't work.

```sh
btw service status     # "Running (pid ...)" or what's wrong
btw service logs       # the last 100 lines of $BTW_HOME/logs/gateway.log
grep -i error "$BTW_HOME/logs/gateway.log" | tail -n 20
```

Don't run `btw service logs -f` (it never ends), `btw start`, `btw setup`, or
`btw service install|uninstall`: they are for the owner at the terminal.

### Restarting

You run inside the gateway. `btw service restart` ends your reply on the spot, so the person never
gets an answer, and stops whatever btw is doing in other chats and automations at that moment.
Restart only when an admin asks or agrees (for a new address, or after an update), and only when
`btw service status` says it is running. Start it in the background with a delay, so your reply is
finished first:

```sh
nohup sh -c 'sleep 60; btw service restart' >/dev/null 2>&1 &
```

Then say that btw restarts in about a minute, is gone for a few seconds, and that if the page
doesn't come back they should reload it.

### Updating

`cat "$BTW_HOME/bin/btw"` shows which btw runs. A path ending in `dist/cli.js` is an npm install:
update it with the npm next to the node that runs btw (run with `timeout_seconds` set to 600),
then restart as above.

```sh
node_bin=$(sed -n 's/^exec "\([^"]*\)".*/\1/p' "$BTW_HOME/bin/btw") &&
  "$(dirname "$node_bin")/npm" install -g btw-agent@latest
```

A path ending in `packages/cli/src/index.ts` is a source checkout: leave updating to the owner.

This skill ships with btw and is replaced on updates. To adapt it for this profile, copy the folder
into the profile's skills folder and edit the copy; it takes precedence.
