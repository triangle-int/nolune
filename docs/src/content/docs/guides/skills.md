---
title: Skills
description: Teach nolune repeatable tasks, and give its commands what they need.
---

Skills live in `~/.nolune/profiles/<profile>/skills` and `~/.agents/skills`
([Agent Skills](https://agentskills.io) format). The agent creates its own with `nolune skill new`.

Each skill's name and description go into every new chat, so turn off the ones a profile doesn't
need on its **Skills** page (or `nolune skill disable <name> --profile <slug>`).

## Searching the web

nolune searches the web and reads pages out of the box, with the built-in `web` skill, through
[Firecrawl](https://www.firecrawl.dev). Without a key it uses Firecrawl's free tier, which allows
your computer a limited number of searches a day. When the family needs more, add a key from a
free Firecrawl account under **Models & keys**, or at the terminal:

```sh
nolune key set firecrawl fc-...
```

`FIRECRAWL_API_URL` points nolune at a Firecrawl you run yourself.

## Environment variables

Some skills need a key of their own, like a smart home token. Give the agent's commands extra
environment variables with `nolune env set`:

```sh
nolune env set HASS_TOKEN ...
```
