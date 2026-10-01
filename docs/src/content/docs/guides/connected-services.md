---
title: Connected services
description: Give nolune the tools of other apps and services, like a calendar, GitHub or the smart home, through MCP servers.
---

Many apps and services offer an [MCP server](https://modelcontextprotocol.io): a set of tools an
assistant can use, like searching a calendar, creating an issue or turning on the lights. Connect
one and nolune's agent can use its tools in chats, automations and subagents, on every model.

## Connecting a server

Admins connect servers on the **Connected services** page, in the menu under their name: **Connect
a server**, give it a name, and what its instructions say:

- **A command** that starts it on this computer, like
  `npx -y @modelcontextprotocol/server-filesystem ~/Documents` or `uvx mcp-server-time`, with the
  environment variables it needs (`NAME=value`, a line each).
- **An address**, often ending in `/mcp`, with the headers it needs, like
  `Authorization: Bearer …`. Choose SSE for older servers.

nolune connects to it right away and says how many tools it has, or why it couldn't connect. It
keeps a server it couldn't reach, so you can install what it needs and press **Check**.

The same from a terminal:

```sh
nolune mcp add time -- uvx mcp-server-time
nolune mcp add github https://api.githubcopilot.com/mcp/ --header 'Authorization: Bearer ghp_…'
nolune mcp add-json files '{"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/Users/anna/Documents"]}'
nolune mcp list
nolune mcp rm github
```

`add-json` takes a server as MCP clients' settings write it, so the snippet from a server's README
can be pasted, `"mcpServers"` and all.

### Who gets it

Every profile gets a server unless you choose some under **Profiles** (`--profile <slug>` in the
CLI). A server that reaches someone's own account, like their email, belongs in their profile.

**What it's for** is a few words that tell the agent when to use it. Left empty, nolune uses what
the server says about itself.

## How the agent uses it

nolune gives the agent one tool, `run_command`, and connected services don't change that. Each new
chat's instructions name the profile's servers, and the agent lists their tools, looks at what one
takes and calls it with `nolune mcp` commands:

```sh
nolune mcp tools github
nolune mcp call github search_issues '{"query": "repo:anna/garden is:open"}'
```

So every model gets them the same way, including the plans, and adding a server doesn't change the
tools or the prompt cache of the chats already running. They see it from their next new chat. Pictures
a tool returns are shown to the agent, as with `nolune view`.

[Auto mode](/docs/guides/auto-mode/) checks each call like any other command: looking things up goes
ahead, while creating, changing, deleting or sending something in someone's account needs someone in
the chat to have asked for it.

## Good to know

- A server that runs on this computer runs as your account, like the agent's commands, and finds
  programs (`npx`, `uvx`, `docker`) where they do. Connect only servers you trust.
- nolune keeps each server running while it's used, and stops it after 10 minutes without a call
  or when the gateway stops.
- Servers, with their keys, are kept in `~/.nolune/config.json`. The page and `nolune mcp list` show
  the keys' names, never the keys.
- Servers that only sign in through a web page (OAuth) can't be connected yet. Use one that takes a
  key or token instead, when the service offers it.
