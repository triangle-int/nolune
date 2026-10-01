<script lang="ts">
	import { onMount, untrack, type Snippet } from 'svelte';
	import { enhance } from '$app/forms';
	import { page } from '$app/state';
	import type { CommandMode } from '@nolune/core';
	import type { Avatar } from '@nolune/core/avatars';
	import type { FolderItem } from '$lib/folders';
	import { getI18n } from '$lib/i18n';
	import { Attachments } from '$lib/uploads.svelte';
	import CommandModeMenu from './CommandModeMenu.svelte';
	import Composer from './Composer.svelte';
	import FolderMenu from './FolderMenu.svelte';
	import ModelMenu from './ModelMenu.svelte';

	interface Props {
		/** The profile the chat is started in. */
		slug: string;
		presets: { id: string; name: string; provider?: string }[];
		defaultPresetId: string;
		efforts: string[];
		folders: FolderItem[];
		/** The folder the chat starts in, until someone picks another. */
		folderId: string | null;
		/** The profile's assistant, for the reasoning slider. */
		avatar: Avatar;
		/** How commands run in Models & keys, which a new chat starts with. */
		commandFallback: CommandMode;
		placeholder?: string;
		autofocus?: boolean;
		class?: string;
		/** Before the composer's column, like a greeting. */
		header?: Snippet;
		/** In the composer's column, above and below it. `suggest` puts text in the box. */
		above?: Snippet<[suggest: (text: string) => void]>;
		below?: Snippet<[suggest: (text: string) => void]>;
		/** After the composer's column. */
		footer?: Snippet;
	}

	const { m } = getI18n();

	let {
		slug,
		presets,
		defaultPresetId,
		efforts,
		folders,
		folderId: initialFolderId,
		avatar,
		commandFallback,
		placeholder = m.chat.placeholder,
		autofocus = false,
		class: className,
		header,
		above,
		below,
		footer
	}: Props = $props();

	/** The reasoning picked last time on this device. The model always starts at the default. */
	const STORAGE_KEY = 'nolune-new-chat';

	let text = $state('');
	const attachments = new Attachments(() => slug, m);
	let presetId = $state(untrack(() => defaultPresetId));
	let effort = $state('medium');
	/** Never remembered: every new chat starts as Models & keys says. */
	let commands = $state<CommandMode>(untrack(() => commandFallback));
	// Follows the page (`?folder=`) until someone picks another folder in the chip.
	let folderId = $derived(initialFolderId);
	let submitting = $state(false);
	let problem = $state<string | null>(null);
	let textarea = $state<HTMLTextAreaElement | null>(null);
	let formEl = $state<HTMLFormElement>();

	onMount(() => {
		try {
			const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
			if (efforts.includes(saved.effort)) effort = saved.effort;
		} catch {
			// Nothing saved, or storage is blocked.
		}
	});

	function remember() {
		try {
			localStorage.setItem(STORAGE_KEY, JSON.stringify({ effort }));
		} catch {
			// Storage is blocked; the defaults are fine.
		}
	}

	function suggest(value: string) {
		text = value;
		textarea?.focus();
		textarea?.setSelectionRange(value.length, value.length);
	}
</script>

<form
	bind:this={formEl}
	method="POST"
	action="/p/{slug}"
	class={className}
	use:enhance={() => {
		submitting = true;
		problem = null;
		remember();
		return async ({ result, update }) => {
			if (result.type === 'failure') {
				problem = String(result.data?.message ?? m.newChat.couldNotStart);
			} else await update();
			submitting = false;
		};
	}}
>
	<input type="hidden" name="preset" value={presetId} />
	<input type="hidden" name="effort" value={effort} />
	<input type="hidden" name="folder" value={folderId ?? ''} />
	<input type="hidden" name="commands" value={commands} />
	{#each attachments.ids as id (id)}
		<input type="hidden" name="upload" value={id} />
	{/each}

	{@render header?.()}

	<div class="mx-auto w-full max-w-3xl">
		{@render above?.(suggest)}

		<Composer
			bind:value={text}
			bind:textarea
			name="text"
			{placeholder}
			{attachments}
			busy={submitting}
			{autofocus}
			onsubmit={() => formEl?.requestSubmit()}
		>
			{#snippet tools()}
				<ModelMenu
					{efforts}
					{effort}
					onEffortChange={(value) => (effort = value)}
					{presets}
					{presetId}
					{defaultPresetId}
					onPresetChange={(id) => (presetId = id)}
					{avatar}
				/>
				<FolderMenu {folders} {folderId} {slug} onchange={(id) => (folderId = id)} />
				<CommandModeMenu
					mode={commands}
					fallback={commandFallback}
					canUnrestrict={page.data.user?.isAdmin === true}
					onchange={(mode) => (commands = mode)}
				/>
			{/snippet}
		</Composer>

		{@render below?.(suggest)}

		{#if problem}
			<p class="mt-2 text-center text-sm text-destructive">{problem}</p>
		{/if}
	</div>

	{@render footer?.()}
</form>
