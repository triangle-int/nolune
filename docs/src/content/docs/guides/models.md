---
title: Models and keys
description: Run chats on an API key (Anthropic, OpenAI, xAI), OpenRouter, a model server of your own, or a Claude or ChatGPT plan.
---

A profile's welcome asks for a model the first time. As the admin you can add or replace API keys,
sign in with ChatGPT and add models on the web, under **Models & keys** in your account menu, or
from the terminal as below. Model presets are shared by all profiles.

## API keys

```sh
nolune key set anthropic       # or openai / openrouter / xai
```

nolune checks a key before it stores it. OpenAI's key also makes pictures on the
[Images](/docs/guides/images/) page. Firecrawl's key (`nolune key set firecrawl`) is for
[searching the web](/docs/guides/skills/#searching-the-web), which works without one up to a daily
limit.

## Grok, with an xAI key

With `nolune key set xai` (a key from [console.x.ai](https://console.x.ai)), presets run on xAI's
Grok models: `nolune preset add grok-4.7 --provider xai`, or the xAI chip in Add a model, which
lists the models the key can use with their context windows. Each model gets the reasoning
effort it takes nearest the chat's. Grok sees pictures (sent as JPEG or PNG); PDFs are saved for
the agent and named in the message. Chats are paid from the key's team credits; when they run
out, or the team hits its spending limit, the chat says so.

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

- **`claude-plan`: Claude Pro or Max.** Chats run through
  [Claude Code](https://claude.com/claude-code) on this computer, unmodified, signed in to your
  Claude account; Claude Code keeps the sign-in and nolune never sees it. Setup installs it with
  Anthropic's installer, asking first, and starts Claude Code's own sign-in in your browser, so run
  it in a terminal on this computer. Anthropic
  [counts this](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan)
  as Agent SDK use of your subscription.
- **`chatgpt-plan`: ChatGPT Plus or Pro.** Nothing to install: nolune uses OpenAI's
  [Sign in with ChatGPT](https://developers.openai.com/siwc) for open-source apps (in preview), and
  its requests count toward your plan's usage, shared with ChatGPT and Codex. **Continue with
  ChatGPT** under Models & keys, or `nolune chatgpt-plan setup`, opens ChatGPT's sign-in page, where
  you allow nolune to use your plan. OpenAI sends the browser back to `127.0.0.1`, this computer:
  in a browser here that's all, and from a phone or another computer the page it ends on doesn't
  load, so copy its address and paste it under Models & keys (or into `setup` at a terminal).
  nolune keeps the sign-in in `~/.nolune/chatgpt.json`, readable by you only. See and limit what
  nolune uses in [ChatGPT's usage settings](https://chatgpt.com/settings/usage), where you can also
  disconnect it. `nolune chatgpt-plan models` lists what the plan offers (OpenAI lists a new model
  only to Codex versions made for it, so nolune asks for the list as Codex's latest release, which
  it looks up on npm), and
  `nolune chatgpt-plan logout` signs out. Earlier versions ran Codex for this: its folder,
  `~/.nolune/codex`, isn't used any more and can be deleted.

## Switching models in a chat

The chip in the message box picks the model and how long it thinks, in a new chat or an existing
one. After a change the next reply reads the whole chat again (it isn't cached for the new setting
yet), so it's slower and costs more once; the chat asks before that happens.

`nolune preset default <name>` sets the model new chats start with, and `nolune preset list` shows
them all.
