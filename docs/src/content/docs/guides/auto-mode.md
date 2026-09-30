---
title: Auto mode
description: How nolune checks the agent's commands before they run, and how to change that.
---

Before a command runs, a model checks it and blocks what could do harm nobody in the chat asked
for: deleting more than you asked, sending files or passwords out, changing security settings,
installing things from unknown places, buying or posting for you. Commands that only look (`ls`,
`cat`, `grep`, `nolune memory search`...) run without a check.

A blocked command doesn't run; nolune says what it wanted to do, and a yes from you in the chat lets
it through.

## The model that checks

Checks use the chat's own model unless an admin picks a faster preset for them; every command that
does more than look costs a short call to it. `nolune config set safety-model <preset>` picks the
model, and `nolune config set safety-model chat` goes back to each chat's own.

## Turning it off

Admins switch to **Unrestricted** (commands run unchecked, not recommended) under Commands in Models
& keys, or with `nolune config set command-mode unrestricted`. nolune can't change either setting
itself.

A single chat can differ: the shield in its message box switches it between Auto and Unrestricted
(only admins can pick Unrestricted there), and the subagents it starts follow it.

:::note
Auto mode guards against mistakes and manipulation, like a web page or an email that tries to talk
the agent into something. It isn't a sandbox: the agent still runs as your account. See
[DESIGN.md](https://github.com/triangle-int/nolune/blob/main/DESIGN.md#auto-mode) for how the check
works.
:::
