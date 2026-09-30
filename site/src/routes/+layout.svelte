<script lang="ts">
	import '../app.css';
	import { resolve } from '$app/paths';
	import Wordmark from '$lib/Wordmark.svelte';

	let { children } = $props();

	// The header and footer every page shares. The section links point at the home page, so they
	// work from any page.
	const home = resolve('/');
	const GITHUB = 'https://github.com/triangle-int/nolune';
	const NPM = 'https://www.npmjs.com/package/nolune';
</script>

<header class="bar">
	<div class="wrap bar-row">
		<a href={home} class="home"><Wordmark class="wordmark" /></a>
		<nav>
			<a href="{home}#features">Features</a>
			<a href="{home}#how">How it works</a>
			<a href={GITHUB}>GitHub</a>
		</nav>
		<a href="{home}#how" class="pill">Install</a>
	</div>
</header>

{@render children()}

<footer>
	<div class="wrap footer-row">
		<Wordmark class="wordmark" />
		<nav>
			<a href={GITHUB}>GitHub</a>
			<a href={NPM}>npm</a>
			<a href="{GITHUB}/blob/main/LICENSE">MIT license</a>
		</nav>
		<p>Made by Triangle Interactive</p>
	</div>
</footer>

<style>
	.bar {
		border-bottom: 1px solid var(--line);
	}
	.bar-row {
		display: flex;
		gap: 40px;
		align-items: center;
		height: 80px;
	}
	.bar nav {
		display: flex;
		gap: 44px;
		margin-left: auto;
		font-size: 0.95rem;
	}
	nav a {
		color: var(--text);
		text-decoration: none;
	}
	nav a:hover {
		color: var(--cream);
	}
	.bar :global(.wordmark),
	footer :global(.wordmark) {
		display: block;
		height: 30px;
		width: auto;
		color: var(--cream);
	}
	footer {
		border-top: 1px solid var(--line);
	}
	.footer-row {
		display: flex;
		gap: 40px;
		align-items: center;
		padding-top: 32px;
		padding-bottom: 32px;
		font-size: 0.9rem;
	}
	footer nav {
		display: flex;
		gap: 28px;
		margin-left: auto;
	}
	footer p {
		margin: 0;
	}

	@media (max-width: 960px) {
		.bar nav {
			display: none;
		}
		.bar-row .pill {
			margin-left: auto;
		}
		.footer-row {
			flex-wrap: wrap;
			gap: 20px 28px;
		}
		footer nav {
			margin-left: 0;
		}
	}
</style>
