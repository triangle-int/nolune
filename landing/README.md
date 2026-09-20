# Nolune landing site

The public site at [nolune.dev](https://nolune.dev): the product description,
the interactive Little Moon demo, the skills library, the self-hosting docs,
and the legal pages. It is a SvelteKit app that prerenders to static files;
nothing runs on a request path, there is no database, no analytics, and no
credentials are needed to build or deploy it.

## Routes

| Route | Source | Notes |
| --- | --- | --- |
| `/` | `src/routes/+page.svelte` | Hero, story, install, call to action |
| `/docs`, `/docs/<section>` | `src/routes/docs/` | Self-hosting docs: prerequisites, install, upgrade, backup, uninstall, troubleshooting; the section list lives in `src/lib/docs.ts` |
| `/skills` | `src/routes/skills/+page.svelte` | Reads the community registry in the browser |
| `/privacy`, `/terms` | `src/routes/{privacy,terms}/` | Legal pages |
| `/install.sh`, `/uninstall.sh` | `src/routes/{install,uninstall}.sh/+server.ts` | The repository's `scripts/install.sh` and `scripts/uninstall.sh`, imported with `?raw` at build time so the served file is the one at the deployed commit. `vercel.json` serves them as `text/plain` |

`src/routes/+layout.ts` sets `prerender = true` for the whole site, and
`svelte.config.js` turns a dangling internal link or hash into a build error.

## Developing

```sh
pnpm install
pnpm dev
```

Use `pnpm`, not npm. Tokens and shared classes (`.section-shell`, `.eyebrow`,
`.section-title`) are in `src/app.css`; read
[docs/design-system.md](../docs/design-system.md) before changing the UI.

## Checks

```sh
pnpm check                                  # svelte-check
pnpm build                                  # prerenders into .vercel/output
python3 ../scripts/tests/landing-static-site.py   # static output and link check
```

The Python check runs in CI after the build. It fails when a route becomes a
serverless function, an internal link or hash does not resolve, the served
installer scripts differ from the repository's, `vercel.json` rewrites a path
off-site, or the docs index stops naming a section. Source-level guards live
in `server/tests/landing_static_site.rs` and `server/tests/identity_language.rs`
(the docs copy must describe one self-hosted companion, never accounts or
hosted plans).

## Deploying

The site deploys to Vercel with `@sveltejs/adapter-vercel`. Every route is a
prerendered file under `.vercel/output/static`; the only function the adapter
emits is its internal 404 catch-all. Copy changes need no redeploy of anything
else, and a server release never touches the site.
