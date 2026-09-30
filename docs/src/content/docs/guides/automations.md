---
title: Automations
description: Recurring tasks in plain words, background work and subagents.
---

## Automations

Ask nolune in a conversation ("every weekday at 7:30, tell us if we need umbrellas", "check my email
every 10 minutes and tell me when the school writes"). It sets up a trigger that runs in the
background, and what it finds shows up under the bell at the top; open a notification to continue
it as a conversation.

Each profile's **Automations** page lists them. From the terminal: `nolune trigger list`, and the
other `nolune trigger` commands in [Commands](/docs/reference/commands/#automations).

## Background work and subagents

nolune can run a long command in the background and carry on; its output comes back to the chat
when it's done.

For big or parallel jobs it starts subagents with `nolune agent run` (the built-in `subagents` skill
explains when): each works in a hidden conversation of its own that starts with only its task, and
reports back to the chat. While they work, the chat lists them under "Working in the background",
where you can open a subagent's own chat or stop everything. Logs:
`~/.nolune/profiles/<profile>/agents`.

:::tip
Plan limits assume one person's ordinary use. Keep busy automations and subagents on an API key
preset rather than a [Claude or ChatGPT plan](/docs/guides/models/#on-your-own-plan-instead-of-an-api-key).
:::
