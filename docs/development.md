# Development

Requires pnpm. The workspace has three packages: `packages/web` (the SvelteKit app, which is also
the gateway), `packages/core` (the agent and storage) and `packages/cli`. Node runs the TypeScript
in `packages/core` and `packages/cli` directly (type stripping), so no build step is needed for the
CLI while developing.

```sh
pnpm install
pnpm nolune setup          # same CLI, from source
cp packages/web/.env.example packages/web/.env    # ORIGIN=http://localhost:5173
pnpm dev
```

```sh
pnpm check                          # each package's check: svelte-check for web, tsc for the rest
pnpm lint                           # prettier --check + eslint (pnpm format to fix formatting)
pnpm test                           # each package's vitest: *.test.ts next to the code
pnpm db:generate --name <change>    # after editing packages/core/src/db/schema.ts
pnpm build                          # packages/web/build + dist/cli.js (what the npm package ships)
pnpm start                          # run the built gateway with the settings from nolune config
```

Migrations are applied automatically when the gateway or the CLI opens the database.

The relay (`packages/relay`) is a separate server, deployed on its own: see its
[README](../packages/relay/README.md) for running it locally and in production.

Every test starts with an empty `NOLUNE_HOME` in a temp folder (`packages/core/src/test/setup.ts`), so
tests of the database code run against a fresh, migrated SQLite file and never touch your data.
Each package runs its own tests (`pnpm --filter @nolune/core test:watch` to watch one): SvelteKit
finds its app from the folder it runs in, so the web tests can't run from the repository root.

The root `package.json` is the `nolune` package that goes to npm; the packages in `packages/` are
private. The web app's `dependencies` (only `better-sqlite3`) are what adapter-node leaves out of
`packages/web/build`; everything else is bundled, so keep the web app's other packages in
`devDependencies`.

CI (`.github/workflows/ci.yml`) runs format, lint, types, tests and the build on every pull request
and on pushes to `main`.

## Publishing

`npm pack` builds and packs `packages/web/build/` (without source maps), `dist/cli.js` and the
migrations. The service that `nolune service install` writes (a LaunchAgent on macOS, a systemd
user unit on Linux) points at `dist/cli.js`, so keep it where it is: if it moved, an update would
leave the service unable to start the gateway until `nolune service install` ran again.

To release, set `version` in `package.json`, merge it to `main`, then push a matching tag:
`git tag v0.1.0 && git push origin v0.1.0`. The Publish workflow (`.github/workflows/publish.yml`)
checks the tag against `package.json`, runs format, lint, types and tests, and publishes with
`npm publish`, unless that version is on npm already. It signs in to npm with
[trusted publishing](https://docs.npmjs.com/trusted-publishers/), so there's no npm token in the
repository's secrets, and npm adds provenance on its own.

The same tag builds the macOS app (`macos/scripts/build-app.sh`) on a Mac for each architecture,
checks that it runs, and, once npm has the version, makes the GitHub release with
`nolune-macos-apple-silicon.dmg` and `nolune-macos-intel.dmg`. Named without the version, they're
always at `https://github.com/triangle-int/nolune/releases/latest/download/<name>`, for the site to
link to. Running Publish by hand (Actions > Publish > Run workflow) only builds the app and
drafts the next release's notes, which is the way to try the signing and read the notes without
releasing anything: the disk images are in the run's artifacts, the notes in its summary.

Claude writes the release notes (`scripts/release-notes.mjs`), for the families who use nolune,
from the commits since the last tag: squash-merged pull requests, whose messages say what changed
and why, so a clear message makes clear notes. `scripts/release-notes/example.md` (0.2.0's) sets
their shape and tone; the workflow adds the download links and the full changelog. It needs the
`ANTHROPIC_API_KEY` secret, an Anthropic API key. Without it, or when Claude can't, the release
lists the pull requests instead. Notes are only written when the release is made, so a re-run
leaves them, and they can be edited on the release page like any.

For macOS to open the app on other Macs, it's signed with a Developer ID and notarized by Apple,
with these secrets (Settings > Secrets and variables > Actions). Without them the app is ad-hoc
signed, the run warns, and the release says how to open it anyway.

- `MACOS_CERTIFICATE` and `MACOS_CERTIFICATE_PASSWORD`: a Developer ID Application certificate
  (it needs the [Apple Developer Program](https://developer.apple.com/programs/); make it in Xcode,
  Settings > Accounts > Manage Certificates). Export it with its private key from Keychain Access
  as a .p12 with a password, then `base64 -i certificate.p12 | pbcopy` for the first secret.
- `APPLE_API_KEY`, `APPLE_API_KEY_ID` and `APPLE_API_ISSUER`, for notarizing: in App Store Connect,
  Users and Access > Integrations > App Store Connect API, make a team key with the Developer
  role. The first secret is the downloaded .p8 file's contents, the second the key's ID, the third
  the Issuer ID above the list.

Once, by hand: publish the first version from your computer (`npm login`, then `npm publish`), since
a trusted publisher is set on a package that exists, and push its tag as usual (the workflow finds
it on npm and only runs the checks). Then, on npmjs.com under the package's
Settings, add a trusted publisher (GitHub Actions, this repository, `publish.yml`) that may publish
with `npm publish`, and under Publishing access require two-factor authentication and disallow
tokens.
