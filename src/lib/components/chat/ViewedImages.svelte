<script lang="ts">
	import { resolve } from '$app/paths';
	import type { DisplayViewedImage } from '@btw/core';

	interface Props {
		conversationId: string;
		/** What the command looked at with `btw view`. */
		images: DisplayViewedImage[];
	}

	let { conversationId, images }: Props = $props();

	/** Pictures under a step are at most this tall; the viewer shows them full size. */
	const MAX_HEIGHT = 128;

	function url(id: string): string {
		return resolve('/api/c/[id]/media/[mediaId]', { id: conversationId, mediaId: id });
	}

	function size(image: DisplayViewedImage): { width?: number; height?: number } {
		if (!image.width || !image.height) return {};
		const scale = Math.min(1, MAX_HEIGHT / image.height);
		return { width: Math.round(image.width * scale), height: Math.round(image.height * scale) };
	}
</script>

<!-- The same markup as pictures in replies, so a click opens the chat's viewer. -->
<div class="mt-1.5 mb-0.5 flex flex-wrap gap-1.5">
	{#each images as image (image.id)}
		<button
			type="button"
			data-media-view
			data-name={image.name}
			data-download="{url(image.id)}?download"
			aria-label="Open {image.name}"
			title={image.path}
			class="block max-w-full cursor-zoom-in overflow-hidden rounded-xl border"
		>
			<img
				src={url(image.id)}
				alt={image.name}
				{...size(image)}
				loading="lazy"
				decoding="async"
				class="block h-auto max-h-32 max-w-full bg-muted"
			/>
		</button>
	{/each}
</div>
