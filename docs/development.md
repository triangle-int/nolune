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
migrations. The LaunchAgent that `nolune service install` writes points at `dist/cli.js`, so keep it
where it is: if it moved, an update would leave the LaunchAgent unable to start the gateway until
`nolune service install` ran again.

To release, set `version` in `package.json`, merge it to `main`, then push a matching tag:
`git tag v0.1.0 && git push origin v0.1.0`. The Publish workflow (`.github/workflows/publish.yml`)
checks the tag against `package.json`, runs format, lint, types and tests, and publishes with
`npm publish`, unless that version is on npm already. It signs in to npm with
[trusted publishing](https://docs.npmjs.com/trusted-publishers/), so there's no npm token in the
repository's secrets, and npm adds provenance on its own.

Once, by hand: publish the first version from your computer (`npm login`, then `npm publish`), since
a trusted publisher is set on a package that exists, and push its tag as usual (the workflow finds
it on npm and only runs the checks). Then, on npmjs.com under the package's
Settings, add a trusted publisher (GitHub Actions, this repository, `publish.yml`) that may publish
with `npm publish`, and under Publishing access require two-factor authentication and disallow
tokens.
