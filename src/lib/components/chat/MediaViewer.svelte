<script lang="ts" module>
	export interface ViewedPicture {
		src: string;
		alt: string;
		/** File name of the original, and where to download it. */
		name: string;
		download: string;
	}

	/**
	 * Opens the viewer when a picture in a reply is clicked (the buttons that
	 * src/lib/markdown.ts renders), for the whole chat at once.
	 */
	export function pictureClicks(open: (picture: ViewedPicture) => void) {
		return (node: HTMLElement) => {
			const onClick = (event: MouseEvent) => {
				const button = (event.target as Element).closest<HTMLElement>('[data-media-view]');
				const img = button?.querySelector('img');
				if (!button || !img) return;
				open({
					src: img.getAttribute('src') ?? '',
					alt: img.alt,
					name: button.dataset.name ?? '',
					download: button.dataset.download ?? ''
				});
			};
			node.addEventListener('click', onClick);
			return () => node.removeEventListener('click', onClick);
		};
	}
</script>

<script lang="ts">
	import DownloadIcon from '@lucide/svelte/icons/download';
	import { buttonVariants } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';

	interface Props {
		picture: ViewedPicture | null;
	}

	let { picture = $bindable() }: Props = $props();
</script>

<Dialog.Root open={picture !== null} onOpenChange={(open) => !open && (picture = null)}>
	<Dialog.Content class="max-h-[calc(100dvh-2rem)] gap-3 p-3 sm:max-w-[min(72rem,calc(100%-2rem))]">
		{#if picture}
			<Dialog.Title class="sr-only">{picture.alt || picture.name}</Dialog.Title>
			<img
				src={picture.src}
				alt={picture.alt}
				class="mx-auto max-h-[calc(100dvh-8rem)] w-auto max-w-full rounded-3xl object-contain"
			/>
			<div class="flex items-center gap-3 pl-3">
				<span class="min-w-0 flex-1 truncate text-muted-foreground"
					>{picture.alt || picture.name}</span
				>
				<a
					href={picture.download}
					download={picture.name}
					rel="external"
					class={buttonVariants({ variant: 'outline', size: 'sm' })}
				>
					<DownloadIcon />
					Download
				</a>
			</div>
		{/if}
	</Dialog.Content>
</Dialog.Root>
