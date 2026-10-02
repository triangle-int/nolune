<script lang="ts" module>
	export interface ViewedPicture {
		src: string;
		alt: string;
		/** File name of the original, and where to download it. */
		name: string;
		download: string;
		/** The picture's button in the chat, which gets the focus back when the viewer closes. */
		trigger: HTMLElement;
	}

	/** The pictures of one message, which the viewer goes through, and the one that was clicked. */
	export interface PictureGallery {
		pictures: ViewedPicture[];
		index: number;
	}

	function viewed(button: HTMLElement): ViewedPicture | null {
		const img = button.querySelector('img');
		if (!img) return null;
		return {
			src: img.getAttribute('src') ?? '',
			alt: img.alt,
			name: button.dataset.name ?? '',
			download: button.dataset.download ?? '',
			trigger: button
		};
	}

	/**
	 * Opens the viewer when a picture in a reply is clicked (the buttons that
	 * src/lib/markdown.ts renders), for the whole chat at once. The viewer gets every picture
	 * of the message it's in (its closest `data-media-group`), in the order they're shown.
	 */
	export function pictureClicks(open: (gallery: PictureGallery) => void) {
		return (node: HTMLElement) => {
			const onClick = (event: MouseEvent) => {
				const button = (event.target as Element).closest<HTMLElement>('[data-media-view]');
				if (!button?.querySelector('img')) return;
				const group = button.closest('[data-media-group]');
				// A group inside the message (a command's pictures) is one of its own.
				const buttons = group
					? [...group.querySelectorAll<HTMLElement>('[data-media-view]')].filter(
							(b) => b.closest('[data-media-group]') === group
						)
					: [button];
				const pictures: ViewedPicture[] = [];
				let index = 0;
				for (const b of buttons) {
					const picture = viewed(b);
					if (!picture) continue;
					if (b === button) index = pictures.length;
					pictures.push(picture);
				}
				open({ pictures, index });
			};
			node.addEventListener('click', onClick);
			return () => node.removeEventListener('click', onClick);
		};
	}
</script>

<script lang="ts">
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import { Button, buttonVariants } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	interface Props {
		gallery: PictureGallery | null;
	}

	let { gallery = $bindable() }: Props = $props();

	const { m } = getI18n();

	/** Starts at the clicked picture each time the viewer opens. */
	let index = $derived(gallery?.index ?? 0);
	const picture = $derived(gallery?.pictures[index]);
	const count = $derived(gallery?.pictures.length ?? 0);

	/** The picture last shown, kept after closing so the focus can go back to it. */
	let trigger = $state.raw<HTMLElement>();
	$effect(() => {
		if (picture) trigger = picture.trigger;
	});

	function go(step: number) {
		index = Math.min(Math.max(index + step, 0), count - 1);
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
		if (event.key === 'ArrowLeft') go(-1);
		else if (event.key === 'ArrowRight') go(1);
		else return;
		event.preventDefault();
	}

	/** Pinched in, when a swipe moves around the picture instead. */
	let zoomed = $state(false);
	$effect(() => {
		const viewport = window.visualViewport;
		if (!viewport) return;
		const update = () => (zoomed = viewport.scale > 1);
		update();
		viewport.addEventListener('resize', update);
		return () => viewport.removeEventListener('resize', update);
	});

	/**
	 * A sideways swipe anywhere in the viewer goes to the next picture or back, so one that starts
	 * on an arrow over the picture counts too. The viewer keeps sideways swipes from the browser
	 * meanwhile, which would take one to the right as going back a page.
	 */
	const swipes = $derived(count > 1 && !zoomed);
	let touch: { x: number; y: number } | null = null;
	const SWIPE = 48;

	function onTouchstart(event: TouchEvent) {
		const t = event.touches[0];
		touch = swipes && event.touches.length === 1 ? { x: t.clientX, y: t.clientY } : null;
	}

	function onTouchend(event: TouchEvent) {
		const start = touch;
		touch = null;
		const t = event.changedTouches[0];
		if (!start || !t) return;
		const dx = t.clientX - start.x;
		const dy = t.clientY - start.y;
		if (Math.abs(dx) > SWIPE && Math.abs(dx) > Math.abs(dy) * 1.5) go(dx < 0 ? 1 : -1);
	}

	function restoreFocus(event: Event) {
		// Back to the picture without scrolling the chat to it, which the default focus does.
		event.preventDefault();
		trigger?.focus({ preventScroll: true });
	}
</script>

<Dialog.Root open={gallery !== null} onOpenChange={(open) => !open && (gallery = null)}>
	<Dialog.Content
		onCloseAutoFocus={restoreFocus}
		onkeydown={onKeydown}
		ontouchstart={onTouchstart}
		ontouchend={onTouchend}
		ontouchcancel={() => (touch = null)}
		class={cn(
			// One column as wide as the viewer, else a long file name with no spaces in it
			// (a phone's photo) widens it past the screen, and the picture with it.
			// Clear of a phone's status bar, home indicator and notch too.
			'max-h-[calc(100dvh-2rem-env(safe-area-inset-top)-env(safe-area-inset-bottom))] grid-cols-1 gap-3 p-3 sm:max-w-[min(72rem,calc(100%-2rem-env(safe-area-inset-left)-env(safe-area-inset-right)))]',
			swipes && 'touch-pan-y touch-pinch-zoom'
		)}
	>
		{#if picture}
			<Dialog.Title class="sr-only">{picture.alt || picture.name}</Dialog.Title>
			<img
				src={picture.src}
				alt={picture.alt}
				class="mx-auto max-h-[calc(100dvh-8rem-env(safe-area-inset-top)-env(safe-area-inset-bottom))] w-auto max-w-full rounded-3xl object-contain"
			/>
			<!-- Halfway down the viewer, which stays in the middle of the screen whatever the
			     picture's height, so the arrows don't move away from under the pointer. -->
			{#if index > 0}
				<Button
					variant="secondary"
					size="icon"
					class="absolute top-1/2 left-5 -translate-y-1/2 shadow-md"
					onclick={() => go(-1)}
				>
					<ChevronLeftIcon />
					<span class="sr-only">{m.attachments.previous}</span>
				</Button>
			{/if}
			{#if index < count - 1}
				<Button
					variant="secondary"
					size="icon"
					class="absolute top-1/2 right-5 -translate-y-1/2 shadow-md"
					onclick={() => go(1)}
				>
					<ChevronRightIcon />
					<span class="sr-only">{m.attachments.next}</span>
				</Button>
			{/if}
			<div class="flex items-center gap-3 pl-3">
				<span class="min-w-0 flex-1 truncate text-muted-foreground">
					{#if count > 1}
						<span class="text-foreground tabular-nums"
							>{m.attachments.position(index + 1, count)}</span
						>
						·
					{/if}
					{picture.alt || picture.name}
				</span>
				<a
					href={picture.download}
					download={picture.name}
					rel="external"
					class={buttonVariants({ variant: 'outline', size: 'sm' })}
				>
					<DownloadIcon />
					{m.attachments.download}
				</a>
			</div>
		{/if}
	</Dialog.Content>
</Dialog.Root>
