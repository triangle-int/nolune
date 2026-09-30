<script lang="ts">
	import { untrack } from 'svelte';
	import * as Sidebar from '$lib/components/ui/sidebar';
	import AppSidebar from '$lib/components/AppSidebar.svelte';

	let { data, children } = $props();

	let sidebarOpen = $state(untrack(() => data.sidebarOpen));
</script>

<Sidebar.Provider bind:open={sidebarOpen} class="h-dvh min-h-0">
	{#if data.user}
		<AppSidebar
			profile={data.profile}
			profiles={data.profiles}
			folders={data.folders}
			conversations={data.conversations}
			user={data.user}
		/>
	{/if}
	<main class="flex h-dvh min-w-0 flex-1 flex-col bg-background">
		{@render children()}
	</main>
</Sidebar.Provider>
