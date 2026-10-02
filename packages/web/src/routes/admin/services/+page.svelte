<script lang="ts">
	import TopBar from '$lib/components/TopBar.svelte';
	import McpServers from '$lib/components/admin/McpServers.svelte';
	import { getI18n } from '$lib/i18n';

	let { data, form } = $props();
	const { m } = getI18n();
</script>

<div class="flex h-full flex-col">
	<TopBar />
	<main class="min-h-0 flex-1 overflow-y-auto">
		<div class="mx-auto max-w-2xl space-y-8 px-4 py-8 sm:py-12">
			<div class="space-y-1">
				<h1 class="text-2xl font-semibold">{m.services.title}</h1>
				<p class="text-muted-foreground">{m.services.hint}</p>
			</div>

			<McpServers servers={data.servers} profiles={data.profiles} result={form} />
		</div>
	</main>
</div>
