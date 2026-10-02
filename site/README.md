# nolune.dev

The landing page: a SvelteKit site that prerenders to static files with `@sveltejs/adapter-static`.
It takes the assistant avatars from `@nolune/core/avatars`, so the page draws the same mascots as the
app.

Every page shares the header and footer in `src/routes/+layout.svelte`. Its section links point at
the home page (`/#features`), so they work from any page, and `.wrap` and `.pill` in `src/app.css`
are there for new pages to use.

```sh
pnpm --filter @nolune/site dev      # http://localhost:5173
pnpm --filter @nolune/site check
pnpm --filter @nolune/site build    # writes site/build
```

To deploy on Vercel, set the project's Root Directory to `site`. adapter-static notices Vercel and
writes its output format, so there's nothing else to configure.

`/docs` is a separate site ([docs/](../docs)), deployed as its own Vercel project. `vercel.json`
forwards `/docs` and everything under it to that project's address, so it opens on this domain.
The rule is `/docs/:path(.*)` rather than `/docs/:path*`, which on Vercel doesn't match a path
ending in a slash, as every docs page does. Its
links here skip SvelteKit's router, and the prerender ignores them. Locally, run the docs with
`pnpm --filter @nolune/docs dev`.

Pricing (`#pricing`, and its link in the header) sells the nolune plan. It shows once the plan is
open (`NOLUNE_PLAN_OPEN` in `@nolune/core/nolune-plan-open`, the switch the gateway reads too), and
before that only on Vercel's previews and the dev server (`__PREVIEW__`, set in `vite.config.ts`).
Its prices and the launch offer are written in the page: change them with Stripe's. Subscribe goes
to the account page on nolune's API, which signs people in and opens Checkout.

`/privacy` is nolune's privacy policy, which the App Store listing and the iOS app link. It says
what nolune, its apps, the relay, the nolune plan's API and this site do with people's information,
so a change to any of them that sends or keeps something new belongs there too, with a new date.

`/terms` are the nolune plan's terms, which Stripe's Checkout and customer portal and the account
page on nolune's API link. They say what the plan gives, how it renews, ends and is refunded, and
how it may be used, so a change to any of that in `packages/api` (or to the prices in Stripe)
belongs there too, with a new date. Both pages are `Prose` (`src/lib/Prose.svelte`).

`static/og.png`, the picture shown when someone shares a link, is a 1200×630 screenshot of the hero
with reduced motion on. Take a new one when the hero changes.
