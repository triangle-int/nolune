<script lang="ts">
	import { sections } from '$lib/docs';
</script>

<svelte:head>
	<title>Self-hosting docs — Nolune</title>
	<meta name="description" content="Prerequisites, install, upgrade, backup, uninstall and troubleshooting for a self-hosted Nolune server. No account, no hosted service." />
</svelte:head>

<article>
	<p class="eyebrow">Self-hosting docs</p>
	<h1 class="section-title">Run Nolune on hardware you own.</h1>
	<p class="docs-lede">Nolune is one server, one companion, and the computers you connect to it. These pages cover the whole life of that server: what it needs, how to install and pair it, how to keep it current, how to back it up, and how to remove it. There is no account and no hosted service; the source is on GitHub, and the install script you run is served from this repository.</p>

	<h2>Quick start</h2>
	<ol class="steps">
		<li>Give the server a home on an always-on Mac or Linux computer. Choose <strong>Install on this computer</strong> in the <a href="https://github.com/triangle-int/nolune/releases" target="_blank" rel="noopener">desktop app</a>, or run the one-liner in a terminal:
			<pre><code>curl -fsSL https://nolune.dev/install.sh | bash</code></pre></li>
		<li>Pair the browser that opens at <code>http://localhost:26559</code>: run <code>nolune pair</code> on the same computer and enter the one-time code it prints.</li>
		<li>Follow the onboarding, add your model provider key under <strong>Settings → Connections</strong>, and meet your companion.</li>
	</ol>
	<p>Everything else, from what the installer needs to how to update or uninstall, is one of the sections below.</p>
</article>

<div class="docs-cards">
	{#each sections as entry, i (entry.slug)}
		<a class="docs-card" href="/docs/{entry.slug}">
			<span class="number">0{i + 1}</span>
			<strong>{entry.title}</strong>
			<p>{entry.summary}</p>
			<span class="read">Read <span aria-hidden="true">→</span></span>
		</a>
	{/each}
</div>

<article>
	<h2>Also useful</h2>
	<ul>
		<li><a href="https://github.com/triangle-int/nolune" target="_blank" rel="noopener">The source and README on GitHub</a>, including the data layout and every environment variable.</li>
		<li><a href="https://github.com/triangle-int/nolune/releases" target="_blank" rel="noopener">Releases</a>: server binaries and desktop apps for macOS, Windows and Linux.</li>
		<li>The <a href="/install.sh">install script</a> and <a href="/uninstall.sh">uninstall script</a> exactly as this site serves them, so you can read them before you run them.</li>
		<li>The <a href="/skills">skills library</a>: abilities the community has taught Nolune.</li>
		<li><a href="https://github.com/triangle-int/nolune/blob/main/SECURITY.md" target="_blank" rel="noopener">Security policy</a> for reporting a vulnerability.</li>
	</ul>
</article>

<style>
	.docs-cards{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:1rem;margin:2rem 0 1rem}
	.docs-card{display:flex;flex-direction:column;gap:.5rem;padding:1.4rem 1.3rem;border:1px solid var(--color-border);border-radius:12px;background:var(--surface);transition:border-color .2s}
	.docs-card:hover{border-color:var(--color-warm)}
	.docs-card .number{font-size:.68rem;letter-spacing:.08em;color:var(--color-warm)}
	.docs-card strong{font-size:1.1rem;font-weight:500;letter-spacing:-.02em;color:var(--color-text)}
	.docs-card p{margin:0;font-size:.88rem;line-height:1.6;color:var(--color-text-dim)}
	.docs-card .read{margin-top:auto;padding-top:.6rem;font-size:.78rem;color:var(--color-text-dim)}
	.docs-card:hover .read{color:var(--color-text)}
	@media(max-width:600px){.docs-cards{grid-template-columns:1fr}}
</style>
