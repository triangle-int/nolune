---
title: Install and set up
description: Install nolune, connect a model and add the family.
---

## The Mac app

On a Mac, the easiest way in is the app. Download it for
[Apple silicon](https://github.com/triangle-int/nolune/releases/latest/download/nolune-macos-apple-silicon.dmg)
or [Intel](https://github.com/triangle-int/nolune/releases/latest/download/nolune-macos-intel.dmg)
(macOS 13 or later), open the disk image and drag nolune to Applications. It brings its own Node, and
its welcome sets up your account, an address that works from anywhere and Full Disk Access. After
that it stays in the menu bar, opens at login and keeps nolune running while it's there.

You still need a model connection (below), which a new profile's welcome asks for. The rest of this
page installs nolune from the terminal instead, on macOS or Linux.

## What you need

- **macOS or Linux** (the background service needs systemd there) and **Node.js 22.18** or later;
  the Mac app brings its own Node.
- **A model connection**, any of these:
  - the [nolune plan](https://nolune.dev/#pricing), a subscription to nolune itself that covers
    chats, pictures and memory search with no keys;
  - an [Anthropic API key](https://console.anthropic.com/), an
    [OpenAI API key](https://platform.openai.com/api-keys), an
    [xAI API key](https://console.x.ai) or an
    [OpenRouter API key](https://openrouter.ai/settings/keys). Chats run on Claude, on OpenAI's GPT
    models, on xAI's Grok, or on any model OpenRouter serves that can call tools; you can have them
    all;
  - a model server of your own (Ollama, LM Studio, oMLX, vLLM...) with a model that can call tools;
  - a Claude Pro or Max plan signed in to [Claude Code](https://claude.com/claude-code) on the same
    computer, or a ChatGPT Plus or Pro plan, signed in with ChatGPT.

[Models and keys](/docs/guides/models/) explains each of them.

## Install

```sh
npm install -g nolune
nolune setup                   # your account, and the address: nolune's relay, or your own
nolune key set openai          # optional: GPT models for chats, and pictures (Images page)
nolune service install         # run in the background: a LaunchAgent on macOS, systemd on Linux
nolune user invite Anna        # a link for a family member to make their account
```

Without systemd, run `nolune start` under your own process manager instead.

Then open the address `nolune setup` printed, sign in and make a profile: its welcome asks for a
model, on an API key or a plan. As the admin you can also add or replace API keys, sign in with
ChatGPT, and add models on the web, under **Models & keys** in your account menu, and add
people or send them invite links under **People**.

## On a Mac

**Files in Documents, Desktop, Photos, Mail.** macOS blocks background processes from these until
you give the `node` binary Full Disk Access (System Settings > Privacy & Security > Full Disk
Access). `nolune setup` prints the exact path. The macOS app
([`macos/`](https://github.com/triangle-int/nolune/tree/main/macos)) walks you through this
instead, and its switch is named nolune and covers only nolune.

**Keep the Mac awake** if people should reach it at any time (System Settings > Energy).

## On Linux

`nolune service install` writes a systemd user service (`~/.config/systemd/user/nolune.service`,
systemd 240 or later) and turns on lingering for your user, so it runs from boot whether you're
logged in or not. Where that needs an admin, it prints the `sudo loginctl enable-linger` command.
Run it from your own login, not with `sudo` or `su`, which leave no user systemd to talk to.
`systemctl --user status nolune` works too.

## Reaching it from other devices

Say yes to nolune's relay in `nolune setup` and you get an address like
`https://smiths.nolune.family` that works on any device, at home or away, with no tunnel or port
forwarding. Otherwise the gateway listens on `127.0.0.1:5780`, so only this computer can open it.
[Remote access](/docs/guides/remote-access/) explains the relay, and how to use a tunnel of your own
instead.

:::caution[Notifications on iPhones need the relay]
The nolune app for iPhone puts the bell's notifications on the lock screen only through nolune's
relay, which holds the key Apple asks for. On a tunnel or a relay of your own, the app gets none; the
bell still shows them whenever the app is open.
:::

## Where things are

- Logs: `nolune service logs -f`.
- Data: `~/.nolune` (override with `NOLUNE_HOME`). Conversations are in a local SQLite database;
  profile files, memory and configuration are files under it. When you use a hosted model or
  embeddings provider, the relevant content is sent to that provider.
- `nolune help` lists every command; see [Commands](/docs/reference/commands/).

## Update

When a new release is out, admins see it in the menu under their name, with how to update this
install. The gateway asks GitHub once a day; `nolune config set update-check off` stops it. From
npm:

```sh
npm install -g nolune@latest && nolune service restart
```

The macOS app's menu has a Download row for the new app instead: quit nolune, drag the new one to
Applications and open it. Chats, memory and settings stay as they are.
