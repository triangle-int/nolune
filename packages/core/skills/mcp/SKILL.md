---
name: mcp
description: Use the tools of apps and services the family connected to nolune (MCP servers, like a calendar, GitHub, Notion or the smart home) with `nolune mcp`. Use whenever a task involves one of the connected services named in your instructions, or someone asks what nolune is connected to.
---

# Connected services (MCP servers)

An admin can connect apps and services to nolune as MCP servers. Each one has tools: search the
calendar, create an issue, turn on the lights. You use them through `nolune mcp`, in your commands
like anything else. The services this profile had when the conversation started are listed in your
instructions; `nolune mcp list` shows them as they are now.

## Find the tool

```sh
nolune mcp tools github                  # its tools, with what each does, and what the server says about using it
nolune mcp tools github search_issues    # one tool: its whole description and its arguments (JSON Schema)
```

`nolune mcp tools` with no server lists the tools of every server this profile has. In the list,
`create_issue(owner, repo, title, body?)` means `body` can be left out. Look at a tool on its own
before you call it for the first time: its arguments' schema says their names, types and allowed
values.

## Call it

The arguments are one JSON object, in single quotes. Pass long or tricky ones on stdin with a quoted
heredoc, so quotes and `$` in them can't break the command:

```sh
nolune mcp call github search_issues '{"query": "repo:anna/garden is:open label:bug"}'

nolune mcp call notion create_page - <<'EOF'
{"title": "Packing list", "content": "- passports\n- chargers"}
EOF
```

- What the tool returns is printed as it is. Text that's long, or JSON you need to dig through,
  is better saved to a file and read with `jq`, `grep` or `head` than printed whole:
  `nolune mcp call github list_issues '{...}' > /tmp/issues.json && jq length /tmp/issues.json`.
- Pictures it returns are attached for you to see, like `nolune view`, and the line says where
  the file is: show one to the family with `![what it shows](path)`. Other files (audio, PDFs)
  are saved, and the line says where.
- The command exits with 1 when the tool reports an error; read what it says, fix the arguments
  and try again, or tell the person what went wrong.
- A tool that works slowly (a long search, a big export) needs a longer `timeout_seconds` than the
  default 120.

## Be careful with what changes things

Tools act in someone's account, outside this computer. Looking things up is fine whenever it helps.
Before a tool sends, posts, books, buys, deletes or changes something, be sure that's what the
person asked for, as you would doing it by hand. `[reads only]` and `[can delete or overwrite]` in
the tool list are what the server says about a tool; other tools may change things too.

## When it doesn't work

- "isn't connected in this profile" or "there's no MCP server called …": it's not this profile's.
  Say so; an admin can add it under Models & keys.
- "couldn't start", "couldn't reach", "turned nolune away": the server is down, missing something,
  or its key no longer works. Tell the person in plain words; an admin fixes it under Models &
  keys. Don't edit nolune's config.json yourself.
- When someone asks you to connect a new service, `nolune mcp add` can (see `nolune help`), with
  the key they give you. Servers that only sign in through a web page (OAuth) can't be connected
  yet.
