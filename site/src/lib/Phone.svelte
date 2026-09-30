<script lang="ts" module>
	export type Person = 'polina' | 'timur' | 'grandma';

	export const NAMES: Record<Person, string> = {
		polina: 'Polina',
		timur: 'Timur',
		grandma: 'Grandma'
	};

	export interface Message {
		id: number;
		from: Person | 'nolune';
		text: string;
		/** A reply's folded "Worked for" line. */
		worked?: string;
	}

	/** The app's color for a person (packages/web/src/lib/components/UserAvatar.svelte). */
	export function hue(name: string): number {
		return [...name].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);
	}

	/** "Timur", "Timur and Grandma", as the app's typing line lists people. */
	export function list(names: string[]): string {
		return names.length < 2
			? names.join('')
			: `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`;
	}
</script>

<script lang="ts">
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import type { Avatar } from '@nolune/core/avatars';
	import { AVATAR_COLORS } from './avatars';
	import Mascot from './Mascot.svelte';

	// One person's phone in the family demo: the chat as the app shows it to them. Their own
	// messages have no name over them, everyone else's do, and while others write, they see who.

	interface Props {
		me: Person;
		avatar: Avatar;
		messages: Message[];
		drafts: Record<Person, string>;
		/** Who's typing, in the order they started. */
		typing: Person[];
		/** nolune is working on a reply. */
		working: boolean;
	}

	let { me, avatar, messages, drafts, typing, working }: Props = $props();

	const others = $derived(typing.filter((person) => person !== me));
	const draft = $derived(drafts[me]);
</script>

