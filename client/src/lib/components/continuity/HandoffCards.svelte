<script lang="ts">
	// The handoff cards offered right now (#82), above the activity trail.
	// Cards come from `GET /handoffs` and are replaced in place on
	// `handoff_updated`; the computers to pick from come from `GET /machines`
	// and follow `machine_updated` / `machine_forgotten`. "Continue here" is
	// the computer the user named before (kept in this browser), or the only
	// connected one; otherwise the card asks. A reconnect reloads both, so a
	// continuation the server closed while this browser was away (a restart)
	// shows its outcome instead of "continuing" forever.
	import { untrack } from "svelte";
	import {
		acceptHandoff,
		dismissHandoff,
		fetchHandoffs,
		fetchMachines,
		keepHandoff,
		previewHandoff,
	} from "$lib/api/client.js";
	import type { ContinuityRecordError, HandoffCard as Card, MachineInfo, ServerEvent } from "$lib/api/types.js";
	import { resolveHere, upsertCard } from "$lib/continuity/handoff.js";
	import { getWebSocket } from "$lib/stores/websocket.svelte.js";
	import HandoffCard from "./HandoffCard.svelte";

	let { slug }: { slug: string } = $props();

	const HERE_KEY = "nolune:handoff:here";
	const ws = getWebSocket();

	let cards = $state<Card[]>([]);
	let errors = $state<ContinuityRecordError[]>([]);
	let machines = $state<MachineInfo[]>([]);
	let loading = $state(true);
	let loadError = $state("");
	let remembered = $state<string | null>(null);
	let now = $state(Math.floor(Date.now() / 1000));

	const hereId = $derived(resolveHere(machines, remembered));

	function readRemembered(): string | null {
		try {
			return typeof localStorage === "undefined" ? null : localStorage.getItem(HERE_KEY);
		} catch {
			return null;
		}
	}

	function rememberHere(machineId: string) {
		remembered = machineId;
		try {
			localStorage.setItem(HERE_KEY, machineId);
		} catch {
			// A browser that refuses storage still gets this session's choice.
		}
	}

	async function load() {
		loading = true;
		loadError = "";
		try {
			const [listing, known] = await Promise.all([fetchHandoffs(slug), fetchMachines(slug)]);
			cards = [...listing.handoffs].sort((a, b) => b.updated_at - a.updated_at);
			errors = listing.errors;
			machines = known.machines;
		} catch {
			loadError = "Could not load unfinished tasks.";
		} finally {
			loading = false;
		}
	}

	let hadConnection = false;
	$effect(() => {
		const isConnected = ws.connected;
		untrack(() => {
			if (!isConnected) return;
			if (!hadConnection) {
				// The first connection: the load below covers it.
				hadConnection = true;
				return;
			}
			load();
		});
	});

	$effect(() => {
		remembered = readRemembered();
		load();
		const unsub = ws.subscribe((event: ServerEvent) => {
			if (!("instance_slug" in event) || event.instance_slug !== slug) return;
			if (event.type === "handoff_updated") {
				cards = upsertCard(cards, event.card);
			} else if (event.type === "machine_updated") {
				const rest = machines.filter((m) => m.machine_id !== event.machine.machine_id);
				machines = [...rest, event.machine];
			} else if (event.type === "machine_forgotten") {
				machines = machines.filter((m) => m.machine_id !== event.machine_id);
			}
		});
		const tick = setInterval(() => (now = Math.floor(Date.now() / 1000)), 30_000);
		return () => {
			unsub();
			clearInterval(tick);
		};
	});

	function updated(card: Card) {
		cards = upsertCard(cards, card);
	}
</script>

{#if loading}
	<p class="handoffs-status" role="status">Checking for unfinished tasks…</p>
{:else if loadError}
	<div class="handoffs-error" role="alert">
		<p>{loadError}</p>
		<button class="nl-button-secondary" onclick={load}>Try again</button>
	</div>
{:else if cards.length > 0 || errors.length > 0}
	<section class="handoffs" aria-labelledby="handoffs-title">
		<h3 id="handoffs-title">Unfinished tasks</h3>
		<p class="handoffs-lead">Review a task from any of your computers and choose where to continue it. Nothing continues until you say so.</p>
		{#if errors.length > 0}
			<p class="handoffs-note" role="alert">
				{errors.length === 1 ? "One task file could not be read" : `${errors.length} task files could not be read`} and {errors.length === 1 ? "is" : "are"} left untouched: {errors.map((e) => e.file).join(", ")}.
			</p>
		{/if}
		<ul class="handoffs-list">
			{#each cards as card (card.record_id)}
				<li>
					<HandoffCard
						{card}
						{machines}
						{hereId}
						{now}
						onpreview={(machineId) => previewHandoff(slug, card.record_id, machineId)}
						onaccept={(machineId) => acceptHandoff(slug, card.record_id, machineId)}
						onkeep={() => keepHandoff(slug, card.record_id)}
						ondismiss={() => dismissHandoff(slug, card.record_id)}
						onupdated={updated}
						onhere={rememberHere}
					/>
				</li>
			{/each}
		</ul>
	</section>
{/if}

<style>
	.handoffs { max-width: 720px; margin: 0 auto 24px; display: flex; flex-direction: column; gap: 12px; }
	.handoffs h3 { margin: 0; font: 400 22px/1.2 var(--font-display); color: var(--foreground); }
	.handoffs-lead, .handoffs-note, .handoffs-status { margin: 0; font: 400 14px/1.6 var(--font-body); color: var(--text-secondary); }
	.handoffs-status { max-width: 720px; margin: 0 auto 16px; }
	.handoffs-note { color: var(--destructive); }
	.handoffs-error { max-width: 720px; margin: 0 auto 24px; display: flex; flex-wrap: wrap; align-items: center; gap: 12px; color: var(--text-secondary); font: 400 14px/1.6 var(--font-body); }
	.handoffs-error p { margin: 0; }
	.handoffs-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 12px; }
</style>
