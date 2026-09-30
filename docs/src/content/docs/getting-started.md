---
title: Install and set up
description: Install nolune, connect a model and add the family.
---

## What you need

- **macOS or Linux** (the background service needs systemd there) and **Node.js 22.18** or later.
- **A model connection**, any of these:
  - an [Anthropic API key](https://console.anthropic.com/), an
    [OpenAI API key](https://platform.openai.com/api-keys) or an
    [OpenRouter API key](https://openrouter.ai/settings/keys). Chats run on Claude, on OpenAI's GPT
    models, or on any model OpenRouter serves that can call tools; you can have all three;
  - a model server of your own (Ollama, LM Studio, oMLX, vLLM...) with a model that can call tools;
  - a Claude Pro or Max plan signed in to [Claude Code](https://claude.com/claude-code) on the same
    computer, or a ChatGPT Plus, Pro or Business plan signed in to OpenAI's
    [Codex](https://developers.openai.com/codex/cli) there.

[Models and keys](/docs/guides/models/) explains each of them.

## Install

```sh
npm install -g nolune
nolune setup                   # your account and the public URL
nolune key set openai          # optional: GPT models for chats, and pictures (Images page)
nolune service install         # run in the background: a LaunchAgent on macOS, systemd on Linux
nolune user create Anna anna@example.com   # add family members (prints their password)
```

Without systemd, run `nolune start` under your own process manager instead.

Then open the address `nolune setup` printed, sign in and make a profile: its welcome asks for a
model, on an API key or a plan. As the admin you can also add or replace API keys, sign in with
ChatGPT, and add models on the web, under **Models & keys** in your account menu.

## On a Mac

**Files in Documents, Desktop, Photos, Mail.** macOS blocks background processes from these until
you give the `node` binary Full Disk Access (System Settings > Privacy & Security > Full Disk
Access). `nolune setup` prints the exact path.

**Keep the Mac awake** if people should reach it at any time (System Settings > Energy).

## On Linux

`nolune service install` writes a systemd user service (`~/.config/systemd/user/nolune.service`,
systemd 240 or later) and turns on lingering for your user, so it runs from boot whether you're
logged in or not. Where that needs an admin, it prints the `sudo loginctl enable-linger` command.
Run it from your own login, not with `sudo` or `su`, which leave no user systemd to talk to.
`systemctl --user status nolune` works too.

## Reaching it from other devices

The gateway listens on `127.0.0.1:5780`, so at first only this computer can open it.
[Remote access](/docs/guides/remote-access/) shows how to put a tunnel in front of it.

## Where things are

- Logs: `nolune service logs -f`.
- Data: `~/.nolune` (override with `NOLUNE_HOME`). Conversations are in a local SQLite database;
  profile files, memory and configuration are files under it. When you use a hosted model or
  embeddings provider, the relevant content is sent to that provider.
- `nolune help` lists every command; see [Commands](/docs/reference/commands/).

## Update

```sh
npm install -g nolune@latest && nolune service restart
```
