<script lang="ts">
	import type { Snippet } from 'svelte';

	/** A page of prose, as the privacy policy and the terms are: one column, in the site's type. */
	let { children }: { children: Snippet } = $props();
</script>

<main class="wrap">
	<article>
		{@render children()}
	</article>
</main>

<style>
	article {
		max-width: 72ch;
		padding-block: 72px 96px;
		line-height: 1.75;
	}
	article :global(h1),
	article :global(h2) {
		color: var(--cream);
		font-family: var(--sans);
	}
	article :global(h1) {
		margin: 0;
		font-weight: 900;
		font-size: clamp(2.6rem, 5vw, 4rem);
		letter-spacing: -0.03em;
	}
	article :global(h2) {
		margin: 56px 0 0;
		font-weight: 800;
		font-size: 1.6rem;
		letter-spacing: -0.01em;
	}
	article :global(.updated) {
		margin-top: 12px;
		color: var(--muted);
		font-size: 0.9rem;
	}
	article :global(.lede) {
		margin-top: 32px;
		font-size: 1.1rem;
	}
	article :global(p),
	article :global(ul) {
		margin: 18px 0 0;
	}
	article :global(ul) {
		padding-left: 1.2em;
	}
	article :global(li + li) {
		margin-top: 10px;
	}
	/* The page's own monospace, at its size (browsers make `code` smaller). */
	article :global(code) {
		font: inherit;
		color: var(--cream);
		overflow-wrap: anywhere;
	}
	article :global(a) {
		color: var(--cream);
		text-underline-offset: 3px;
	}
	article :global(a:hover) {
		color: var(--rust);
	}

	@media (max-width: 560px) {
		article {
			padding-block: 48px 72px;
		}
	}
</style>
