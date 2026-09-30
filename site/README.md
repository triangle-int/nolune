# nolune.dev

The landing page: a SvelteKit site that prerenders to static files with `@sveltejs/adapter-static`.
It takes the assistant avatars from `@nolune/core/avatars`, so the page draws the same mascots as the
app.

```sh
pnpm --filter @nolune/site dev      # http://localhost:5173
pnpm --filter @nolune/site check
pnpm --filter @nolune/site build    # writes site/build
```

To deploy on Vercel, set the project's Root Directory to `site`. adapter-static notices Vercel and
writes its output format, so there's nothing else to configure.

`static/og.png`, the picture shown when someone shares a link, is a 1200×630 screenshot of the hero
with reduced motion on. Take a new one when the hero changes.
