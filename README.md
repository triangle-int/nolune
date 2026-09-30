![nolune and its eight avatars floating in space](docs/images/nolune-space.png)

# An AI assistant for your family

I built nolune for my family: a place where we can each have our own space, invite each other into
shared profiles, and get help with everyday things.

nolune runs on your computer and gives everyone a built-in web UI. Make a profile for yourself, one for
the whole family, or one for something you're doing together. Each has its own conversations,
memory, skills, and assistant personality. In a shared profile, everyone can pick up the same
conversation, and the assistant knows who's speaking.

![nolune in action: cleaning up Downloads, scheduling weather checks, and saving memories](docs/demo/nolune-demo.gif)

[Watch the full-resolution demo](docs/demo/nolune-demo.mp4) · Clean up files → schedule a task → remember what matters.

_Screenshots use fictional accounts and sample content in the running app._

## Your space, our space

A profile is a space for a person or a group, rather than a separate login. One account can belong
to several profiles, and you can switch between them whenever you need to.

| Space              | Who it's for           | What you might keep there                         |
| ------------------ | ---------------------- | ------------------------------------------------- |
| Your own profile   | Just you               | Personal projects, ideas, and preferences         |
| The family profile | Everyone at home       | Meal plans, household notes, and family routines  |
| A shared project   | The people taking part | A holiday, a birthday, or something you're making |

Any member can add another existing user through **People & profile**. They join the profile's
conversations and memory, so you don't have to keep forwarding answers or explaining the same
background. Members can also manage the profile and its membership.

![Personal and shared profiles in nolune](docs/screenshots/profiles.png)

## What makes it useful

- **A built-in web UI for the whole family.** Sign in from a browser to switch profiles, chat,
  attach files, manage memory and skills, create images, and review automations. Family members
  use the interface; the terminal is for setup and administration.
- **Memory you can see and change.** nolune remembers preferences, people, plans, and where things
  are. Each member can have a linked person note, so “I” means the person writing. Review, edit,
  merge, or delete notes on the Memory page, undo automatically saved memories, or turn learning
  from chats off.
- **An assistant that can do the work.** It can work with files on the host computer, run commands,
  inspect photos and documents, and return files in the chat. Give it a form to fill in or ask it
  to find the photos from your last trip.
- **Help between conversations.** Ask for a recurring task in ordinary language. Automations run
  in the background and bring results to the notification bell, where you can continue the work
  as a conversation.
- **A place for things you're doing together.** Folders keep related chats, instructions, and files
  together. Skills teach the assistant repeatable tasks; each profile chooses which ones to use.
- **Room to make things.** Image templates help you start with a party invitation, a storybook
  page, a postcard, or a sticker pack. Choose a template or describe an idea, then ask for changes
  in the chat. Image generation needs an OpenAI API key.
- **Your choice of models.** Use Anthropic, OpenAI, OpenRouter, or your own model server. The project
  also integrates with Claude Code and Codex sign-ins. Switch models within a conversation while
  keeping its history.
- **Built for everyday use.** Pick an avatar and personality for each profile. Bring over a summary
  of what another assistant knows about you during onboarding. The interface supports English,
  Russian, German, Spanish, and French, with technical details tucked away until you want them.

![Editable family memory in nolune](docs/screenshots/memory.png)

## One tool. Less overhead. More cache reuse.

nolune gives the agent a single tool: **`run_command`**. It uses the command line for files,
memory, skills, automations, and background work. That keeps the tool definitions small while
letting the assistant use the programs already on your computer.

The conversation is designed to keep its prompt cache stable: tool definitions and the system
prompt are saved with the chat, new messages are appended, and recalled memories arrive with
the relevant message instead of changing the system prompt on every turn. Model responses are
preserved for replay to their original provider.

Together, a small tool interface and reusable prompt prefixes reduce token overhead and can
lower repeated-input costs on providers that support caching. Actual savings depend on the
model, cache lifetime, and workload; switching models or changing a chat's instructions can
require a fresh cache. Enable **Show technical details** to inspect token usage and cache hits.

See [prompt caching](DESIGN.md#prompt-caching) for the implementation.

## Try it

You'll need **Node.js 22.18+** and a model connection. `nolune service install` runs the gateway in
the background: as a LaunchAgent on macOS, and as a systemd user service on Linux.

```sh
npm install -g nolune
nolune setup
nolune service install
```

Without systemd, run `nolune start` under your own process manager instead.

Open the address from setup, sign in, and create your first profile. Its welcome helps you pick a
model and personalize the assistant. Admins manage providers and models under **Models & keys**.
For an API key, you can also use the CLI:

```sh
nolune key set openai       # or anthropic / openrouter
```

To bring someone into the family:

1. Create their account on the host computer:

   ```sh
   nolune user create Anna anna@example.com
   ```

2. Give them the address and the password printed by the command.
3. Open **People & profile** in the profile you want to share, and add them by name or email.

They can create their own profiles and add existing users too. Accounts are created by the host's
administrator; joining a profile doesn't send an email invitation.

> [!IMPORTANT]
> nolune is built for people you trust. The agent runs commands with your host account's permissions,
> without a sandbox. In auto mode, the default, a model checks each command before it runs and
> blocks what could do harm nobody asked for; it's a safeguard against mistakes and manipulation,
> not a sandbox. Profiles organize access in the web app; they do not isolate the agent from files
> on the computer. Everyone in a shared profile can read its chats and memory.

## Your computer, your setup

Conversations are stored in a local SQLite database; profile files, memory, and configuration live
under `~/.nolune` (or `NOLUNE_HOME`). When you use a hosted model or embeddings provider, relevant
content is sent to that provider.

The installed gateway listens on `127.0.0.1:5780`. To reach it from other devices, set up a tunnel
or reverse proxy and configure its address:

```sh
nolune config set origin https://nolune.example.com
nolune service restart
```

Keep the host awake when the family needs access. On macOS, access to protected folders may
require Full Disk Access for the Node binary; setup prints its path.

See the [setup and usage guide](docs/usage.md) for model connections, subscription integrations,
remote access, skills, automations, and everyday commands.

## Development

Built with TypeScript, SvelteKit, Svelte 5, Tailwind CSS, Drizzle, SQLite, and Better Auth.
Use pnpm for development:

```sh
pnpm install
pnpm nolune setup
cp packages/web/.env.example packages/web/.env
pnpm dev
```

The example environment uses `http://localhost:5173`. Development uses `~/.nolune` by default;
set `NOLUNE_HOME` for both setup and the dev server if you want a separate instance.

```sh
pnpm check     # Svelte and TypeScript checks
pnpm lint      # Formatting and ESLint
pnpm test      # Vitest, in every package
pnpm build     # Web app and CLI
```

The web app lives in `packages/web/`, the agent and storage in `packages/core/`, and the CLI in
`packages/cli/`. Database migrations apply automatically. Tests use a temporary data directory.

Read the [design notes](DESIGN.md) for architecture and tradeoffs, or the
[development guide](docs/development.md) for database changes, CI, and publishing.
