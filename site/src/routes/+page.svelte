<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import CheckIcon from '@lucide/svelte/icons/check';
	import LaptopIcon from '@lucide/svelte/icons/laptop';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import CodeXmlIcon from '@lucide/svelte/icons/code-xml';
	import { resolve } from '$app/paths';
	import { AVATARS, type Avatar } from '@nolune/core/avatars';
	import { AVATAR_COLORS } from '$lib/avatars';
	import ChatWindow from '$lib/ChatWindow.svelte';
	import Mascot from '$lib/Mascot.svelte';
	import Orbit from '$lib/Orbit.svelte';
	import Stars from '$lib/Stars.svelte';
	import Wordmark from '$lib/Wordmark.svelte';

	const GITHUB = 'https://github.com/triangle-int/nolune';
	const NPM = 'https://www.npmjs.com/package/nolune';

	const steps = [
		{
			command: 'npm install -g nolune',
			text: 'Install it on the computer that stays at home. It needs Node.js 22.18 or later.'
		},
		{
			command: 'nolune setup',
			text: 'Connect a model and make an account for everyone in the family.'
		},
		{
			command: 'nolune service install',
			text: 'Keep it running in the background, then open the address from setup and sign in. On Linux, run nolune start.'
		}
	];

	let avatar = $state<Avatar>('planet');
	let hovered = $state<Avatar | null>(null);

	/** The command last copied, while its check mark shows. */
	let copied = $state<string | null>(null);
	let copiedTimer: ReturnType<typeof setTimeout> | undefined;

	async function copy(command: string) {
		try {
			await navigator.clipboard.writeText(command);
		} catch {
			return;
		}
		copied = command;
		clearTimeout(copiedTimer);
		copiedTimer = setTimeout(() => (copied = null), 1600);
	}
</script>

<svelte:head>
	<title>nolune: a little agent that lives on your computer</title>
	<meta
		name="description"
		content="nolune is an open-source AI assistant for your family. It runs on your computer, remembers what matters, runs errands on a schedule, and keeps everyone in one chat."
	/>
	<meta property="og:title" content="nolune: a little agent that lives on your computer" />
	<meta
		property="og:description"
		content="An open-source AI assistant for your family, running on your own computer."
	/>
	<meta property="og:type" content="website" />
	<meta property="og:url" content="https://nolune.dev/" />
	<meta property="og:image" content="https://nolune.dev/og.png" />
	<meta property="og:image:width" content="1200" />
	<meta property="og:image:height" content="630" />
	<meta name="twitter:card" content="summary_large_image" />
</svelte:head>

