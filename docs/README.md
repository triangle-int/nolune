# nolune docs

The documentation at [nolune.dev/docs](https://nolune.dev/docs/): an [Astro](https://astro.build)
site with [Starlight](https://starlight.astro.build). The pages are Markdown (or MDX, for
components) in [`src/content/docs`](src/content/docs), and the sidebar is in `astro.config.mjs`.

```sh
pnpm --filter @nolune/docs dev      # http://localhost:4321/docs/
pnpm --filter @nolune/docs check
pnpm --filter @nolune/docs build    # writes dist/docs
```

It's built for the path `/docs` (`base` in `astro.config.mjs`) and into `dist/docs`, so it has the
same paths on its own Vercel project as on nolune.dev, which forwards `/docs` to it
([`site/vercel.json`](../site/vercel.json)). The build also writes the search index (Pagefind) and
`llms.txt`, `llms-full.txt` and `llms-small.txt` for assistants that read docs.

[Commands](src/content/docs/reference/commands.md) follows `nolune help`: when the CLI's help
changes, update the page too.

The screenshots, demo and images next to this file are the README's; pages use them with relative
paths.
