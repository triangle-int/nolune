<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import EraserIcon from '@lucide/svelte/icons/eraser';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import Undo2Icon from '@lucide/svelte/icons/undo-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { getI18n } from '$lib/i18n';
	import { cn } from '$lib/utils';

	interface Props {
		/** Called with the drawing as a PNG file. */
		ondone: (file: File) => void;
		onclose: () => void;
	}

	let { ondone, onclose }: Props = $props();

	const { m } = getI18n();

	const COLORS = [
		'#111111',
		'#8a8a8a',
		'#e5484d',
		'#f76b15',
		'#ffc53d',
		'#46a758',
		'#0090ff',
		'#8e4ec6',
		'#a1662f'
	];
	/** The paper: the eraser paints it back. */
	const PAPER = '#ffffff';
	const EXPORT_SIZE = 1024;

	/** Points and widths are fractions of the canvas side, so the drawing survives resizing. */
	interface Stroke {
		color: string;
		width: number;
		points: [number, number][];
	}

	let strokes = $state<Stroke[]>([]);
	let tool = $state<'pen' | 'eraser'>('pen');
	let color = $state(COLORS[0]);
	/** Pen width, 1-40, as a share of the side: 10 is 1%. */
	let size = $state(10);
	let canvas = $state<HTMLCanvasElement>();
	let side = $state(0);
	let current: Stroke | null = null;

	function paint(ctx: CanvasRenderingContext2D, stroke: Stroke, px: number) {
		const [first, ...rest] = stroke.points;
		ctx.strokeStyle = stroke.color;
		ctx.fillStyle = stroke.color;
		ctx.lineWidth = stroke.width * px;
		ctx.lineCap = 'round';
		ctx.lineJoin = 'round';
		if (!rest.length) {
			// A tap is a dot.
			ctx.beginPath();
			ctx.arc(first[0] * px, first[1] * px, (stroke.width * px) / 2, 0, Math.PI * 2);
			ctx.fill();
			return;
		}
		ctx.beginPath();
		ctx.moveTo(first[0] * px, first[1] * px);
		// Through the midpoints, so fast strokes come out as curves rather than corners.
		for (let i = 0; i < rest.length - 1; i++) {
			const [x, y] = rest[i];
			const [nx, ny] = rest[i + 1];
			ctx.quadraticCurveTo(x * px, y * px, ((x + nx) / 2) * px, ((y + ny) / 2) * px);
		}
		const [lx, ly] = rest[rest.length - 1];
		ctx.lineTo(lx * px, ly * px);
		ctx.stroke();
	}

	function redraw(target: HTMLCanvasElement, px: number) {
		const ctx = target.getContext('2d');
		if (!ctx) return;
		ctx.fillStyle = PAPER;
		ctx.fillRect(0, 0, target.width, target.height);
		for (const stroke of strokes) paint(ctx, stroke, px);
		if (current) paint(ctx, current, px);
	}

	/** Keeps the canvas square and sharp as its space changes. */
	function fit(node: HTMLElement) {
		const observer = new ResizeObserver(([entry]) => {
			const { width, height } = entry.contentRect;
			side = Math.floor(Math.min(width, height));
		});
		observer.observe(node);
		return () => observer.disconnect();
	}

	$effect(() => {
		if (!canvas || !side) return;
		const ratio = window.devicePixelRatio || 1;
		canvas.width = Math.round(side * ratio);
		canvas.height = Math.round(side * ratio);
		void strokes.length;
		redraw(canvas, canvas.width);
	});

	function point(event: PointerEvent): [number, number] {
		const rect = canvas!.getBoundingClientRect();
		return [(event.clientX - rect.left) / rect.width, (event.clientY - rect.top) / rect.height];
	}

	function down(event: PointerEvent) {
		if (!canvas || event.button > 0) return;
		canvas.setPointerCapture(event.pointerId);
		current = {
			color: tool === 'eraser' ? PAPER : color,
			width: (tool === 'eraser' ? size * 3 : size) / 1000,
			points: [point(event)]
		};
		redraw(canvas, canvas.width);
	}

	function move(event: PointerEvent) {
		if (!current || !canvas) return;
		for (const e of event.getCoalescedEvents?.() ?? [event]) current.points.push(point(e));
		redraw(canvas, canvas.width);
	}

	function up() {
		if (!current) return;
		strokes.push(current);
		current = null;
	}

	async function done() {
		const out = document.createElement('canvas');
		out.width = EXPORT_SIZE;
		out.height = EXPORT_SIZE;
		redraw(out, EXPORT_SIZE);
		const blob = await new Promise<Blob | null>((resolve) => out.toBlob(resolve, 'image/png'));
		if (blob) ondone(new File([blob], 'drawing.png', { type: 'image/png' }));
	}
