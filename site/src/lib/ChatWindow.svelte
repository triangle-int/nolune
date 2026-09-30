<script lang="ts">
	import SquarePenIcon from '@lucide/svelte/icons/square-pen';
	import SearchIcon from '@lucide/svelte/icons/search';
	import ImagesIcon from '@lucide/svelte/icons/images';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import BrainIcon from '@lucide/svelte/icons/brain';
	import PuzzleIcon from '@lucide/svelte/icons/puzzle';
	import UsersIcon from '@lucide/svelte/icons/users';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import PanelLeftIcon from '@lucide/svelte/icons/panel-left';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
	import BellIcon from '@lucide/svelte/icons/bell';
	import PaperclipIcon from '@lucide/svelte/icons/paperclip';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import type { Avatar } from '@nolune/core/avatars';
	import { AVATAR_COLORS } from './avatars';
	import Mascot from './Mascot.svelte';

	// The app in its light theme, drawn in HTML after the real screens (the sidebar and chat in
	// packages/web/src/lib/components), with a made-up family.

	interface Props {
		avatar: Avatar;
	}

	let { avatar }: Props = $props();

	const nav = [
		{ icon: SquarePenIcon, label: 'New chat' },
		{ icon: SearchIcon, label: 'Search chats' },
		{ icon: ImagesIcon, label: 'Images' },
		{ icon: ClockIcon, label: 'Automations' },
		{ icon: BrainIcon, label: 'Memory' },
		{ icon: PuzzleIcon, label: 'Skills' },
		{ icon: UsersIcon, label: 'People & profile' }
	];

	const chats = [
		'Cleaning up Downloads',
		'Umbrella check for London',
		"Grandma's birthday card",
		'Meal plan for the week'
	];
</script>

