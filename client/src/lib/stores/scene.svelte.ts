/**
 * Unified scene store — manages the shared 3D scene state across all routes.
 *
 * Modes:
 *   home      — glass orbs for each instance, raycasting active
 *   selecting — clicked orb moves to center, others fade, navigation starts
 *   intro     — sphere does rising/traveling/settling cinematic
 *   chat      — sphere at final position, mood/voice reactive
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

export type SceneMode = "home" | "selecting" | "onboarding" | "intro" | "chat";
export type IntroPhase = "idle" | "rising" | "traveling" | "settling" | "done";

export interface SceneStore {
	readonly mode: SceneMode;
	readonly selectedSlug: string | null;
	readonly introProgress: number;
	readonly introPhase: IntroPhase;
	readonly selectProgress: number;
	readonly mood: string;
	readonly thinking: boolean;
	readonly voiceAmplitude: number;
	presenting: boolean;
	recalledMemories: RecalledMemory[];
	/** What the companion is really doing, derived from runtime events (#86). */
	readonly companion: CompanionState;
	/** Accessible sentence for `companion`, e.g. "Nolune is working on studio-mac: opening Finder." */
	readonly companionStatus: string;

	enterHome(): void;
	enterOnboarding(slug: string): void;
	finishOnboarding(): void;
	enterChat(slug: string): void;
	setMood(m: string): void;
	setThinking(v: boolean): void;
	setVoiceAmplitude(v: number): void;
	/** Feed one runtime event through the companion-state reducer. */
	companionEvent(event: CompanionEvent): void;
	skipIntro(): void;
	tick(): void;
}

const introPlayedSlugs = new Set<string>();

// Timing constants (seconds)
const SELECT_DURATION = 0.7;
const INTRO_DURATION = 6.0;
const PHASE_TRAVELING = 1.5;
const PHASE_SETTLING = 3.5;
/** How long "completed" is shown before the companion settles back to idle (ms). */
const COMPLETED_HOLD_MS = 4000;

export function createSceneStore(): SceneStore {
	let mode = $state<SceneMode>("home");
	let selectedSlug = $state<string | null>(null);
	let introProgress = $state(0);
	let introPhase = $state<IntroPhase>("idle");
	let selectProgress = $state(0);
	let mood = $state("calm");
	let thinking = $state(false);
	let voiceAmplitude = $state(0);
	let presenting = $state(false);
	let recalledMemories = $state<RecalledMemory[]>([]);
	// The reducer returns frozen snapshots, replaced wholesale; no deep proxy needed.
	let companion = $state.raw<CompanionState>(initialCompanionState());
	const companionStatus = $derived(companionStatusText(companion));
	let settleTimer: ReturnType<typeof setTimeout> | null = null;

	let selectStartTime = 0;
	let introStartTime = 0;

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

	// ── Tick — called every frame by SharedScene ──
	function tick() {
		if (mode === "selecting") {
			const elapsed = (performance.now() - selectStartTime) / 1000;
			selectProgress = Math.min(elapsed / SELECT_DURATION, 1);
			if (elapsed >= SELECT_DURATION) {
				mode = "intro";
				introStartTime = performance.now();
				introProgress = 0;
				introPhase = "rising";
				selectProgress = 1;
			}
		} else if (mode === "intro") {
			const elapsed = (performance.now() - introStartTime) / 1000;
			introProgress = Math.min(elapsed / INTRO_DURATION, 1);
			if (elapsed < PHASE_TRAVELING) {
				introPhase = "rising";
			} else {
				// Switch to chat as soon as sphere starts traveling to final pos.
				// The sphere animation continues smoothly via lerp in SharedScene.
				mode = "chat";
				introPhase = "done";
				introProgress = 1;
			}
		}
	}

	const store: SceneStore = {
		get mode() { return mode; },
		get selectedSlug() { return selectedSlug; },
		get introProgress() { return introProgress; },
		get introPhase() { return introPhase; },
		get selectProgress() { return selectProgress; },
		get mood() { return mood; },
		get thinking() { return thinking; },
		get voiceAmplitude() { return voiceAmplitude; },
		get presenting() { return presenting; },
		set presenting(v) { presenting = v; },
		get recalledMemories() { return recalledMemories; },
		set recalledMemories(v) { recalledMemories = v; },
		get companion() { return companion; },
		get companionStatus() { return companionStatus; },

		enterHome() {
			if (mode === "selecting" || mode === "intro") return;
			// Clear played so intro replays on next visit
			introPlayedSlugs.clear();
			mode = "home";
			selectedSlug = null;
			introProgress = 0;
			introPhase = "idle";
			selectProgress = 0;
		},

		enterOnboarding(slug: string) {
			selectedSlug = slug;
			mode = "onboarding";
			introProgress = 0;
			introPhase = "idle";
		},

		finishOnboarding() {
			// Transition onboarding → intro → chat
			if (mode !== "onboarding") return;
			mode = "intro";
			introStartTime = performance.now();
			introProgress = 0;
			introPhase = "rising";
			if (selectedSlug) {
				introPlayedSlugs.add(selectedSlug);
			}
		},

		enterChat(slug: string) {
			if (mode === "selecting" || mode === "intro" || mode === "onboarding") return;
			selectedSlug = slug;
			// Skip intro on mobile — 3D animation not visible, just delays UI
			const isMobile = typeof window !== "undefined" && window.innerWidth < 640;
			if (isMobile || introPlayedSlugs.has(slug)) {
				introPlayedSlugs.add(slug);
				mode = "chat";
				introPhase = "done";
				introProgress = 1;
			} else {
				introPlayedSlugs.add(slug);
				mode = "intro";
				introStartTime = performance.now();
				introProgress = 0;
				introPhase = "rising";
			}
		},

		setMood(m) { mood = m; },
		setThinking(v) { thinking = v; },
		setVoiceAmplitude(v) { voiceAmplitude = v; },
		companionEvent,
		skipIntro() {
			mode = "chat";
			introPhase = "done";
			introProgress = 1;
		},

		tick,
	};

	return store;
}

export function setSceneStore(s: SceneStore) {
	setContext(SCENE_KEY, s);
}

export function getSceneStore(): SceneStore {
	return getContext<SceneStore>(SCENE_KEY);
}