</script>

<!-- Above the template dialog, which keeps its overlay and blocks the page's pointer events
	until it has finished closing: that would swallow the first stroke. -->
<div
	class="pointer-events-auto fixed inset-0 z-[60] flex flex-col bg-background pt-[env(safe-area-inset-top)] pr-[env(safe-area-inset-right)] pb-[max(0.75rem,env(safe-area-inset-bottom))] pl-[env(safe-area-inset-left)]"
	role="dialog"
	aria-modal="true"
	aria-label={m.drawing.title}
>
	<div class="flex items-center gap-2 px-3 py-3">
		<button
			type="button"
			onclick={onclose}
			class="flex size-10 items-center justify-center rounded-full bg-muted hover:bg-accent"
			aria-label={m.common.close}
		>
			<XIcon class="size-5" />
		</button>
		<div class="flex flex-1 justify-center">
			<div
				class="flex gap-1 rounded-full bg-muted p-1"
				role="radiogroup"
				aria-label={m.drawing.tool}
			>
				{#each [{ id: 'pen', icon: PencilIcon, label: m.drawing.pen }, { id: 'eraser', icon: EraserIcon, label: m.drawing.eraser }] as t (t.id)}
					<button
						type="button"
						role="radio"
						aria-checked={tool === t.id}
						aria-label={t.label}
						onclick={() => (tool = t.id as 'pen' | 'eraser')}
						class={cn(
							'flex h-8 w-11 items-center justify-center rounded-full transition-colors',
							tool === t.id ? 'bg-background shadow-xs' : 'text-muted-foreground'
						)}
					>
						<t.icon class="size-4.5" />
					</button>
				{/each}
			</div>
		</div>
		<button
			type="button"
			onclick={() => strokes.pop()}
			disabled={!strokes.length}
			class="flex size-10 items-center justify-center rounded-full bg-muted hover:bg-accent disabled:opacity-40"
			aria-label={m.drawing.undo}
		>
			<Undo2Icon class="size-5" />
		</button>
	</div>

	<div class="relative flex min-h-0 flex-1 px-3">
		<input
			type="range"
			min="2"
			max="40"
			bind:value={size}
			aria-label={m.drawing.penSize}
			class="absolute top-1/2 left-4 z-10 h-40 w-6 -translate-y-1/2 accent-foreground [direction:rtl] [writing-mode:vertical-lr]"
		/>
		<div class="flex min-h-0 flex-1 items-center justify-center" {@attach fit}>
			<canvas
				bind:this={canvas}
				style:width="{side}px"
				style:height="{side}px"
				onpointerdown={down}
				onpointermove={move}
				onpointerup={up}
				onpointercancel={up}
				class="touch-none rounded-3xl shadow-sm ring-1 ring-border"
			></canvas>
		</div>
	</div>

	<div class="flex items-center gap-3 px-3 pt-3">
		<div class="no-scrollbar flex min-w-0 flex-1 gap-2 overflow-x-auto py-1 pl-1">
			{#each COLORS as swatch (swatch)}
				<button
					type="button"
					onclick={() => {
						color = swatch;
						tool = 'pen';
					}}
					class={cn(
						'size-8 shrink-0 rounded-full ring-offset-2 ring-offset-background transition-shadow',
						color === swatch && tool === 'pen' && 'ring-2 ring-foreground'
					)}
					style:background-color={swatch}
					aria-label={m.drawing.color(swatch)}
				></button>
			{/each}
		</div>
		<button
			type="button"
			onclick={done}
			disabled={!strokes.length}
			class="flex size-12 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground hover:opacity-80 disabled:opacity-30"
			aria-label={m.drawing.use}
		>
			<CheckIcon class="size-6" />
		</button>
	</div>
</div>
