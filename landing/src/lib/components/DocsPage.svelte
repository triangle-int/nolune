<script lang="ts">
	import type { Snippet } from 'svelte';
	import { section, sections } from '$lib/docs';

	let { slug, children }: { slug: string; children: Snippet } = $props();

	const entry = $derived(section(slug));
	const index = $derived(sections.findIndex((candidate) => candidate.slug === slug));
	const previous = $derived(index > 0 ? sections[index - 1] : null);
	const next = $derived(index < sections.length - 1 ? sections[index + 1] : null);
	const number = $derived(String(index + 1).padStart(2, '0'));
</script>

<svelte:head>
	<title>{entry.title} — Nolune docs</title>
	<meta name="description" content={entry.summary} />
</svelte:head>

<article>
	<p class="eyebrow">Self-hosting docs · {number}</p>
	<h1 class="section-title">{entry.title}</h1>
	{@render children()}
</article>

<nav class="pager" aria-label="Docs pages">
	{#if previous}
		<a href="/docs/{previous.slug}" rel="prev"><span>Previous</span><strong>{previous.title}</strong></a>
	{:else}
		<a href="/docs" rel="prev"><span>Back to</span><strong>Overview</strong></a>
	{/if}
	{#if next}
		<a class="next" href="/docs/{next.slug}" rel="next"><span>Next</span><strong>{next.title}</strong></a>
	{/if}
</nav>

<style>
	.pager{display:grid;grid-template-columns:1fr 1fr;gap:1rem;margin-top:4rem;padding-top:2rem;border-top:1px solid var(--color-border)}
	.pager a{display:flex;flex-direction:column;gap:.3rem;padding:1rem 1.1rem;border:1px solid var(--color-border);border-radius:10px;transition:border-color .2s}
	.pager a:hover{border-color:var(--color-warm)}.pager .next{text-align:right;grid-column:2}
	.pager span{font-size:.65rem;letter-spacing:.1em;text-transform:uppercase;color:var(--color-text-ghost)}.pager strong{font-size:.9rem;font-weight:500;color:var(--color-text)}
	@media(max-width:480px){.pager{grid-template-columns:1fr}.pager .next{grid-column:auto;text-align:left}}
</style>
