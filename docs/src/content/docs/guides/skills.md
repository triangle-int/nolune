---
title: Skills
description: Teach nolune repeatable tasks, and give its commands what they need.
---

Skills live in `~/.nolune/profiles/<profile>/skills` and `~/.agents/skills`
([Agent Skills](https://agentskills.io) format). The agent creates its own with `nolune skill new`.

Each skill's name and description go into every new chat, so turn off the ones a profile doesn't
need on its **Skills** page (or `nolune skill disable <name> --profile <slug>`).

## Environment variables

Some skills need a key of their own, like a web-search one. Give the agent's commands extra
environment variables with `nolune env set`:

```sh
nolune env set FIRECRAWL_API_KEY fc-...
```
