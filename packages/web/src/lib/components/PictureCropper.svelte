<script lang="ts">
	import { SvelteMap } from 'svelte/reactivity';
	import { Button } from '$lib/components/ui/button';
	import { CENTERED, MAX_ZOOM, panBy, sourceSquare, zoomTo, type Crop } from '$lib/crop';
	import { getI18n } from '$lib/i18n';

	interface Props {
		/** The picture as it was picked, any size. */
		image: ImageBitmap;
		/** While the cropped one uploads. */
		saving?: boolean;
		oncancel: () => void;
		/** With the square the circle shows, OUTPUT pixels a side. */
		onsave: (picture: Blob) => void;
	}

	let { image, saving = false, oncancel, onsave }: Props = $props();

	const { m } = getI18n();
	/** The square on the page, in CSS pixels. */
	const STAGE = 224;
	/** The saved square, in pixels: sharp at three times the largest avatar on the page. */
	const OUTPUT = 256;
	const dpr = Math.min(3, window.devicePixelRatio || 1);

	let crop = $state<Crop>(CENTERED);
	let canvas = $state<HTMLCanvasElement>();

	function draw(target: HTMLCanvasElement) {
		const context = target.getContext('2d');
		if (!context) return;
		context.imageSmoothingQuality = 'high';
		const { sx, sy, size } = sourceSquare(crop, image.width, image.height);
		context.clearRect(0, 0, target.width, target.height);
		context.drawImage(image, sx, sy, size, size, 0, 0, target.width, target.height);
	}

	$effect(() => {
		if (canvas) draw(canvas);
	});

	const pan = (dx: number, dy: number) => (crop = panBy(crop, dx, dy, image.width, image.height));
	const zoom = (to: number) => (crop = zoomTo(crop, to, image.width, image.height));

	/** Pointers down on the square: one drags, two pinch. */
	const pointers = new SvelteMap<number, { x: number; y: number }>();
	let pinch: { spread: number; zoom: number } | null = null;

	function spread(): number {
		const [a, b] = [...pointers.values()];
		return Math.hypot(a.x - b.x, a.y - b.y) || 1;
	}

	function onpointerdown(event: PointerEvent & { currentTarget: HTMLElement }) {
		event.currentTarget.setPointerCapture(event.pointerId);
		pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
		if (pointers.size === 2) pinch = { spread: spread(), zoom: crop.zoom };
	}

	function onpointermove(event: PointerEvent) {
		const last = pointers.get(event.pointerId);
		if (!last) return;
		pointers.set(event.pointerId, { x: event.clientX, y: event.clientY });
		if (pinch && pointers.size === 2) zoom((pinch.zoom * spread()) / pinch.spread);
		else if (pointers.size === 1)
			pan((event.clientX - last.x) / STAGE, (event.clientY - last.y) / STAGE);
	}

	function onpointerup(event: PointerEvent) {
		pointers.delete(event.pointerId);
		pinch = null;
	}

	function onwheel(event: WheelEvent) {
		event.preventDefault();
		zoom(crop.zoom * Math.exp(-event.deltaY * 0.002));
	}

	function onkeydown(event: KeyboardEvent) {
		const step = event.shiftKey ? 0.1 : 0.02;
		const moves: Record<string, [number, number]> = {
			ArrowLeft: [-step, 0],
			ArrowRight: [step, 0],
			ArrowUp: [0, -step],
			ArrowDown: [0, step]
		};
		if (event.key in moves) pan(...moves[event.key]);
		else if (event.key === '+' || event.key === '=') zoom(crop.zoom * 1.1);
		else if (event.key === '-') zoom(crop.zoom / 1.1);
		else return;
		event.preventDefault();
	}

	function save() {
		const out = document.createElement('canvas');
		out.width = out.height = OUTPUT;
		draw(out);
		// WebP where the browser can write it; the others give PNG, which is fine too.
		out.toBlob((blob) => blob && onsave(blob), 'image/webp', 0.9);
	}
</script>

<div class="flex flex-col items-center gap-4">
	<!--
		The square is what's saved; the circle is what avatars show of it. It takes the arrow keys and
		+/- itself, as an application, which Svelte counts as not interactive.
	-->
	<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
	<div
		role="application"
		aria-label={m.account.cropHint}
		tabindex="0"
		class="relative cursor-grab touch-none overflow-hidden rounded-3xl bg-muted outline-none select-none focus-visible:ring-3 focus-visible:ring-ring/30 active:cursor-grabbing"
		style:width="{STAGE}px"
		style:height="{STAGE}px"
		{onpointerdown}
		{onpointermove}
		{onpointerup}
		onpointercancel={onpointerup}
		{onwheel}
		{onkeydown}
	>
		<canvas
			bind:this={canvas}
			width={Math.round(STAGE * dpr)}
			height={Math.round(STAGE * dpr)}
			class="size-full"
		></canvas>
		<div
			class="pointer-events-none absolute inset-0 rounded-full shadow-[0_0_0_999px_rgb(0_0_0/0.45)] ring-2 ring-white/70"
		></div>
	</div>

	<label class="flex w-full max-w-64 items-center gap-3 text-muted-foreground">
		<span class="shrink-0">{m.account.zoom}</span>
		<input
			type="range"
			min="1"
			max={MAX_ZOOM}
			step="0.01"
			value={crop.zoom}
			oninput={(event) => zoom(event.currentTarget.valueAsNumber)}
			class="min-w-0 flex-1 accent-primary"
		/>
	</label>
	<p class="text-center text-muted-foreground">{m.account.cropHint}</p>

	<div class="flex w-full justify-end gap-2">
		<Button variant="ghost" onclick={oncancel} disabled={saving}>{m.common.cancel}</Button>
		<Button onclick={save} disabled={saving}>{m.account.usePicture}</Button>
	</div>
</div>