{#snippet githubMark(size: number)}
	<svg viewBox="0 0 16 16" width={size} height={size} fill="currentColor" aria-hidden="true">
		<path
			d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"
		/>
	</svg>
{/snippet}

{#snippet copyState(command: string)}
	{#if copied === command}
		<CheckIcon size={16} aria-label="Copied" />
	{:else}
		<CopyIcon size={16} aria-label="Copy" />
	{/if}
{/snippet}

<header class="bar">
	<div class="wrap bar-row">
		<a href={resolve('/')} class="home"><Wordmark class="wordmark" /></a>
		<nav>
			<a href="#features">Features</a>
			<a href="#how">How it works</a>
			<a href={GITHUB}>GitHub</a>
		</nav>
		<a href="#how" class="pill">Install</a>
	</div>
</header>

<main>
	<section class="hero">
		<Stars count={70} seed={3} />
		<div class="wrap hero-grid">
			<div>
				<h1>A little agent that lives on your computer</h1>
				<p class="lede">
					It remembers your family, runs errands on your Mac, and keeps everyone in one chat.
				</p>
				<div class="actions">
					<button class="pill install" onclick={() => copy(steps[0].command)}>
						<ChevronRightIcon size={20} />
						<span>{steps[0].command}</span>
						<span class="copy">{@render copyState(steps[0].command)}</span>
					</button>
					<a href={GITHUB} class="github">{@render githubMark(22)}<span>Star on GitHub</span></a>
				</div>
			</div>
			<Orbit />
		</div>
	</section>

	<section class="crew">
		<div class="wrap">
			<h2 class="ruled">Pick who your assistant is</h2>
			<div class="patches" role="radiogroup" aria-label="Assistant look">
				{#each AVATARS as a (a)}
					<button
						class="patch"
						role="radio"
						aria-checked={avatar === a}
						style:--c={AVATAR_COLORS[a].dark}
						onclick={() => (avatar = a)}
						onpointerenter={() => (hovered = a)}
						onpointerleave={() => (hovered = null)}
					>
						<span class="ring">
							<Mascot avatar={a} size={56} mood={hovered === a ? 'working' : 'idle'} />
						</span>
						<span class="name">{a}</span>
					</button>
				{/each}
			</div>
			<p class="caption">Each profile gets its own look and personality. Try one.</p>
		</div>
	</section>

	<section class="product" aria-label="The nolune chat">
		<Stars count={40} seed={11} />
		<div class="wrap product-frame">
			<ChatWindow {avatar} />
		</div>
	</section>

	<section id="features" class="features">
		<div class="wrap columns">
			<article>
				<p class="number">01</p>
				<h3>Remembers the family</h3>
				<svg class="art" viewBox="0 0 320 120" aria-hidden="true">
					<ellipse
						cx="160"
						cy="62"
						rx="146"
						ry="40"
						class="art-line"
						transform="rotate(-6 160 62)"
					/>
					<ellipse
						cx="160"
						cy="62"
						rx="118"
						ry="28"
						class="art-dash"
						transform="rotate(-6 160 62)"
					/>
					{#each [{ x: 64, y: 78, hue: 240, initial: 'A' }, { x: 160, y: 38, hue: 160, initial: 'M' }, { x: 252, y: 58, hue: 20, initial: 'G' }] as person (person.initial)}
						<circle cx={person.x} cy={person.y} r="17" fill="hsl({person.hue} 55% 45%)" />
						<text x={person.x} y={person.y + 5} class="art-initial">{person.initial}</text>
					{/each}
					<path
						d="M292 14Q292 22 300 22Q292 22 292 30Q292 22 284 22Q292 22 292 14Z"
						class="art-spark"
					/>
				</svg>
				<p>
					It knows your people, what they like and what's coming up. You can see and change
					everything it remembers.
				</p>
			</article>
			<article>
				<p class="number">02</p>
				<h3>Runs on a schedule</h3>
				<div class="art schedule">
					<span class="chip">Every weekday at 7:30 <ChevronRightIcon size={18} /></span>
					<svg viewBox="0 0 320 60" aria-hidden="true">
						<path d="M20 52Q160 -8 300 52" class="art-dash" />
						<circle cx="96" cy="24" r="7" class="art-moon" />
						<circle cx="102" cy="21" r="6" class="art-bite" />
						<circle cx="236" cy="22" r="6" class="art-sun" />
					</svg>
				</div>
				<p>
					Ask for a daily check in plain words. It runs in the background and rings the bell when
					there's news.
				</p>
			</article>
			<article>
				<p class="number">03</p>
				<h3>Makes pictures with you</h3>
				<div class="art pictures" style:color={AVATAR_COLORS[avatar].dark}>
					<Mascot {avatar} size={64} />
					<svg viewBox="0 0 90 76" width="90" height="76" aria-hidden="true">
						<rect
							x="6"
							y="8"
							width="76"
							height="60"
							rx="4"
							transform="rotate(-6 44 38)"
							class="art-frame"
						/>
						<path
							d="M18 58L38 36L50 48L58 40L74 56"
							transform="rotate(-6 44 38)"
							class="art-hills"
						/>
						<circle cx="62" cy="24" r="6" class="art-sun" />
					</svg>
				</div>
				<p>
					Invitations, postcards, storybook pages. Start from a template or an idea, then change it
					in the chat.
				</p>
			</article>
		</div>
	</section>

	<section id="how" class="how">
		<div class="wrap">
			<h2 class="ruled">How it works</h2>
			<ol class="steps">
				{#each steps as step, i (step.command)}
					<li>
						<p class="number">{String(i + 1).padStart(2, '0')}</p>
						<button class="command" onclick={() => copy(step.command)}>
							<span class="prompt">$</span>
							<span>{step.command}</span>
							<span class="copy">{@render copyState(step.command)}</span>
						</button>
						<p>{step.text}</p>
					</li>
				{/each}
			</ol>
		</div>
	</section>

	<div class="wrap">
		<ul class="strip">
			<li><LaptopIcon size={22} />Runs on your Mac or Linux</li>
			<li><SparklesIcon size={22} />Claude, GPT or your own models</li>
			<li><CodeXmlIcon size={22} /><a href={GITHUB}>Open source, MIT</a></li>
		</ul>
	</div>
</main>

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
	.wrap {
		width: 100%;
		max-width: 1240px;
		margin: 0 auto;
		padding-inline: 24px;
	}
	section,
	.bar {
		position: relative;
		border-bottom: 1px solid var(--line);
	}
	h1,
	h2,
	h3,
	p {
		margin: 0;
	}
	:global(.wordmark) {
		display: block;
		height: 30px;
		width: auto;
		color: var(--cream);
	}

	/* nav */
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
	.bar nav a,
	footer nav a {
		color: var(--text);
		text-decoration: none;
	}
	.bar nav a:hover,
	footer nav a:hover {
		color: var(--cream);
	}
	.pill {
		display: inline-flex;
		gap: 12px;
		align-items: center;
		padding: 10px 28px;
		border: 1.5px solid var(--rust);
		border-radius: 999px;
		background: none;
		color: var(--rust);
		font: 500 1rem var(--mono);
		text-decoration: none;
		cursor: pointer;
		transition: background 0.2s;
	}
	.pill:hover {
		background: rgb(240 122 60 / 0.1);
	}

	/* hero */
	.hero {
		overflow: hidden;
		background:
			linear-gradient(var(--line) 1px, transparent 1px) 0 0 / 88px 88px,
			linear-gradient(90deg, var(--line) 1px, transparent 1px) 0 0 / 88px 88px;
		background-color: var(--ink);
	}
	.hero::before {
		/* The grid only shows faintly, around the picture. */
		content: '';
		position: absolute;
		inset: 0;
		background: radial-gradient(ellipse 55% 70% at 72% 50%, rgb(8 21 44 / 0.55), var(--ink) 75%);
	}
	.hero-grid {
		position: relative;
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 24px;
		align-items: center;
		padding-top: 72px;
		padding-bottom: 72px;
	}
	h1 {
		max-width: 13ch;
		color: var(--cream);
		font: 900 clamp(2.8rem, 6.2vw, 5.5rem) / 0.95 var(--sans);
		letter-spacing: -0.03em;
	}
	.lede {
		max-width: 44ch;
		margin-top: 30px;
		font-size: 1.15rem;
		line-height: 1.65;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 20px 32px;
		align-items: center;
		margin-top: 40px;
	}
	.install {
		padding: 14px 22px;
		font-size: 1.05rem;
	}
	.copy {
		display: inline-flex;
		color: var(--muted);
	}
	.github {
		display: inline-flex;
		gap: 12px;
		align-items: center;
		color: var(--cream);
		text-underline-offset: 6px;
	}
	.github span {
		text-decoration: underline;
		text-decoration-thickness: 1px;
	}

	/* the assistant's looks */
	.crew {
		padding: 40px 0 44px;
	}
	.ruled {
		display: grid;
		grid-template-columns: 1fr auto 1fr;
		gap: 28px;
		align-items: center;
		color: var(--cream);
		font: 400 1.4rem var(--mono);
	}
	.ruled::before,
	.ruled::after {
		content: '';
		height: 1px;
		background: var(--line);
	}
	.patches {
		display: grid;
		grid-template-columns: repeat(8, 1fr);
		gap: 20px;
		margin-top: 36px;
	}
	.patch {
		display: grid;
		gap: 16px;
		justify-items: center;
		padding: 0;
		border: 0;
		background: none;
		color: var(--c);
		cursor: pointer;
	}
	.ring {
		display: grid;
		place-items: center;
		width: 100%;
		max-width: 116px;
		aspect-ratio: 1;
		border: 2.5px solid var(--c);
		border-radius: 50%;
		background: radial-gradient(
			circle,
			color-mix(in oklab, var(--c) 9%, transparent),
			transparent 70%
		);
		transition:
			box-shadow 0.25s,
			transform 0.25s;
	}
	.ring :global(svg) {
		width: 50%;
		height: auto;
	}
	.patch:hover .ring {
		transform: translateY(-3px);
	}
	.patch[aria-checked='true'] .ring {
		box-shadow:
			0 0 0 5px color-mix(in oklab, var(--c) 20%, transparent),
			0 0 36px color-mix(in oklab, var(--c) 40%, transparent);
	}
	.name {
		font: 500 0.9rem var(--mono);
		letter-spacing: 0.14em;
		text-transform: uppercase;
	}
	.caption {
		margin-top: 28px;
		color: var(--muted);
		font-size: 0.95rem;
		text-align: center;
	}

	/* the app, over the edge of a planet */
	.product {
		overflow: hidden;
		padding: 64px 0 96px;
	}
	.product::before,
	.product::after {
		content: '';
		position: absolute;
		left: 50%;
		top: 46%;
		width: max(1900px, 170vw);
		aspect-ratio: 1;
		border-radius: 50%;
		transform: translateX(-50%);
	}
	.product::before {
		background: #0a1a36;
		box-shadow:
			0 0 0 1.5px rgb(255 150 90 / 0.85),
			0 -8px 50px 6px rgb(240 122 60 / 0.4),
			0 -40px 160px 40px rgb(63 111 184 / 0.18),
			inset 0 24px 90px rgb(240 122 60 / 0.22);
	}
	.product::after {
		/* A halftone rim, fading in from the edge. */
		background: radial-gradient(rgb(151 172 201 / 0.5) 0.9px, transparent 1.4px) 0 0 / 7px 7px;
		mask-image: radial-gradient(closest-side, transparent 90%, #000 99.6%, transparent 100%);
	}
	.product-frame {
		position: relative;
		z-index: 1;
		max-width: 1100px;
	}

	/* features */
	.columns {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		padding-top: 64px;
		padding-bottom: 64px;
	}
	.columns article {
		display: flex;
		flex-direction: column;
		padding: 4px 36px;
	}
	.columns article + article {
		border-left: 1px solid var(--line);
	}
	.columns article:first-child {
		padding-left: 0;
	}
	.columns article:last-child {
		padding-right: 0;
	}
	.number {
		color: #b8c3d8;
		font: 800 2.2rem var(--sans);
		letter-spacing: 0.02em;
	}
	h3 {
		margin-top: 4px;
		color: var(--cream);
		font: 800 clamp(1.5rem, 2.3vw, 1.95rem) / 1.15 var(--sans);
		letter-spacing: -0.01em;
	}
	.columns p:last-child {
		margin-top: auto;
		font-size: 0.95rem;
		line-height: 1.65;
	}
	.art {
		display: block;
		width: 100%;
		height: 132px;
		margin: 22px 0 24px;
	}
	.art-line,
	.art-dash {
		fill: none;
		stroke: var(--cream);
		stroke-width: 1.3;
	}
	.art-line {
		opacity: 0.7;
	}
	.art-dash {
		stroke-dasharray: 3 7;
		stroke-linecap: round;
		opacity: 0.6;
	}
	.art-initial {
		fill: #fff;
		font: 600 15px var(--sans);
		text-anchor: middle;
	}
	.art-spark {
		fill: var(--cream);
	}
	.art-moon {
		fill: var(--cream);
	}
	.art-bite {
		fill: var(--ink);
	}
	.art-sun {
		fill: var(--rust);
	}
	.schedule {
		display: flex;
		flex-direction: column;
		justify-content: center;
		gap: 10px;
	}
	.schedule svg {
		width: 100%;
		height: 60px;
	}
	.chip {
		display: inline-flex;
		align-self: flex-start;
		gap: 8px;
		align-items: center;
		padding: 10px 22px;
		border: 1.5px solid var(--rust);
		border-radius: 12px;
		color: var(--rust);
		font-size: 1.05rem;
	}
	.pictures {
		display: flex;
		gap: 20px;
		align-items: center;
		justify-content: center;
	}
	.art-frame,
	.art-hills {
		fill: none;
		stroke: var(--cream);
		stroke-width: 2.4;
		stroke-linejoin: round;
	}

	/* how it works */
	.how {
		padding: 48px 0 64px;
	}
	.steps {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 36px;
		margin: 40px 0 0;
		padding: 0;
		list-style: none;
	}
	.steps li {
		display: flex;
		flex-direction: column;
		gap: 16px;
	}
	.steps .number {
		color: var(--rust);
		font-size: 1.6rem;
	}
	.command {
		display: flex;
		gap: 12px;
		align-items: center;
		width: 100%;
		padding: 14px 18px;
		border: 1px solid var(--line);
		border-radius: 12px;
		background: var(--ink-raised);
		color: var(--cream);
		font: 500 1rem var(--mono);
		text-align: left;
		cursor: pointer;
		transition: border-color 0.2s;
	}
	.command:hover {
		border-color: var(--rust);
	}
	.command .copy {
		margin-left: auto;
	}
	.prompt {
		color: var(--rust);
	}
	.steps p:last-child {
		line-height: 1.65;
		font-size: 0.95rem;
	}

	/* strip and footer */
	.strip {
		display: flex;
		flex-wrap: wrap;
		gap: 12px 44px;
		justify-content: center;
		margin: 40px 0;
		padding: 18px 32px;
		border: 1.5px solid var(--line);
		border-radius: 999px;
		list-style: none;
		font-size: 1rem;
	}
	.strip li {
		display: flex;
		gap: 14px;
		align-items: center;
	}
	.strip li + li::before {
		content: '';
		width: 5px;
		height: 5px;
		margin-right: 30px;
		border-radius: 50%;
		background: var(--text);
	}
	.strip a {
		text-decoration: none;
	}
	.strip a:hover {
		text-decoration: underline;
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

	@media (max-width: 960px) {
		.bar nav {
			display: none;
		}
		.bar-row .pill {
			margin-left: auto;
		}
		.hero-grid {
			grid-template-columns: 1fr;
			padding-top: 48px;
			padding-bottom: 40px;
		}
		.hero-grid :global(.orbit) {
			max-width: 560px;
			justify-self: center;
		}
		.patches {
			grid-template-columns: repeat(4, 1fr);
			row-gap: 28px;
		}
		.columns,
		.steps {
			grid-template-columns: 1fr;
		}
		.columns article,
		.columns article:first-child,
		.columns article:last-child {
			padding: 36px 0;
		}
		.columns article + article {
			border-top: 1px solid var(--line);
			border-left: 0;
		}
		.columns {
			padding-top: 16px;
			padding-bottom: 16px;
		}
		.art {
			max-width: 420px;
		}
		.footer-row {
			flex-wrap: wrap;
			gap: 20px 28px;
		}
		footer nav {
			margin-left: 0;
		}
	}

	@media (max-width: 560px) {
		.wrap {
			padding-inline: 16px;
		}
		.ruled {
			grid-template-columns: 1fr;
			text-align: center;
			font-size: 1.2rem;
		}
		.ruled::before,
		.ruled::after {
			display: none;
		}
		.patches {
			gap: 14px;
		}
		.name {
			font-size: 0.7rem;
			letter-spacing: 0.08em;
		}
		.install {
			font-size: 0.95rem;
		}
		.strip {
			flex-direction: column;
			align-items: flex-start;
			border-radius: 20px;
		}
		.strip li + li::before {
			display: none;
		}
		.product {
			padding: 40px 0 64px;
		}
	}
</style>
