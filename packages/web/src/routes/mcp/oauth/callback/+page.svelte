<script lang="ts">
	import { resolve } from '$app/paths';
	import { Button } from '$lib/components/ui/button';
	import { getI18n } from '$lib/i18n';

	/** How signing in to an MCP server went, for someone who isn't an admin signed in here. */
	let { data } = $props();
	const { m } = getI18n();
	const t = $derived(m.mcpSignIn);
</script>

<main class="flex min-h-full flex-col items-center justify-center p-4">
	<div class="w-full max-w-sm space-y-6 py-8 text-center">
		<div class="space-y-2">
			<div class="text-4xl font-semibold tracking-tight">nolune</div>
			{#if data.name}
				<h1 class="text-2xl font-medium">{t.doneTitle(data.name)}</h1>
				<p class="text-sm text-muted-foreground">
					{data.tools === null ? t.doneNoTools : t.done(data.tools)}
				</p>
			{:else}
				<h1 class="text-2xl font-medium">{t.failedTitle}</h1>
				<p class="text-sm break-words text-muted-foreground">
					{data.denied ? t.denied : ''}
					{data.problem ?? ''}
				</p>
			{/if}
		</div>
		<Button href={resolve('/admin/services')} variant="outline" class="h-12 w-full text-base">
			{t.back}
		</Button>
	</div>
</main>