{#snippet initial(person: Person)}
	<span class="initial" style:background-color="hsl({hue(NAMES[person])} 55% 45%)">
		{NAMES[person][0]}
	</span>
{/snippet}

<figure class="phone" style:--tone={AVATAR_COLORS[avatar].light}>
	<figcaption>{@render initial(me)}{NAMES[me]}'s phone</figcaption>
	<div class="screen">
		<div class="status" aria-hidden="true"><span>9:41</span><span class="bars"></span></div>
		<header>
			<ChevronLeftIcon size={18} />
			<div>
				<b>Saturday picnic</b>
				<span><Mascot {avatar} size={11} class="tone" />Family</span>
			</div>
		</header>

		<div class="thread">
			{#each messages as message (message.id)}
				{#if message.from === 'nolune'}
					<div class="reply">
						<Mascot {avatar} size={16} class="tone" />
						<div>
							{#if message.worked}
								<p class="worked">{message.worked} <ChevronRightIcon size={11} /></p>
							{/if}
							<p>{message.text}</p>
						</div>
					</div>
				{:else}
					<div class="human">
						{#if message.from !== me}
							<p class="who">{@render initial(message.from)}{NAMES[message.from]}</p>
						{/if}
						<p class="bubble">{message.text}</p>
					</div>
				{/if}
			{/each}
			{#if working}
				<div class="reply">
					<Mascot {avatar} size={16} mood="working" class="tone" />
					<p class="thinking">Thinking…</p>
				</div>
			{/if}
			{#if others.length}
				<div class="human" role="status">
					<p class="who">
						<span class="stack">
							{#each others as person (person)}{@render initial(person)}{/each}
						</span>
						{list(others.map((person) => NAMES[person]))}
						{others.length === 1 ? 'is' : 'are'} typing
					</p>
					<p class="bubble dots" aria-hidden="true"><span></span><span></span><span></span></p>
				</div>
			{/if}
		</div>

		<div class="composer" class:filled={draft}>
			{#if draft}
				<span class="draft">{draft}<span class="caret"></span></span>
			{:else}
				<span class="placeholder">Ask nolune</span>
			{/if}
			<span class="send"><ArrowUpIcon size={14} /></span>
		</div>
	</div>
</figure>

<style>
	.phone {
		display: grid;
		gap: 14px;
		justify-items: center;
		margin: 0;
	}
	figcaption {
		display: flex;
		gap: 8px;
		align-items: center;
		color: var(--text);
		font: 500 0.85rem var(--mono);
	}
	.screen {
		--fg: #0d0d0d;
		--quiet: #5d5d5d;
		--bubble: color-mix(in oklab, #f4f4f4 94%, var(--tone));
		display: flex;
		flex-direction: column;
		width: 264px;
		height: 540px;
		overflow: hidden;
		border: 7px solid #1b2230;
		border-radius: 38px;
		background: #fff;
		color: var(--fg);
		font: 400 13px/1.45 var(--sans);
		text-align: left;
		box-shadow:
			0 0 0 1px rgb(255 255 255 / 0.18),
			0 24px 60px -18px rgb(0 0 0 / 0.75);
	}
	p {
		margin: 0;
	}
	.phone :global(.tone) {
		color: var(--tone);
	}
	.status {
		display: flex;
		justify-content: space-between;
		padding: 8px 20px 2px;
		font-size: 11px;
		font-weight: 600;
	}
	.bars {
		width: 26px;
		height: 10px;
		border-radius: 3px;
		background: linear-gradient(90deg, var(--fg) 70%, #c9c9c9 70%);
	}
	header {
		display: flex;
		gap: 6px;
		align-items: center;
		padding: 6px 12px 8px;
		border-bottom: 1px solid rgb(13 13 13 / 0.06);
		color: var(--quiet);
	}
	header div {
		display: grid;
		line-height: 1.25;
	}
	header b {
		color: var(--fg);
		font-weight: 600;
	}
	header span {
		display: flex;
		gap: 4px;
		align-items: center;
		font-size: 11px;
	}

	/* Newest at the bottom; older messages go up under a fade. */
	.thread {
		display: flex;
		flex: 1;
		flex-direction: column;
		justify-content: flex-end;
		gap: 10px;
		min-height: 0;
		overflow: hidden;
		padding: 10px 12px;
		mask-image: linear-gradient(transparent, #000 44px);
	}
	.thread > * {
		flex-shrink: 0;
		animation: rise 0.28s ease-out both;
	}
	.human {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 3px;
	}
	.who {
		display: flex;
		gap: 5px;
		align-items: center;
		padding: 0 2px;
		color: var(--quiet);
		font-size: 10.5px;
	}
	.bubble {
		max-width: 86%;
		padding: 7px 12px;
		border-radius: 17px;
		background: var(--bubble);
	}
	.reply {
		display: flex;
		gap: 8px;
		align-items: flex-start;
	}
	.reply :global(svg) {
		margin-top: 1px;
	}
	.worked {
		display: flex;
		align-items: center;
		color: var(--quiet);
		font-size: 11px;
	}
	.thinking {
		background: linear-gradient(90deg, var(--quiet) 40%, #d0d0d0 50%, var(--quiet) 60%) 0 0 / 300%
			100%;
		background-clip: text;
		color: transparent;
		animation: shimmer 1.6s linear infinite;
	}
	.initial {
		display: inline-grid;
		place-items: center;
		width: 14px;
		height: 14px;
		border-radius: 50%;
		color: #fff;
		font: 600 8px var(--sans);
	}
	figcaption .initial {
		width: 20px;
		height: 20px;
		font-size: 11px;
	}
	.stack {
		display: flex;
	}
	.stack .initial + .initial {
		margin-left: -4px;
		box-shadow: 0 0 0 1.5px #fff;
	}
	.dots {
		display: flex;
		gap: 3px;
		align-items: center;
		height: 30px;
	}
	.dots span {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--quiet);
		animation: bounce 1.2s ease-in-out infinite;
	}
	.dots span:nth-child(2) {
		animation-delay: 0.15s;
	}
	.dots span:nth-child(3) {
		animation-delay: 0.3s;
	}
	.composer {
		display: flex;
		gap: 8px;
		align-items: center;
		margin: 0 10px 12px;
		padding: 8px 8px 8px 14px;
		border: 1px solid rgb(13 13 13 / 0.12);
		border-radius: 20px;
	}
	.placeholder {
		flex: 1;
		color: #8f8f8f;
	}
	.draft {
		flex: 1;
		min-width: 0;
	}
	.caret {
		display: inline-block;
		width: 1.5px;
		height: 14px;
		margin-left: 1px;
		vertical-align: -2px;
		background: var(--fg);
		animation: blink 1s steps(1) infinite;
	}
	.send {
		display: grid;
		place-items: center;
		flex-shrink: 0;
		width: 26px;
		height: 26px;
		border-radius: 50%;
		background: #e5e5e5;
		color: #fff;
		transition: background 0.2s;
	}
	.filled .send {
		background: var(--fg);
	}

	@keyframes rise {
		from {
			opacity: 0;
			transform: translateY(8px);
		}
	}
	@keyframes bounce {
		0%,
		60%,
		100% {
			transform: translateY(0);
			opacity: 0.45;
		}
		30% {
			transform: translateY(-3px);
			opacity: 1;
		}
	}
	@keyframes shimmer {
		from {
			background-position: 100% 0;
		}
		to {
			background-position: 0% 0;
		}
	}
	@keyframes blink {
		50% {
			opacity: 0;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.thread > *,
		.dots span,
		.thinking,
		.caret {
			animation: none;
		}
	}
</style>
