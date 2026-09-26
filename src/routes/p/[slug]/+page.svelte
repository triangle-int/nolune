<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { enhance } from '$app/forms';
	import BellIcon from '@lucide/svelte/icons/bell';
	import CloudSunIcon from '@lucide/svelte/icons/cloud-sun';
	import FileSearchIcon from '@lucide/svelte/icons/file-search';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import Composer from '$lib/components/chat/Composer.svelte';
	import ModelMenu from '$lib/components/chat/ModelMenu.svelte';

	let { data, form } = $props();

	const SUGGESTIONS = [
		{ icon: BellIcon, label: 'Set a reminder', text: 'Remind me tomorrow at 9:00 to ' },
		{
			icon: CloudSunIcon,
			label: 'Daily weather check',
			text: 'Every weekday at 7:30, check the weather and tell us if we need umbrellas.'
		},
		{ icon: FileSearchIcon, label: 'Find a file', text: 'Find the file on this computer called ' },
		{
			icon: HardDriveIcon,
			label: 'Check free space',
			text: 'How much free disk space is left on this computer?'
		}
	];

	/** The reasoning picked last time on this device. The model always starts at the default. */
	const STORAGE_KEY = 'btw-new-chat';

	let text = $state('');
	let presetId = $state(untrack(() => data.defaultPresetId));
	let effort = $state('medium');
	let submitting = $state(false);
	let textarea = $state<HTMLTextAreaElement | null>(null);
	let formEl = $state<HTMLFormElement>();

	const firstName = $derived(data.user?.name.split(/\s+/)[0] ?? '');

	onMount(() => {
		try {
			const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
			if (data.efforts.includes(saved.effort)) effort = saved.effort;
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

<PageHeader>
	<span class="truncate text-lg font-medium">{data.profile.name}</span>
</PageHeader>

{#if data.presets.length === 0}
	<div class="flex flex-1 items-center justify-center p-6">
		<p class="max-w-md text-center text-muted-foreground">
			No models are set up yet. An admin can add one on the Models page or with
			<code class="rounded bg-muted px-1">btw preset add &lt;model&gt;</code>.
		</p>
	</div>
{:else}
	<form
		bind:this={formEl}
		method="POST"
		class="flex min-h-0 flex-1 flex-col overflow-y-auto px-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] sm:px-4"
		use:enhance={() => {
			submitting = true;
			remember();
			return async ({ update }) => {
				await update();
				submitting = false;
			};
		}}
	>
		<input type="hidden" name="preset" value={presetId} />
		<input type="hidden" name="effort" value={effort} />

		<!-- Phones: greeting in the middle, composer at the bottom. Desktop: both centered. -->
		<div class="flex-1"></div>
		<h1 class="mb-8 text-center text-[28px] leading-tight font-normal tracking-tight">
			{firstName ? `What can I help with, ${firstName}?` : 'What can I help with?'}
		</h1>
		<div class="flex-1 sm:hidden"></div>

		<div class="mx-auto w-full max-w-3xl">
			<div class="-mx-3 mb-3 no-scrollbar flex gap-2 overflow-x-auto px-3 sm:hidden">
				{#each SUGGESTIONS as s (s.label)}
					<button
						type="button"
						onclick={() => suggest(s.text)}
						class="flex shrink-0 items-center gap-2 rounded-full border px-3.5 py-2 text-sm hover:bg-muted"
					>
						<s.icon class="size-4 text-muted-foreground" />
						{s.label}
					</button>
				{/each}
			</div>

			<Composer
				bind:value={text}
				bind:textarea
				name="text"
				placeholder="Ask btw"
				busy={submitting}
				autofocus
				onsubmit={() => formEl?.requestSubmit()}
			>
				{#snippet tools()}
					<ModelMenu
						efforts={data.efforts}
						{effort}
						onEffortChange={(value) => (effort = value)}
						presets={data.presets}
						{presetId}
						defaultPresetId={data.defaultPresetId}
						onPresetChange={(id) => (presetId = id)}
					/>
				{/snippet}
			</Composer>

			<div class="mt-4 hidden flex-wrap justify-center gap-2 sm:flex">
				{#each SUGGESTIONS as s (s.label)}
					<button
						type="button"
						onclick={() => suggest(s.text)}
						class="flex items-center gap-2 rounded-full border px-3.5 py-2 text-sm text-muted-foreground hover:bg-muted hover:text-foreground"
					>
						<s.icon class="size-4" />
						{s.label}
					</button>
				{/each}
			</div>

			{#if form?.message}
				<p class="mt-2 text-center text-sm text-destructive">{form.message}</p>
			{/if}
		</div>
		<div class="hidden flex-[1.3] sm:block"></div>
	</form>
{/if}
