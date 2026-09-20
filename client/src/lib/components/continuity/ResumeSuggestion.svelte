<script lang="ts">
	// The ritual's one current suggestion (#83), shown wherever the user
	// lands. It tells the server that Nolune was opened (once on mount, and
	// when the tab comes back into view), so a break can be measured; the
	// server decides whether that is a trigger. The offer comes from
	// `GET /resume` and follows `resume_updated`; a `handoff_updated` for
	// the same record resolves it once the card is decided.
	import { untrack } from "svelte";
	import { dismissResume, fetchResume, refuseResume, resumeOpened, snoozeResume } from "$lib/api/client.js";
	import type { ResumeOffer, ServerEvent } from "$lib/api/types.js";
	import { applyResumeEvent } from "$lib/continuity/resume.js";
	import { getWebSocket } from "$lib/stores/websocket.svelte.js";
	import ResumeBanner from "./ResumeBanner.svelte";

	let { slug }: { slug: string } = $props();

	const ws = getWebSocket();
	let offer = $state<ResumeOffer | null>(null);
	let now = $state(Math.floor(Date.now() / 1000));

	function fold(event: ServerEvent) {
		if (!("instance_slug" in event) || event.instance_slug !== slug) return;
		const next = applyResumeEvent(offer, event);
		if (next !== undefined) offer = next as ResumeOffer | null;
	}

	async function load() {
		try {
			offer = (await fetchResume(slug)).suggestion;
		} catch {
			// The banner is optional: a failed read shows nothing rather than an error.
		}
	}

	async function opened() {
		try {
			const outcome = await resumeOpened(slug);
			if (outcome.suggestion) offer = outcome.suggestion;
		} catch {
			// Not a trigger this time; the next open reports again.
		}
	}

	let hadConnection = false;
	$effect(() => {
		const isConnected = ws.connected;
		untrack(() => {
			if (!isConnected) return;
			if (!hadConnection) {
				hadConnection = true;
				return;
			}
			load();
		});
	});

	$effect(() => {
		slug;
		untrack(() => {
			offer = null;
			load().then(opened);
		});
		const onVisible = () => {
			if (document.visibilityState === "visible") opened();
		};
		document.addEventListener("visibilitychange", onVisible);
		const unsub = ws.subscribe(fold);
		const tick = setInterval(() => (now = Math.floor(Date.now() / 1000)), 30_000);
		return () => {
			document.removeEventListener("visibilitychange", onVisible);
			unsub();
			clearInterval(tick);
		};
	});

	async function refuse() {
		await refuseResume(slug);
		offer = null;
	}

	async function snooze(until: number) {
		await snoozeResume(slug, until);
		offer = null;
	}

	async function dismiss() {
		if (!offer) return;
		await dismissResume(slug, offer.record_id);
		offer = null;
	}
</script>

{#if offer}
	<div class="resume-slot">
		<ResumeBanner {offer} {now} reviewHref={`/${slug}/activity#handoff-${offer.record_id}`} onrefuse={refuse} onsnooze={snooze} ondismiss={dismiss} />
	</div>
{/if}

<style>
	.resume-slot { flex-shrink: 0; padding: 12px 24px 0; max-width: 100%; }
	.resume-slot > :global(.resume) { max-width: 720px; margin: 0 auto; }
	@media (max-width: 720px) { .resume-slot { padding: 12px 16px 0; } }
</style>
