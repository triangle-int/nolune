<p align="center">
  <img src="landing/static/assets/nolune-moon.svg" alt="Nolune, a lavender crescent companion" width="200" />
</p>

<h1 align="center">Nolune</h1>

<p align="center">
  <strong>Your AI, with a computer of its own.</strong><br>
  A personal AI companion that lives on your machine and works across your computers.
</p>

<p align="center">
  <a href="https://nolune.dev">Website</a> &nbsp;&bull;&nbsp;
  <a href="https://github.com/triangle-int/nolune/releases">Download</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-2024-CE422B?style=flat-square&logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/sveltekit-5-FF3E00?style=flat-square&logo=svelte" alt="SvelteKit" />
  <img src="https://img.shields.io/badge/tauri-2-24C8D8?style=flat-square&logo=tauri" alt="Tauri" />
  <img src="https://img.shields.io/badge/license-MIT-A97CF8?style=flat-square" alt="MIT License" />
</p>

<br>

Nolune is an open-source, self-hosted AI companion with a persistent identity, long-term memory, and the ability to use real computers.

Run Nolune's always-on brain on a Mac mini, home server, or spare computer. Then connect the desktop app on your other machines so Nolune can work with their screens, apps, files, and terminals from one interface.

Unlike a faceless agent dashboard, Nolune has a voice, personality, mood, and animated presence you can make your own. Your companion and its memories stay on hardware you control.

<br>

## How it works

### 1. Give Nolune a home

Install the Nolune server on an always-on macOS or Linux machine. It keeps your companion running, stores its memory, and coordinates its work. Either use the desktop app's **Install on this computer** button, or run the one-liner:

```bash
curl -fsSL https://nolune.dev/install.sh | bash
```

The installer downloads the server, prepares `~/.nolune`, starts the server in the foreground so you can watch its logs, and opens `http://localhost:26559`. The first browser has to be paired: run `nolune pair` on the machine you just installed on and enter the eight-digit code it prints. Then follow the onboarding.

Paired browsers stay signed in. Review or revoke them, or mint a code for another device, under **Settings → Connections**. The installer adds `~/.nolune/bin` to your `PATH`; until you open a new shell, use `~/.nolune/bin/nolune pair`.