<div class="window" style:--tone={AVATAR_COLORS[avatar].light}>
	<div class="chrome" aria-hidden="true"><span></span><span></span><span></span></div>
	<div class="app">
		<aside class="sidebar">
			<div class="profile">
				{#key avatar}
					<Mascot {avatar} size={22} mood="done" class="tone" />
				{/key}
				<b>Family</b>
				<ChevronDownIcon size={16} />
				<span class="grow"></span>
				<PanelLeftIcon size={18} class="quiet" />
			</div>
			<ul class="nav">
				{#each nav as item (item.label)}
					<li><item.icon size={16} />{item.label}</li>
				{/each}
			</ul>
			<p class="group">Chats</p>
			<ul class="chats">
				{#each chats as chat, i (chat)}
					<li class:active={i === 0}>{chat}</li>
				{/each}
			</ul>
			<div class="me"><span class="initial">A</span>Anna</div>
		</aside>

		<div class="main">
			<header>
				<b>Cleaning up Downloads</b>
				<span class="grow"></span>
				<EllipsisIcon size={18} class="quiet" />
				<BellIcon size={18} class="quiet" />
			</header>
			<div class="thread">
				<p class="bubble">What's taking up space in my Downloads?</p>
				<div class="reply">
					{#key avatar}
						<Mascot {avatar} size={24} mood="done" class="tone" />
					{/key}
					<div class="text">
						<p>I'll look at the Downloads folder to see what's using the space.</p>
						<p class="worked">Worked for 12s <ChevronRightIcon size={14} /></p>
						<p>Your Downloads folder is 404 MB in total. <b>What's taking the space:</b></p>
						<ul>
							<li><code>Lake trip.mov</code> is 172 MB, the largest file by far.</li>
							<li>
								<code>zoom-installer.pkg</code> is 92 MB. It's installed already, so this copy can go.
							</li>
							<li><code>School photo day.zip</code> is 39 MB.</li>
						</ul>
						<p>I haven't deleted anything. Do you want me to remove the installer?</p>
					</div>
				</div>
			</div>
			<div class="composer">
				<p class="placeholder">Ask nolune</p>
				<div class="tools">
					<PaperclipIcon size={18} class="quiet" />
					<span class="effort">Medium <ChevronDownIcon size={14} /></span>
					<span class="grow"></span>
					<span class="send"><ArrowUpIcon size={18} /></span>
				</div>
			</div>
			<p class="note">nolune can make mistakes, and it can change files on this computer.</p>
		</div>
	</div>
</div>

<style>
	.window {
		--fg: #0d0d0d;
		--quiet: #5d5d5d;
		--sidebar: color-mix(in oklab, #f9f9f9 94%, var(--tone));
		--accent: color-mix(in oklab, #ececec 90%, var(--tone));
		--bubble: color-mix(in oklab, #f4f4f4 94%, var(--tone));
		container-type: inline-size;
		overflow: hidden;
		border: 1px solid rgb(255 255 255 / 0.35);
		border-radius: 14px;
		background: #fff;
		color: var(--fg);
		font: 400 15px/1.6 var(--sans);
		text-align: left;
		box-shadow:
			0 0 0 6px rgb(151 172 201 / 0.12),
			0 30px 80px -20px rgb(0 0 0 / 0.7);
	}
	.chrome {
		display: flex;
		gap: 7px;
		padding: 11px 14px;
		background: var(--sidebar);
		border-bottom: 1px solid rgb(13 13 13 / 0.06);
	}
	.chrome span {
		width: 11px;
		height: 11px;
		border-radius: 50%;
		background: #ff5f57;
	}
	.chrome span:nth-child(2) {
		background: #febc2e;
	}
	.chrome span:nth-child(3) {
		background: #28c840;
	}
	.app {
		display: grid;
		grid-template-columns: 236px 1fr;
		min-height: 540px;
	}
	ul {
		margin: 0;
		padding: 0;
		list-style: none;
	}
	p {
		margin: 0;
	}
	.grow {
		flex: 1;
	}
	.window :global(.quiet) {
		color: var(--quiet);
	}
	.window :global(.tone) {
		color: var(--tone);
	}

	.sidebar {
		display: flex;
		flex-direction: column;
		padding: 10px 8px;
		background: var(--sidebar);
		font-size: 14px;
	}
	.profile {
		display: flex;
		gap: 6px;
		align-items: center;
		padding: 4px 8px 12px;
		font-size: 17px;
	}
	.nav li,
	.chats li {
		display: flex;
		gap: 10px;
		align-items: center;
		padding: 6px 10px;
		border-radius: 10px;
	}
	.group {
		margin: 18px 10px 6px;
		color: var(--quiet);
	}
	.chats li {
		overflow: hidden;
		white-space: nowrap;
		text-overflow: ellipsis;
		display: block;
	}
	.chats .active {
		background: var(--accent);
	}
	.me {
		display: flex;
		gap: 10px;
		align-items: center;
		margin-top: auto;
		padding: 8px 10px 2px;
	}
	.initial {
		display: grid;
		place-items: center;
		width: 26px;
		height: 26px;
		border-radius: 50%;
		background: hsl(240 55% 45%);
		color: #fff;
		font-size: 12px;
		font-weight: 600;
	}

	.main {
		display: flex;
		flex-direction: column;
		min-width: 0;
		padding: 0 28px 10px;
	}
	header {
		display: flex;
		gap: 16px;
		align-items: center;
		padding: 12px 0;
		font-size: 16px;
	}
	header b {
		font-weight: 500;
	}
	.thread {
		display: flex;
		flex-direction: column;
		gap: 22px;
		max-width: 640px;
		width: 100%;
		margin: 6px auto 20px;
	}
	.bubble {
		align-self: flex-end;
		max-width: 80%;
		padding: 8px 16px;
		border-radius: 18px;
		background: var(--bubble);
	}
	.reply {
		display: flex;
		gap: 14px;
		align-items: flex-start;
	}
	.reply :global(svg) {
		margin-top: 2px;
	}
	.text {
		display: flex;
		flex-direction: column;
		gap: 10px;
		min-width: 0;
	}
	.text ul {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding-left: 20px;
		list-style: disc;
	}
	.worked {
		display: flex;
		gap: 2px;
		align-items: center;
		margin-top: -4px;
		color: var(--quiet);
		font-size: 14px;
	}
	code {
		padding: 1px 5px;
		border-radius: 6px;
		background: #f4f4f4;
		font: 500 13.5px var(--mono);
	}
	.composer {
		max-width: 640px;
		width: 100%;
		margin: auto auto 0;
		padding: 14px 16px 10px;
		border: 1px solid rgb(13 13 13 / 0.1);
		border-radius: 24px;
		box-shadow: 0 4px 16px rgb(0 0 0 / 0.05);
	}
	.placeholder {
		color: #8f8f8f;
	}
	.tools {
		display: flex;
		gap: 16px;
		align-items: center;
		margin-top: 12px;
		color: var(--quiet);
		font-size: 14px;
	}
	.effort {
		display: flex;
		gap: 3px;
		align-items: center;
	}
	.send {
		display: grid;
		place-items: center;
		width: 34px;
		height: 34px;
		border-radius: 50%;
		background: #e5e5e5;
		color: #fff;
	}
	.note {
		margin-top: 8px;
		color: var(--quiet);
		font-size: 12px;
		text-align: center;
	}

	@container (width < 720px) {
		.app {
			grid-template-columns: 1fr;
			min-height: 0;
		}
		.sidebar {
			display: none;
		}
		.main {
			padding: 0 16px 10px;
			font-size: 14px;
		}
	}
</style>
