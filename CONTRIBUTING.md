# Contributing to Nolune

Thanks for your interest in contributing! Here's how to get started.

## Development Setup

### Quick start

You need [rustup](https://rustup.rs) and Node.js 22 LTS; on Linux also
`pkg-config` and `libssl-dev`. rustup picks up the Rust version from
`rust-toolchain.toml`, and the script enables the pinned pnpm through corepack.

```bash
./scripts/dev.sh
```

That installs the client dependencies, builds and starts the server, and runs
the web client with hot reload. Open `http://localhost:5173`.

- **Separate data.** The dev server keeps its data in `.dev/home` inside the
  checkout (`--fresh` wipes it), never in `~/.nolune`.
- **Runs beside an install.** It listens on port 26560 (`NOLUNE_DEV_PORT`), so
  an installed Nolune on 26559 keeps running.
- **No pairing.** The dev server listens on loopback only with auth disabled.
- **Model keys.** Put `ANTHROPIC_API_KEY`, `OPENAI_API_KEY` or
  `OPENROUTER_API_KEY` in a `.env` file at the repo root (it is gitignored), or
  add a provider under **Settings → Connections**.
- **Restart after Rust changes.** Client changes reload in place; after
  changing Rust, stop with Ctrl-C and rerun the script.

The sections below run each piece by hand.

### Server

The server embeds `client/build`, so build the web client once first
(`pnpm --dir client install && pnpm --dir client build`). Then:

```bash
cargo run --manifest-path server/Cargo.toml -- onboard   # writes ~/.nolune/config.toml with a token
cargo run --manifest-path server/Cargo.toml -- gateway   # runs the server in the foreground
cargo run --manifest-path server/Cargo.toml -- pair      # second terminal: one-time code for the first browser
```

`nolune onboard` is safe to rerun. Because it generates a token, the web client
at `http://localhost:26559` opens on a pairing gate: enter the code that
`nolune pair` printed, then add your model provider API key under
**Settings → Connections**, or edit `~/.nolune/config.toml` by hand;
`server/config.example.toml` documents every section. Point `NOLUNE_HOME` at a
scratch directory to keep a development server away from your real data.

### Client

```bash
cd client
pnpm install
pnpm dev
```

### Landing

```bash
cd landing
pnpm install
pnpm dev
```

The site is fully static: no environment file, database, or credentials are
needed to build or deploy it (see [landing/README.md](landing/README.md)).

### Desktop (Tauri)

```bash
cd desktop
pnpm install
pnpm tauri dev
```

## Pull Requests

1. Fork the repo and create your branch from `main`
2. Make your changes
3. Test your changes locally
4. Create a pull request with a clear description

## Versioning

Single source of truth: `VERSION` file in repo root.

```bash
./scripts/bump-version.sh 0.20.0
```

## Code Style

- **Rust**: `cargo fmt` + `cargo clippy`
- **TypeScript/Svelte**: follow existing patterns
- **Commits**: short imperative descriptions

## CI checks

GitHub Actions runs `.github/workflows/ci.yml` on pull requests, pushes to
`main`, and manual dispatches. It checks Rust formatting, tests and Clippy,
all three Svelte frontends, desktop JavaScript tests,
and the installer/release shell regression tests. Rust desktop checks run on
macOS; server checks run on Linux. CI uses the Rust version in
`rust-toolchain.toml`, Node.js 22, and pnpm 10.34.5.

To run the checks locally:

```bash
cargo fmt --all -- --check
for project in client desktop landing; do
  pnpm --dir "$project" install --frozen-lockfile
  pnpm --dir "$project" check
  pnpm --dir "$project" build
done
pnpm --dir desktop test
# Build the frontends above before checking Rust (the server embeds client/build).
# Native dependencies are the same as for development of each package.
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets
bash scripts/tests/install.sh
bash scripts/tests/release-workflow.sh
```

## Reporting Issues

Use [GitHub Issues](https://github.com/triangle-int/nolune/issues). Include:
- Steps to reproduce
- Expected vs actual behavior
- Server logs if applicable

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