Press Ctrl-C to stop the server and `nolune gateway` to start it again. To keep it running without a terminal, opt in to the background service with `nolune gateway install` (see [Install](#install)).

### 2. Connect your computers

Install the desktop app on the computers where you want Nolune to act. Each connected machine becomes another place where your companion can see the screen, use apps, work with files, and run commands.

For example, Nolune can live on a Mac mini at home while you talk to it through the desktop app on your MacBook.

### 3. Ask it to do the work

Talk to Nolune normally. It can research something on the web, organize files, work inside an app, run a command on a connected machine, or continue a task while you are away.

Computer-use actions appear through a visible desktop overlay. You choose which machines to connect and grant the operating-system permissions they need.

<br>

## Why Nolune

### A companion, not a control panel

Nolune is designed around one persistent character rather than a collection of disposable chats. Its personality lives in `soul.md`, its mood changes over time, and its animated skin gives it a recognizable presence.

### One mind across multiple computers

The server holds Nolune's identity and memory: one companion per server. Connected desktop apps give it eyes and hands on other machines. Computers, chats, and the people it talks to are contexts of that one identity, never separate companions, so what it learns in one place it knows everywhere. You can keep the server somewhere reliable and interact with the same companion from wherever you work.

### Memory you can inspect

Nolune stores memories as ordinary files organized by topic. You can read them, edit them, back them up, or remove them. Keyword and semantic retrieval bring relevant memories back into later conversations.

### Proactive when you want it to be

Heartbeats and schedules let Nolune check in, follow recurring routines, and start useful work without waiting for a new message every time.

### Yours from end to end

Nolune is self-hosted and BYOK. There is no required Nolune cloud account, and Nolune does not impose its own usage limits. Your model provider's pricing and limits still apply.

<br>

## Your companion

<p align="center">
  <img src="client/static/skins/moon/character.svg" alt="Nolune Little Moon companion" width="200" />
</p>

**Nolune · Little Moon** — a curious lavender crescent. One familiar identity across your computers, with memories and personality that stay yours.

Our [design system](docs/design-system.md) documents the visual language, reusable UI patterns, and accessibility rules. Run the client and open `/design-system` for the interactive reference.

<br>

## Capabilities

- **Computer use** — See the screen, click, type, scroll, and use apps on connected machines.
- **Remote files and shell** — Read and edit files or run commands on the server and connected computers.
- **Persistent memory** — Remember people, preferences, projects, and shared moments across conversations.
- **Voice** — Speak through optional text-to-speech and voice mode.
- **Web and communication** — Search the web, work with email, and use installed integrations.
- **Skills and MCP** — Add new workflows and connect external tools without changing Nolune's core.
- **Schedules and heartbeats** — Run recurring routines and initiate conversations proactively.

<br>

## Install

There are two ways to get a server. Both end the same way: the `nolune` binary in `~/.nolune/bin`, a workspace in `~/.nolune`, and the server running in the foreground. No background service is created unless you ask for one.

### Desktop app

Download the desktop app from [GitHub Releases](https://github.com/triangle-int/nolune/releases). Builds are available for macOS, Windows, and Linux.

On first run, choose **Install on this computer**. The app downloads the server for your platform, prepares the workspace, starts the server, and opens your companion. Nothing to paste and no terminal required. **Show logs** reveals the server output if you want it, and **Use nightly builds** sits behind an advanced toggle. The server runs while the app is open and starts again with it; the desktop app's **Run in background** setting hands it to a user-level service so it keeps running after you quit.

If you already run a server elsewhere, choose **Connect to an existing server** instead and enter its URL and auth token. That is how a laptop reaches the companion living on a Mac mini at home.

### One-line installer

The script does the same on macOS or Linux and leaves the server running in the foreground:

```bash
curl -fsSL https://nolune.dev/install.sh | bash
```

It is a thin wrapper: it downloads the binary and hands over to `nolune onboard` and `nolune gateway`. Set `NOLUNE_CHANNEL=nightly` for nightly builds or `NOLUNE_DIR` for a custom directory.

### The `nolune` command

Everything the installers do, you can do yourself:

| Command | What it does |
|---------|--------------|
| `nolune onboard` | Prepares `~/.nolune` and `config.toml` with a generated auth token. Safe to rerun. `--json` prints the result for scripts, `--port` picks the port |
| `nolune gateway` | Runs the server in the foreground and prints `nolune: ready <url>` once it listens (`nolune gateway run` is the explicit form) |
| `nolune gateway install` | Registers a user-level launchd agent (macOS) or systemd user unit (Linux) that runs the gateway, and starts it |
| `nolune gateway uninstall` | Stops and removes that service; data is untouched |
| `nolune gateway start` / `stop` / `restart` / `nolune gateway status` / `nolune gateway logs` | Manage the service once installed |
| `nolune pair` | Prints a one-time code so a browser can sign in |
| `nolune uninstall --keep-data` | Removes the service, binary, and log but keeps `~/.nolune` |
| `nolune uninstall --yes` | Removes everything, including your data |
| `--profile <name>` | Any of the above for a second, fully isolated server on the same machine: its own data root at `~/.nolune-profiles/<name>/`, config, port, auth token, log, and background service, from the same binary. `nolune onboard --profile molinka` then `nolune gateway install --profile molinka`; the default profile stays `~/.nolune`. All profiles run the one binary under `~/.nolune/bin/`, so `nolune uninstall` on the default profile warns which profiles' services lose it |

Service registration is per user and needs no elevated privileges. It is not available on Windows yet; run `nolune gateway` there.

The release workflow runs on `v*` tags; manual runs must select a `v*` tag. It publishes server binaries and macOS, Windows, and Linux desktop artifacts. Nothing in this workflow provisions or updates running servers. To exercise the pipeline from any branch without creating a release, run it manually with `dry_run` enabled; the binaries are attached to the workflow run as artifacts instead.

<br>

## Architecture

```text
server/     Rust + Axum — always-on brain, tools, memory, and embedded web client
client/     SvelteKit 5 — companion interface
desktop/    Tauri 2 — native client and computer-use bridge
landing/    SvelteKit — public website and documentation
```

| Layer | Technology |
|-------|------------|
| Server | Rust, Axum, Tokio |
| LLM | Anthropic or OpenAI API |
| Web client | SvelteKit 5, Tailwind CSS |
| Desktop | Tauri 2 |
| Memory | File-based storage with keyword and vector search |
| Extensions | Skills and Model Context Protocol |
| Distribution | Native binaries, launchd, and systemd |

### Data layout

Everything important is stored as files under `~/.nolune`:

```text
~/.nolune/
├── config.toml
├── browser_sessions.json    paired browsers (hashes only)
└── instances/
    └── companion/               the one companion this server hosts
        ├── soul.md              personality definition
        ├── heartbeat.md         proactive behavior
        ├── mood.json            emotional state
        ├── memory/              long-term memory library
        ├── drops/               autonomous creative artifacts
        ├── uploads/             user-uploaded files
        ├── skills/              installed skills
        └── chats/               conversation history
```

<br>

## Configuration

Most settings are available through the interface. Advanced configuration lives at `~/.nolune/config.toml`.

| Environment variable | Description |
|----------------------|-------------|
| `NOLUNE_HOME` | Data directory, defaults to `~/.nolune` |
| `NOLUNE_AUTH_TOKEN` | API token override. The token is for automation, the CLI and the desktop app; browsers pair for a revocable session instead and are unaffected when it changes |
| `NOLUNE_PUBLIC_URL` | Public URL for the server. Defaults to `http://localhost:<port>`; set it when you reach Nolune through another address so shared file links work |
| `ANTHROPIC_API_KEY` | Anthropic API key override |
| `OPENAI_API_KEY` | OpenAI API key override |
| `RUST_LOG` | Logging level, defaults to `info` |

<br>

## Updates

Nolune checks for updates automatically. Apply an update through Settings or run:

```bash
~/.nolune/bin/update
```

Then restart the server: `nolune gateway restart` if it runs as a background service, otherwise stop it with Ctrl-C and run `nolune gateway` again (or reopen the desktop app, which restarts the server it manages). The desktop app's own updater only updates the app.

### Uninstall

```bash
curl -fsSL https://nolune.dev/uninstall.sh | bash
```

Keep your data while removing the server:

```bash
KEEP_DATA=1 curl -fsSL https://nolune.dev/uninstall.sh | bash
```

The script delegates to `nolune uninstall --keep-data` or `nolune uninstall --yes`, which you can also run directly. Without a terminal the script defaults to keeping your data.

<br>

## Development

See the [Nolune launch cutover checklist](docs/nolune-cutover.md) for repository, domain, and release setup.

```bash
# Server
cd server && cargo run

# Web client
cd client && pnpm install && pnpm dev

# Desktop app
cd desktop && pnpm install && pnpm tauri dev

# Landing site
cd landing && pnpm install && pnpm dev
```

Use `pnpm`, not npm, for the JavaScript workspaces. See [CONTRIBUTING.md](CONTRIBUTING.md) for the complete development setup.

## Security

Computer use and remote access are powerful capabilities. Only connect machines you control, keep your authentication token private, and review the permissions granted to the desktop app.

See [SECURITY.md](SECURITY.md) to report a vulnerability.

## License

MIT — see [LICENSE](LICENSE).

Computer use is driven by a pinned [Cua Driver](https://github.com/trycua/cua) 0.28.2 (MIT, Cua AI, Inc.), verified by checksum and never updated on its own; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

<br>

<p align="center">
  <img src="client/src/lib/assets/favicon.svg" alt="Nolune" width="32" />
  <br><br>
  <sub>Built by <a href="https://triangleint.com">Triangle Interactive LLC</a></sub>
</p>
