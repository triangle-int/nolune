import { tick } from 'svelte';

/** The row being dragged, as the floating copy draws it (viewport pixels). */
export interface DraggedRow<T> {
	item: T;
	left: number;
	width: number;
	height: number;
	top: number;
	/**
	 * What is under the row: the `data-drop-target` of a folder's row or its list of chats (the
	 * folder's id) or of the chat list (''); null over anything else.
	 */
	target: string | null;
	/** Whether the gap in the lists grows and shrinks; not when the drag starts or ends. */
	animateGap: boolean;
	/** Dropped: the row slides to `top` (and shrinks away into a closed folder when `vanish`). */
	settling: { top: number; vanish: boolean } | null;
}

export interface ChatDragOptions<T> {
	/** The scrolling list the row is kept inside. */
	container: () => HTMLElement | null | undefined;
	/** Where a dropped row goes: the gap it will fill, or a closed folder's row to vanish into. */
	settleOn: () => { el: Element; vanish: boolean } | null;
	/** The drop, once the row has slid into place. `target` null: it goes back. */
	drop: (item: T, target: string | null) => void;
	/** Called when the target under the row changes. */
	hover?: (target: string | null) => void;
}

/** How far a mouse moves before a press becomes a drag. */
const MOUSE_SLOP = 4;
/** On touch screens a drag starts with a long press; moving earlier is scrolling. */
const TOUCH_HOLD_MS = 400;
const TOUCH_SLOP = 8;
/** Near this far from the top or bottom of the list, it scrolls. */
const EDGE = 48;
const MAX_SCROLL_STEP = 14;
export const SETTLE_MS = 180;

/**
 * Dragging chats in the sidebar, with pointer events rather than the browser's drag and drop, so
 * it works the same with a mouse and a finger. The row only moves up and down, stays inside the
 * scrolling list (which scrolls near its edges), and lands where the page says it will.
 */
export class ChatDrag<T> {
	row = $state<DraggedRow<T> | null>(null);

	#options: ChatDragOptions<T>;
	/** Where the pointer grabbed the row, from its top. */
	#grab = 0;
	#pointerY = 0;
	#frame = 0;

	constructor(options: ChatDragOptions<T>) {
		this.#options = options;
	}

	/**
	 * An attachment for the scrolling list. A blocking `touchmove` listener has to be there before
	 * a touch starts, or the browser scrolls without asking the page; with it, `touch` below can
	 * stop the scrolling once a row is picked up.
	 */
	holdScroll = (node: HTMLElement) => {
		const hold = (e: TouchEvent) => {
			if (this.row) e.preventDefault();
		};
		node.addEventListener('touchmove', hold, { passive: false });
		return () => node.removeEventListener('touchmove', hold);
	};

	/** On a row's `pointerdown`: mouse and pen drag once they move a little. */
	press(event: PointerEvent & { currentTarget: HTMLElement }, item: T) {
		// Fingers go through `touch`: pointer events are cancelled as soon as the list scrolls.
		if (event.pointerType === 'touch' || !event.isPrimary || event.button !== 0) return;
		const { pointerId } = event;
		const gesture = this.#gesture(event.currentTarget, item, event.clientX, event.clientY, false);
		if (!gesture) return;
		const move = (e: PointerEvent) => {
			if (e.pointerId === pointerId) gesture.move(e.clientX, e.clientY);
		};
		const up = (e: PointerEvent) => {
			if (e.pointerId === pointerId) gesture.end();
		};
		const cancel = (e: PointerEvent) => {
			if (e.pointerId === pointerId) gesture.end(true);
		};
		window.addEventListener('pointermove', move);
		window.addEventListener('pointerup', up);
		window.addEventListener('pointercancel', cancel);
		gesture.onStop(() => {
			window.removeEventListener('pointermove', move);
			window.removeEventListener('pointerup', up);
			window.removeEventListener('pointercancel', cancel);
		});
	}

	/**
	 * On a row's `touchstart`: a long press picks the row up, and moving before that scrolls the
	 * list as usual. Touch events keep going to the element the touch started on, even after the
	 * row leaves the list, so the listeners go on it.
	 */
	touch(event: TouchEvent & { currentTarget: HTMLElement }, item: T) {
		if (event.touches.length !== 1) return;
		const element = event.currentTarget;
		const { identifier, clientX, clientY } = event.touches[0];
		const gesture = this.#gesture(element, item, clientX, clientY, true);
		if (!gesture) return;
		const mine = (e: TouchEvent) =>
			Array.from(e.changedTouches).find((t) => t.identifier === identifier);
		const move = (e: TouchEvent) => {
			const t = mine(e);
			if (!t) return;
			if (gesture.started()) e.preventDefault();
			gesture.move(t.clientX, t.clientY);
		};
		const end = (e: TouchEvent) => {
			if (mine(e)) gesture.end(e.type === 'touchcancel');
		};
		// A long press would open the link's menu.
		const noMenu = (e: Event) => e.preventDefault();
		element.addEventListener('touchmove', move, { passive: false });
		element.addEventListener('touchend', end);
		element.addEventListener('touchcancel', end);
		element.addEventListener('contextmenu', noMenu);
		gesture.onStop(() => {
			element.removeEventListener('touchmove', move);
			element.removeEventListener('touchend', end);
			element.removeEventListener('touchcancel', end);
			element.removeEventListener('contextmenu', noMenu);
		});
	}

