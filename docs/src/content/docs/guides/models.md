---
title: Models and keys
description: Run chats on an API key, OpenRouter, a model server of your own, or a Claude or ChatGPT plan.
---

A profile's welcome asks for a model the first time. As the admin you can add or replace API keys,
sign in with ChatGPT and add models on the web, under **Models & keys** in your account menu, or
from the terminal as below. Model presets are shared by all profiles.

## API keys

```sh
nolune key set anthropic       # or openai / openrouter
```

nolune checks a key before it stores it. OpenAI's key also makes pictures on the
[Images](/docs/guides/images/) page.

## Through OpenRouter

With `nolune key set openrouter`, a preset can run any model
[OpenRouter](https://openrouter.ai/models) serves that can call tools:
`nolune preset add deepseek/deepseek-v4.1-flash --provider openrouter`. OpenRouter's ids name the
model's maker. Pictures and PDFs go only to models that take them; for the others, they're saved
for the agent and named in the message, like any other file.

## On your own servers

Chats can run on model servers of your own, like [Ollama](https://ollama.com) or
[LM Studio](https://lmstudio.ai) on this computer, or vLLM on a machine with GPUs. Add each one as a
custom provider, with the name you want to see, under Models & keys (Add custom provider, under the
API keys) or with `nolune provider add Ollama http://localhost:11434` (`--key` if it wants one). It
speaks OpenAI's API unless you pass `--api anthropic` (Ollama, LM Studio and oMLX have both;
llama.cpp's server only Anthropic's). Then it's a provider like the others:
`nolune preset add qwen3:8b --provider Ollama`, or its chip in Add a model. nolune asks it for its
models to check it. The model must be able to call tools; pictures and PDFs reach it as their paths.

## On your own plan instead of an API key

Chats can run on a subscription someone in the family already has. Pick one in a new profile's
welcome, or with `nolune <plan> setup` and a preset on that provider
(`nolune preset add claude-opus-5-5 --provider claude-plan`, or on Models & keys).
`nolune <plan> status` says who the plan is signed in as. Plan limits assume one person's ordinary
use: keep busy automations and subagents on an API key preset. See
[DESIGN.md](https://github.com/triangle-int/nolune/blob/main/DESIGN.md#plans) for what works
differently.

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

## Switching models in a chat

The chip in the message box picks the model and how long it thinks, in a new chat or an existing
one. After a change the next reply reads the whole chat again (it isn't cached for the new setting
yet), so it's slower and costs more once; the chat asks before that happens.

`nolune preset default <name>` sets the model new chats start with, and `nolune preset list` shows
them all.
