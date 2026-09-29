<script lang="ts">
	import { resolve } from '$app/paths';
	import type { DisplayPicture } from '@nolune/core';
	import { getI18n } from '$lib/i18n';

	interface Props {
		conversationId: string;
		picture: DisplayPicture;
	}

	let { conversationId, picture }: Props = $props();

	const { m } = getI18n();

	/** Pictures in the chat are at most this tall; the viewer shows them full size. */
	const MAX_HEIGHT = 192;

	const url = $derived(
		resolve('/api/c/[id]/media/[mediaId]', { id: conversationId, mediaId: picture.id })
	);

	/** The size it's shown at, so the chat doesn't jump while it loads. */
	const size = $derived.by(() => {
		if (!picture.width || !picture.height) return {};
		const scale = Math.min(1, MAX_HEIGHT / picture.height);
		return {
			width: Math.round(picture.width * scale),
			height: Math.round(picture.height * scale)
		};
	});
</script>

<!-- The same markup as pictures in replies, so a click opens the chat's viewer. -->
<button
	type="button"
	data-media-view
	data-name={picture.name}
	data-download="{url}?download"
	aria-label={m.attachments.open(picture.name)}
	class="block max-w-full cursor-zoom-in overflow-hidden rounded-xl border"
>
	<img
		src={url}
		alt={picture.name}
		{...size}
		loading="lazy"
		decoding="async"
		class="block h-auto max-h-48 max-w-full bg-muted"
	/>
</button>
