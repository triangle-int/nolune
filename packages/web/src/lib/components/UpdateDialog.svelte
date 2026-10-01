<script lang="ts">
	import DownloadIcon from '@lucide/svelte/icons/download';
	import type { Update } from '@nolune/core';
	import CopyField from '$lib/components/admin/CopyField.svelte';
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { getI18n } from '$lib/i18n';

	interface Props {
		open: boolean;
		/** A newer nolune, and how this install gets it (updates.ts in core). */
		update: Update;
	}

	let { open = $bindable(), update }: Props = $props();

	const { m } = getI18n();
	let content = $state<HTMLElement | null>(null);
</script>

<Dialog.Root bind:open>
	<!-- Opens on the dialog: on the copy button, its tooltip would show before anything's read. -->
	<Dialog.Content
		bind:ref={content}
		class="sm:max-w-md"
		onOpenAutoFocus={(event) => {
			event.preventDefault();
			content?.focus();
		}}
	>
		<Dialog.Header>
			<Dialog.Title>{m.update.available(update.version)}</Dialog.Title>
			<Dialog.Description>{m.update.running(update.current)}</Dialog.Description>
		</Dialog.Header>

		<div class="space-y-3 text-sm">
			{#if update.command}
				<p>{update.install === 'source' ? m.update.source : m.update.npm}</p>
				<CopyField text={update.command} label={m.update.copy} />
			{:else}
				<p>{m.update.app}</p>
			{/if}
			<p class="text-muted-foreground">{m.update.kept}</p>
		</div>

		<!-- eslint-disable svelte/no-navigation-without-resolve -- nolune's releases on GitHub -->
		<Dialog.Footer>
			<Button variant="outline" href={update.url} target="_blank" rel="noreferrer">
				{m.update.whatsNew}
			</Button>
			{#if update.install === 'app'}
				<Button href={update.download ?? update.url} target="_blank" rel="noreferrer">
					<DownloadIcon />
					{m.update.download}
				</Button>
			{/if}
		</Dialog.Footer>
		<!-- eslint-enable svelte/no-navigation-without-resolve -->
	</Dialog.Content>
</Dialog.Root>
