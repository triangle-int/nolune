<script lang="ts">
	import { page } from '$app/state';
	import Nav from '$lib/components/Nav.svelte';
	import Footer from '$lib/components/Footer.svelte';
	import { sections } from '$lib/docs';

	let { children } = $props();
	const current = $derived(page.url.pathname.replace(/\/$/, '') || '/');
</script>

<a class="skip" href="#main">Skip to content</a>
<Nav />
<main id="main" class="docs">
	<div class="section-shell docs-shell">
		<aside class="docs-side">
			<nav aria-label="Self-hosting docs">
				<a href="/docs" aria-current={current === '/docs' ? 'page' : undefined}>Overview</a>
				{#each sections as entry (entry.slug)}
					<a href="/docs/{entry.slug}" aria-current={current === `/docs/${entry.slug}` ? 'page' : undefined}>{entry.title}</a>
				{/each}
			</nav>
		</aside>
		<div class="docs-body">{@render children()}</div>
	</div>
</main>
<Footer />

<style>
	.skip{position:fixed;top:1rem;left:1rem;z-index:200;background:#f6f3ec;color:#201d29;padding:1rem;transform:translateY(-200%)}.skip:focus{transform:translateY(0)}
	.docs{background:var(--color-bg);min-height:100vh}
	.docs-shell{display:grid;grid-template-columns:200px minmax(0,1fr);gap:4rem;align-items:start;padding-top:8.5rem}
	.docs-side{position:sticky;top:6rem}.docs-side nav{display:flex;flex-direction:column;gap:.15rem}
	.docs-side a{display:block;padding:.6rem .85rem;border-radius:6px;font-size:.85rem;color:var(--color-text-dim);border-left:2px solid transparent;transition:color .2s,background .2s}
	.docs-side a:hover{color:var(--color-text);background:#ffffff08}.docs-side a[aria-current=page]{color:var(--color-text);background:var(--color-warm-glow);border-left-color:var(--color-warm)}

	/* Long-form prose. The pages hand in plain HTML, so the rules are global within the body. */
	.docs-body{max-width:760px}
	.docs-body :global(.section-title){font-size:clamp(2.3rem,4vw,3.3rem);margin:.9rem 0 1.25rem}
	.docs-body :global(.docs-lede){font-size:1.08rem;line-height:1.7;color:#d4cddb;margin:0 0 2.5rem;max-width:640px}
	.docs-body :global(h2){font:500 1.4rem/1.3 var(--font-body);letter-spacing:-.02em;margin:3rem 0 1rem;padding-top:2.25rem;border-top:1px solid var(--color-border);color:var(--color-text)}
	.docs-body :global(h3){font:500 1.05rem/1.4 var(--font-body);margin:2rem 0 .6rem;color:var(--color-text)}
	.docs-body :global(p),.docs-body :global(li){font-size:.95rem;line-height:1.75;color:#d4cddb}
	.docs-body :global(p){margin:0 0 1rem}.docs-body :global(ul),.docs-body :global(ol){margin:0 0 1.25rem;padding-left:1.4rem}.docs-body :global(li){margin:.35rem 0}
	.docs-body :global(strong){color:var(--color-text);font-weight:500}
	.docs-body :global(article a){color:var(--color-text);border-bottom:1px solid var(--color-border-warm);transition:border-color .2s}.docs-body :global(article a:hover){border-bottom-color:var(--color-warm)}
	.docs-body :global(code),.docs-body :global(kbd){font:.85em var(--font-mono);color:var(--color-text);background:#ffffff0d;border:1px solid var(--color-border);border-radius:4px;padding:.1em .4em}
	.docs-body :global(pre){margin:1.25rem 0 1.5rem;padding:1rem 1.2rem;background:var(--surface);border:1px solid var(--color-border-warm);border-radius:10px;overflow-x:auto;scrollbar-width:thin;font:.8rem/1.7 var(--font-mono);color:var(--color-warm)}
	.docs-body :global(pre code){font:inherit;color:inherit;background:none;border:0;padding:0;white-space:pre}
	.docs-body :global(.table-scroll){overflow-x:auto;scrollbar-width:thin;margin:1.25rem 0 1.5rem;border:1px solid var(--color-border);border-radius:10px}
	.docs-body :global(table){width:100%;border-collapse:collapse;font-size:.88rem}
	.docs-body :global(th),.docs-body :global(td){text-align:left;vertical-align:top;padding:.75rem .9rem;border-bottom:1px solid var(--color-border);line-height:1.6;color:#d4cddb}
	.docs-body :global(th){font-size:.7rem;letter-spacing:.1em;text-transform:uppercase;color:var(--color-text-ghost);font-weight:500;background:var(--surface)}
	.docs-body :global(tr:last-child td){border-bottom:0}.docs-body :global(td:first-child code){white-space:nowrap}
	.docs-body :global(.callout){margin:1.5rem 0;padding:.9rem 1.1rem;border-left:2px solid var(--color-warm);background:var(--color-warm-ghost);border-radius:0 8px 8px 0}.docs-body :global(.callout p){margin:0}.docs-body :global(.callout p + p){margin-top:.6rem}
	.docs-body :global(.steps){counter-reset:step;list-style:none;padding:0;margin:0 0 1.5rem}
	.docs-body :global(.steps li){position:relative;padding-left:2.6rem;margin:0 0 1rem}
	.docs-body :global(.steps li::before){counter-increment:step;content:'0' counter(step);position:absolute;left:0;top:.2rem;font-size:.7rem;letter-spacing:.06em;color:var(--color-warm)}

	@media(max-width:820px){
		/* minmax(0,1fr): the nowrap section nav must not widen the column past the viewport. */
		.docs-shell{grid-template-columns:minmax(0,1fr);gap:2rem;padding-top:6.5rem}
		.docs-side{position:static;min-width:0;margin:0 -1.4rem;padding:0 1.4rem;border-bottom:1px solid var(--color-border)}
		.docs-side nav{flex-direction:row;gap:.25rem;overflow-x:auto;scrollbar-width:none;padding-bottom:.5rem}
		.docs-side a{white-space:nowrap;border-left:0;border-bottom:2px solid transparent;border-radius:6px 6px 0 0}
		.docs-side a[aria-current=page]{border-bottom-color:var(--color-warm);background:none}
	}
	@media(max-width:600px){
		/* Tables scroll inside .table-scroll instead of squeezing the second column. */
		.docs-body :global(table){min-width:540px}
	}
</style>
