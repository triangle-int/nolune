<script lang="ts">
	import BellIcon from '@lucide/svelte/icons/bell';
	import CloudSunIcon from '@lucide/svelte/icons/cloud-sun';
	import FileSearchIcon from '@lucide/svelte/icons/file-search';
	import HardDriveIcon from '@lucide/svelte/icons/hard-drive';
	import AssistantAvatar from '$lib/components/AssistantAvatar.svelte';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import NewChatForm from '$lib/components/chat/NewChatForm.svelte';

	let { data } = $props();

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

	const firstName = $derived(data.user?.name.split(/\s+/)[0] ?? '');
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
	<NewChatForm
		slug={data.profile.slug}
		presets={data.presets}
		defaultPresetId={data.defaultPresetId}
		efforts={data.efforts}
		folders={data.folders}
		folderId={data.folderId}
		autofocus
		class="flex min-h-0 flex-1 flex-col overflow-y-auto px-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] sm:px-4"
	>
		{#snippet header()}
			<!-- Phones: greeting in the middle, composer at the bottom. Desktop: both centered. -->
			<div class="flex-1"></div>
			<AssistantAvatar
				avatar={data.profile.avatar}
				mood="idle"
				size={64}
				class="mx-auto mb-5 block"
			/>
			<h1 class="mb-8 text-center text-[28px] leading-tight font-normal tracking-tight">
				{firstName ? `What can I help with, ${firstName}?` : 'What can I help with?'}
			</h1>
			<div class="flex-1 sm:hidden"></div>
		{/snippet}

		{#snippet above(suggest)}
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
		{/snippet}

		{#snippet below(suggest)}
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
		{/snippet}

		{#snippet footer()}
			<div class="hidden flex-[1.3] sm:block"></div>
		{/snippet}
	</NewChatForm>
{/if}
