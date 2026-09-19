<script lang="ts">
	import { page } from "$app/state";
	import ConnectedComputers from "$lib/components/computers/ConnectedComputers.svelte";

	const slug = $derived(page.params.slug!);
</script>

<div class="computers-page">
	<header class="computers-header">
		<h2>Computers</h2>
		<p>Desktops your companion can see and act on. Each one is a place it can be, not a separate companion.</p>
	</header>

	{#key slug}
		<ConnectedComputers {slug} />
	{/key}

	<section class="computers-howto nl-panel">
		<h3>Connect another computer</h3>
		<ol>
			<li>Install the Nolune desktop app on that computer.</li>
			<li>Give it this server's address and its API token from <a href={`/${slug}/settings/advanced`}>Settings › Advanced</a>.</li>
			<li>When it connects, your companion notices under <a href={`/${slug}/activity`}>Activity</a>.</li>
		</ol>
	</section>
</div>

<style>
	.computers-page { height: 100%; overflow-y: auto; padding: 32px 24px calc(32px + env(safe-area-inset-bottom, 0px)); display: flex; flex-direction: column; gap: 24px; max-width: 720px; margin: 0 auto; width: 100%; }
	.computers-header h2 { font: 400 28px/1.2 var(--font-display); letter-spacing: -0.02em; color: var(--foreground); margin: 0 0 8px; }
	.computers-header p { font: 400 14px/1.6 var(--font-body); color: var(--text-secondary); margin: 0; }
	.computers-howto { display: flex; flex-direction: column; gap: 8px; }
	.computers-howto h3 { font: 500 16px var(--font-body); color: var(--foreground); margin: 0; }
	.computers-howto ol { margin: 0; padding-left: 20px; font: 400 14px/1.7 var(--font-body); color: var(--text-secondary); }
	.computers-howto a { color: var(--primary); text-decoration: underline; text-underline-offset: 3px; }
	@media (max-width: 720px) { .computers-page { padding: 24px 16px calc(24px + env(safe-area-inset-bottom, 0px)); } }
</style>
