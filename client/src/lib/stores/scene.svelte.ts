/**
 * Scene store — the companion's shared state across routes: which companion
 * is open, whether onboarding or the chat is showing, and what the companion
 * is really doing (#86), rendered in the chat by `CompanionPresence`.
 *
 * Modes:
 *   home       — no companion open yet
 *   onboarding — the companion is being created
 *   chat       — the companion's tabs are showing
 */

import { getContext, setContext } from "svelte";
import type { RecalledMemory } from "$lib/api/types.js";
import {
	companionStatusText,
	initialCompanionState,
	reduceCompanion,
	type CompanionEvent,
	type CompanionState,
} from "$lib/companion/state.js";

const SCENE_KEY = Symbol("scene");

export type SceneMode = "home" | "onboarding" | "chat";

export interface SceneStore {
	readonly mode: SceneMode;
	readonly selectedSlug: string | null;
	recalledMemories: RecalledMemory[];
	/** What the companion is really doing, derived from runtime events (#86). */
	readonly companion: CompanionState;
	/** Accessible sentence for `companion`, e.g. "Luna is working on studio-mac: opening Finder." */
	readonly companionStatus: string;
	/** The companion's name for the status text; the product name until it is known. */
	readonly companionName: string;

	enterHome(): void;
	enterOnboarding(slug: string): void;
	finishOnboarding(): void;
	enterChat(slug: string): void;
	setCompanionName(name: string): void;
	/** Feed one runtime event through the companion-state reducer. */
	companionEvent(event: CompanionEvent): void;
}

/** How long "completed" is shown before the companion settles back to idle (ms). */
const COMPLETED_HOLD_MS = 4000;

export function createSceneStore(): SceneStore {
	let mode = $state<SceneMode>("home");
	let selectedSlug = $state<string | null>(null);
	let recalledMemories = $state<RecalledMemory[]>([]);
	// The reducer returns frozen snapshots, replaced wholesale; no deep proxy needed.
	let companion = $state.raw<CompanionState>(initialCompanionState());
	let companionName = $state("");
	const companionStatus = $derived(companionStatusText(companion, companionName || undefined));
	let settleTimer: ReturnType<typeof setTimeout> | null = null;

	function companionEvent(event: CompanionEvent) {
		const next = reduceCompanion(companion, event);
		if (next === companion) return;
		companion = next;
		if (settleTimer) {
			clearTimeout(settleTimer);
			settleTimer = null;
		}
		// "completed" is the one transient state: it is time-boxed here, never
		// inside the reducer, so the reducer stays deterministic.
		if (next.kind === "completed") {
			settleTimer = setTimeout(() => {
				settleTimer = null;
				companionEvent({ type: "settle" });
			}, COMPLETED_HOLD_MS);
		}
	}

	const store: SceneStore = {
		get mode() { return mode; },
		get selectedSlug() { return selectedSlug; },
		get recalledMemories() { return recalledMemories; },
		set recalledMemories(v) { recalledMemories = v; },
		get companion() { return companion; },
		get companionStatus() { return companionStatus; },
		get companionName() { return companionName; },

		enterHome() {
			mode = "home";
			selectedSlug = null;
		},

		enterOnboarding(slug: string) {
			selectedSlug = slug;
			mode = "onboarding";
		},

		finishOnboarding() {
			if (mode !== "onboarding") return;
			mode = "chat";
		},

		enterChat(slug: string) {
			if (mode === "onboarding") return;
			selectedSlug = slug;
			mode = "chat";
		},

		setCompanionName(name) { companionName = name; },
		companionEvent,
	};

	return store;
}

export function setSceneStore(s: SceneStore) {
	setContext(SCENE_KEY, s);
}

export function getSceneStore(): SceneStore {
	return getContext<SceneStore>(SCENE_KEY);
}
