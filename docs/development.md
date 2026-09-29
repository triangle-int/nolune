# Development

Requires pnpm. Node runs the TypeScript in `packages/` directly (type stripping), so no build step
is needed for the CLI while developing.

```sh
pnpm install
pnpm nolune setup          # same CLI, from source
cp .env.example .env    # ORIGIN=http://localhost:5173
pnpm dev
```

```sh
pnpm check                          # svelte-check + tsc for packages/core and packages/cli
pnpm lint                           # prettier --check + eslint (pnpm format to fix formatting)
pnpm test                           # vitest: *.test.ts next to the code in src/ and packages/
pnpm db:generate --name <change>    # after editing packages/core/src/db/schema.ts
pnpm build                          # web build + dist/cli.js (what the npm package ships)
pnpm start                          # run the built gateway with the settings from nolune config
```

Migrations are applied automatically when the gateway or the CLI opens the database.

Every test starts with an empty `NOLUNE_HOME` in a temp folder (`packages/core/src/test/setup.ts`), so
tests of the database code run against a fresh, migrated SQLite file and never touch your data.

CI (`.github/workflows/ci.yml`) runs format, lint, types, tests and the build on every pull request
and on pushes to `main`.

## Publishing

`npm pack` builds and packs `build/` (without source maps), `dist/cli.js` and the migrations.

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
