<script lang="ts">
	import { resolve } from '$app/paths';
	import type { DisplayAttachment } from '@btw/core';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import FileIcon from '@lucide/svelte/icons/file';
	import { formatBytes } from '$lib/format';
	import { getI18n } from '$lib/i18n';

	interface Props {
		conversationId: string;
		attachments: DisplayAttachment[];
	}

	let { conversationId, attachments }: Props = $props();

	const i18n = getI18n();
	const { m } = i18n;

	/** Pictures in a message are at most this tall; the viewer shows them full size. */
	const MAX_HEIGHT = 192;

	function url(id: string): string {
		return resolve('/api/c/[id]/media/[mediaId]', { id: conversationId, mediaId: id });
	}

	function size(a: DisplayAttachment): { width?: number; height?: number } {
		if (!a.width || !a.height) return {};
		const scale = Math.min(1, MAX_HEIGHT / a.height);
		return { width: Math.round(a.width * scale), height: Math.round(a.height * scale) };
	}
</script>

<!-- Pictures use the same markup as pictures in replies, so a click opens the chat's viewer. -->
<div class="flex max-w-[85%] flex-wrap justify-end gap-1.5 sm:max-w-[70%]">
	{#each attachments as a (a.id)}
		<div class="flex max-w-full flex-col items-end gap-1">
			{#if a.viewable}
				<button
					type="button"
					data-media-view
					data-name={a.name}
					data-download="{url(a.id)}?download"
					aria-label={m.attachments.open(a.name)}
					class="block max-w-full cursor-zoom-in overflow-hidden rounded-xl border"
				>
					<img
						src={url(a.id)}
						alt={a.name}
						{...size(a)}
						loading="lazy"
						decoding="async"
						class="block h-auto max-h-48 max-w-full bg-muted"
					/>
				</button>
			{:else}
				<!-- eslint-disable svelte/no-navigation-without-resolve -- a download from an API route, not a page -->
				<a
					href="{url(a.id)}?download"
					download={a.name}
					class="inline-flex max-w-full items-center gap-3 rounded-xl border px-3 py-2 leading-snug hover:bg-muted"
				>
					<FileIcon class="size-5 shrink-0 text-muted-foreground" />
					<span class="min-w-0">
						<span class="block truncate text-sm font-medium">{a.name}</span>
						<span class="block truncate text-xs text-muted-foreground"
							>{formatBytes(a.bytes, i18n)}</span
						>
					</span>
					<DownloadIcon class="size-4 shrink-0 text-muted-foreground" />
				</a>
				<!-- eslint-enable svelte/no-navigation-without-resolve -->
			{/if}
			{#if a.note}
				<span class="max-w-72 text-right text-xs text-muted-foreground">
					{m.attachments.onlyPath(a.note)}
				</span>
			{/if}
		</div>
	{/each}
</div>
