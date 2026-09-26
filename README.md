# btw

A small agent that lives on your computer and does things for your family. One gateway serves a
web chat; family members share profiles with their own conversations, skills and memory. The agent
has one tool, `run_command`, with full access to the computer. See [DESIGN.md](DESIGN.md) for how
it works and why.

> **It can do anything your user account can.** Everyone you add can ask it to read, change or
> delete your files. Only add people you trust, and keep the web address behind a login you control.

## Install

Needs macOS (Linux works without the background service), Node 22.18+ and an
[Anthropic API key](https://console.anthropic.com/).

```sh
npm install -g btw-agent
btw setup                      # API key, your account, default model, public URL
btw service install            # run in the background, start at login
btw user create Anna anna@example.com   # add family members (prints their password)
```

Then open the address `btw setup` printed and sign in.

**Reaching it from outside your home.** The gateway listens on `127.0.0.1:5780`. Put a tunnel in
front of it, e.g. [Tailscale Funnel](https://tailscale.com/kb/1223/funnel),
[Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/),
or your own VPS, and tell btw the public URL:

```sh
btw config set origin https://btw.example.com
btw service restart
```

**Files in Documents, Desktop, Photos, Mail.** macOS blocks background processes from these until
you give the `node` binary Full Disk Access (System Settings > Privacy & Security > Full Disk
Access). `btw setup` prints the exact path.

**Keep the Mac awake** if people should reach it at any time (System Settings > Energy).

## Using it

- `btw help` lists every command: users, model presets, profiles, skills, settings.
- The chat keeps what btw did folded under each reply ("Worked for 12s"), with plain-language
  steps. To see the exact commands, token usage and prompt caching, turn on **Show technical
  details** in Settings (click your name at the bottom of the sidebar).
- Skills live in `~/.btw-agent/profiles/<profile>/skills` and `~/.agents/skills`
  ([Agent Skills](https://agentskills.io) format). The agent creates its own with `btw skill new`.
  Each skill's name and description go into every new chat, so turn off the ones a profile doesn't
  need on its **Skills** page (or `btw skill disable <name> --profile <slug>`).
- **Automations.** Ask btw in a conversation ("every weekday at 7:30, tell us if we need umbrellas",
  "check my email every 10 minutes and tell me when the school writes"). It sets up a trigger that
  runs in the background, and what it finds shows up under the bell at the top; open a notification
  to continue it as a conversation. Each profile's Automations page lists them. From the terminal:
  `btw trigger list`.
- Extra environment variables for the agent's commands, e.g. for a firecrawl web-search skill:
  `btw env set FIRECRAWL_API_KEY fc-...`
- Logs: `btw service logs -f`. Data: `~/.btw-agent` (override with `BTW_HOME`).
- Update: `npm install -g btw-agent@latest && btw service restart`.

## Development

Requires pnpm. Node runs the TypeScript in `packages/` directly (type stripping), so no build step
is needed for the CLI while developing.

```sh
pnpm install
pnpm btw setup          # same CLI, from source
cp .env.example .env    # ORIGIN=http://localhost:5173
pnpm dev
```

```sh
pnpm check                          # svelte-check + tsc for packages/core and packages/cli
pnpm lint
pnpm db:generate --name <change>    # after editing packages/core/src/db/schema.ts
pnpm build                          # web build + dist/cli.js (what the npm package ships)
pnpm start                          # run the built gateway with the settings from btw config
```

Migrations are applied automatically when the gateway or the CLI opens the database.

### Publishing

`npm pack` builds and packs `build/`, `dist/cli.js` and the migrations. Before the first
`npm publish`: pick a license, add `license` and `repository` to `package.json`, and remove
`"private": true`.