	/**
	 * A press on a row, from either input: it becomes a drag after moving a little (mouse) or a
	 * long press (touch), and ends in a drop, or back in its place when cancelled.
	 */
	#gesture(element: HTMLElement, item: T, startX: number, startY: number, touch: boolean) {
		if (this.row) return null;
		let started = false;
		let stopped = false;
		const cleanups: (() => void)[] = [];
		this.#pointerY = startY;

		const start = () => {
			started = true;
			const rect = element.getBoundingClientRect();
			this.#grab = startY - rect.top;
			this.row = {
				item,
				left: rect.left,
				width: rect.width,
				height: rect.height,
				top: rect.top,
				target: null,
				animateGap: false,
				settling: null
			};
			document.documentElement.classList.add('dragging-chat');
			if (touch) navigator.vibrate?.(10);
			this.#follow();
			this.#scrollNearEdges();
			// The gap takes the row's place at once; from now on it slides.
			tick().then(() => {
				if (this.row) this.row.animateGap = true;
			});
		};
		const hold = touch ? setTimeout(start, TOUCH_HOLD_MS) : undefined;

		const stop = () => {
			if (stopped) return;
			stopped = true;
			clearTimeout(hold);
			window.removeEventListener('keydown', escape);
			for (const cleanup of cleanups) cleanup();
		};
		const escape = (e: KeyboardEvent) => {
			if (e.key !== 'Escape' || !started) return;
			this.#drop(true);
			stop();
		};
		window.addEventListener('keydown', escape);

		return {
			started: () => started,
			onStop: (cleanup: () => void) => cleanups.push(cleanup),
			move: (x: number, y: number) => {
				if (stopped) return;
				this.#pointerY = y;
				if (started) return this.#follow();
				const distance = Math.hypot(x - startX, y - startY);
				// A finger moving before the long press is scrolling the list.
				if (touch && distance > TOUCH_SLOP) stop();
				else if (!touch && distance > MOUSE_SLOP) start();
			},
			/** Let go (or cancelled, `back`). */
			end: (back = false) => {
				if (stopped) return;
				if (started) {
					if (!back) swallowClick();
					this.#drop(back);
				}
				stop();
			}
		};
	}

	/** Moves the row with the pointer, up and down only, kept inside the list. */
	#follow() {
		const row = this.row;
		if (!row || row.settling) return;
		let top = this.#pointerY - this.#grab;
		const box = this.#options.container()?.getBoundingClientRect();
		if (box) top = Math.min(Math.max(top, box.top), box.bottom - row.height);
		row.top = top;
		this.#retarget();
	}

	/** What's under the middle of the row (the floating copy lets the pointer through). */
	#retarget() {
		const row = this.row;
		if (!row) return;
		const under = document.elementFromPoint(row.left + row.width / 2, row.top + row.height / 2);
		const target = under?.closest('[data-drop-target]')?.getAttribute('data-drop-target') ?? null;
		if (target === row.target) return;
		row.target = target;
		this.#options.hover?.(target);
	}

	#scrollNearEdges() {
		const step = () => {
			const row = this.row;
			const container = this.#options.container();
			if (!row || row.settling || !container) return;
			const box = container.getBoundingClientRect();
			const y = this.#pointerY;
			let speed = 0;
			if (y < box.top + EDGE) speed = -Math.min(MAX_SCROLL_STEP, (box.top + EDGE - y) / 3);
			else if (y > box.bottom - EDGE)
				speed = Math.min(MAX_SCROLL_STEP, (y - box.bottom + EDGE) / 3);
			if (speed) {
				container.scrollTop += speed;
				this.#retarget();
			}
			this.#frame = requestAnimationFrame(step);
		};
		this.#frame = requestAnimationFrame(step);
	}

	/** Slides the row into its place (or back, when `back`), then hands over to `drop`. */
	async #drop(back = false) {
		const row = this.row;
		if (!row || row.settling) return;
		cancelAnimationFrame(this.#frame);
		if (back) row.target = null;
		// Let the gap move to where the row lands before measuring it.
		await tick();
		const spot = this.#options.settleOn();
		row.settling = {
			top: spot ? spot.el.getBoundingClientRect().top : row.top,
			vanish: spot?.vanish ?? false
		};
		setTimeout(() => {
			if (this.row !== row) return;
			this.#options.drop(row.item, row.target);
			this.row = null;
			document.documentElement.classList.remove('dragging-chat');
		}, SETTLE_MS);
	}
}

/** The click that follows letting go of a dragged link would open it. */
function swallowClick() {
	const swallow = (e: Event) => {
		e.preventDefault();
		e.stopPropagation();
	};
	window.addEventListener('click', swallow, { capture: true, once: true });
	setTimeout(() => window.removeEventListener('click', swallow, { capture: true }), 0);
}
