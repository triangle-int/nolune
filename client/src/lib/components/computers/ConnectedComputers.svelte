<script lang="ts">
	// Connected Spaces (#80): every place the one companion can act. The
	// server home is the first row; desktops that ever connected through the
	// Nolune desktop app follow, online first, and a disconnected one stays
	// listed as offline. Rows update from `machine_updated` and
	// `machine_forgotten` events; routine heartbeats are silent on the
	// socket, so the listing is also refreshed by polling, and a 5 s clock
	// re-derives health and last-seen labels between refreshes. A poll that
	// was in flight when an event, a rename or a forget landed never reverts
	// that row: the response is folded in around what changed since it was
	// requested, and a response for a slug this component has left is dropped.
	import { fetchMachines, fetchMeta, forgetMachine, isDesktopRelay, renameMachine, type MachineInfo } from "$lib/api/client.js";
	import type { ServerEvent } from "$lib/api/types.js";
	import { applyMachineEvent, buildSpaces, homeSpace, reconcileListing } from "$lib/computers/spaces.js";
	import { getCompanion } from "$lib/stores/companion.svelte.js";
	import { getWebSocket } from "$lib/stores/websocket.svelte.js";
	import SpaceRow from "./SpaceRow.svelte";

	let { slug, compact = false }: { slug: string; compact?: boolean } = $props();

	const ws = getWebSocket();
	const companion = getCompanion();

	/** Routine heartbeats never reach the socket, so `last_seen` is refreshed this often. */
	const POLL_SECS = 15;
	const TICK_SECS = 5;

	let machines = $state<MachineInfo[]>([]);
	let loading = $state(true);
	let loadError = $state("");
	/** A refresh failed after the list was shown: keep the rows, say so, and stop the clock. */
	let refreshError = $state(false);
	let now = $state(nowSeconds());
	/** Client time of the last listing or event the rows reflect. */
	let freshAt = $state(nowSeconds());
	let version = $state("");

	/** Bumped on every local change to a row; a fetch captures it when it starts. */
	let generation = 0;
	/** The generation at which each row last changed locally (updated or forgotten). */
	const touched = new Map<string, number>();
	/** Bumped when the effect (re)starts and when it is torn down, so a stale fetch is dropped. */
	let epoch = 0;

	function nowSeconds(): number {
		return Math.floor(Date.now() / 1000);
	}

	/** Record one local change so an in-flight listing cannot revert it. */
	function applyLocally(event: Extract<ServerEvent, { type: "machine_updated" | "machine_forgotten" }>) {
		generation += 1;
		touched.set(event.type === "machine_updated" ? event.machine.machine_id : event.machine_id, generation);
		machines = applyMachineEvent(machines, event);
		now = nowSeconds();
		freshAt = now;
	}

	async function load() {
		const startedEpoch = epoch;
		const startedAt = generation;
		try {
			const listing = (await fetchMachines(slug)).machines;
			if (startedEpoch !== epoch) return;
			const changedSince = [...touched].filter(([, at]) => at > startedAt).map(([id]) => id);
			machines = reconcileListing(machines, listing, changedSince);
			now = nowSeconds();
			freshAt = now;
			loadError = "";
			refreshError = false;
		} catch {
			if (startedEpoch !== epoch) return;
			if (loading) loadError = "Could not check which computers are connected.";
			else refreshError = true;
		} finally {
			if (startedEpoch === epoch) loading = false;
		}
	}

	function retry() {
		loading = true;
		loadError = "";
		load();
	}

	$effect(() => {
		slug;
		epoch += 1;
		touched.clear();
		machines = [];
		loading = true;
		loadError = "";
		refreshError = false;
		load();
		fetchMeta()
			.then((meta) => (version = meta.version))
			.catch(() => {});
		const unsub = ws.subscribe((event: ServerEvent) => {
			if ((event.type === "machine_updated" || event.type === "machine_forgotten") && event.instance_slug === slug) {
				applyLocally(event);
			}
		});
		const tick = setInterval(() => (now = nowSeconds()), TICK_SECS * 1000);
		const poll = setInterval(load, POLL_SECS * 1000);
		// A backgrounded tab throttles both timers; catch up as soon as it is looked at.
		const onVisible = () => {
			if (document.visibilityState === "visible") load();
		};
		document.addEventListener("visibilitychange", onVisible);
		return () => {
			epoch += 1;
			unsub();
			clearInterval(tick);
			clearInterval(poll);
			document.removeEventListener("visibilitychange", onVisible);
		};
	});

	// While refreshes fail the clock freezes at the last good listing, so a
	// server this browser cannot reach never reads as every computer going quiet.
	const clock = $derived(refreshError ? freshAt : now);
	const home = $derived(
		homeSpace({
			connected: ws.connected,
			address: isDesktopRelay() || typeof location === "undefined" ? "" : location.host,
			version,
			companionName: companion.context?.companion_name ?? "",
			nowSeconds: now,
		}),
	);
	const spaces = $derived(buildSpaces(machines, clock, home, companion.context?.companion_name ?? ""));
	const desktops = $derived(spaces.filter((space) => space.kind === "desktop").length);

	function renamer(machineId: string): (name: string | null) => Promise<void> {
		return async (name) => {
			const updated = await renameMachine(slug, machineId, name);
			applyLocally({ type: "machine_updated", instance_slug: slug, machine: updated });
		};
	}

	// The server also broadcasts `machine_forgotten`; applying it twice is a no-op.
	// Its refusal for a computer that reconnected meanwhile (409 `machine_online`)
	// is read out of the JSON body, which `forgetMachine` passes through as text.
	function forgetter(machineId: string): () => Promise<void> {
		return async () => {
			try {
				await forgetMachine(slug, machineId);
			} catch (e) {
				const name = machines.find((m) => m.machine_id === machineId)?.display_name ?? "This computer";
				throw new Error(forgetRefusal(e, name));
			}
			applyLocally({ type: "machine_forgotten", instance_slug: slug, machine_id: machineId });
		};
	}

	function forgetRefusal(e: unknown, name: string): string {
		const text = e instanceof Error ? e.message : "";
		let code = "";
		try {
			code = String((JSON.parse(text) as { error?: string }).error ?? "");
		} catch {
			code = text;
		}
		if (code.includes("machine_online")) return `${name} is connected again, so it stays listed.`;
		if (code.includes("not_found")) return `${name} was already forgotten.`;
		return "Could not forget this computer. Try again in a moment.";
	}
</script>

<div class="computers" class:computers-compact={compact}>
	{#if loading}
		<p role="status" class="computers-muted">Checking connected computers…</p>
	{:else if loadError}
		<div class="computers-error" role="alert">
			<p>{loadError}</p>
			<button class="nl-button-secondary" onclick={retry}>Try again</button>
		</div>
	{:else}
		<ul class="computers-list">
			{#each spaces as space (space.id)}
				<SpaceRow
					{space}
					{compact}
					onrename={space.canRename ? renamer(space.id) : undefined}
					onforget={space.canForget ? forgetter(space.id) : undefined}
				/>
			{/each}
		</ul>
		{#if desktops === 0}
			<div class="computers-empty">
				<p class="computers-empty-title">No computers connected yet</p>
				<p class="computers-muted">Install the Nolune desktop app on a computer and point it at this server. It appears here as soon as it connects, and stays listed when it goes offline.</p>
			</div>
		{/if}
		{#if refreshError}
			<p class="computers-muted computers-stale" role="status">Could not refresh just now; showing what was known {Math.max(0, now - freshAt)} s ago.</p>
		{/if}
	{/if}
</div>

<style>
	.computers { display: flex; flex-direction: column; gap: 12px; min-width: 0; }
	.computers-muted { font: 400 14px/1.6 var(--font-body); color: var(--text-secondary); margin: 0; }
	.computers-stale { font-size: 13px; color: var(--text-muted); }
	.computers-error { display: flex; flex-direction: column; align-items: flex-start; gap: 12px; color: var(--destructive); font: 400 14px/1.6 var(--font-body); }
	.computers-error p { margin: 0; }
	.computers-empty { display: flex; flex-direction: column; gap: 6px; padding: 16px; border: 1px dashed var(--border); border-radius: 12px; background: var(--card); }
	.computers-compact .computers-empty { padding: 12px; }
	.computers-empty-title { font: 500 15px var(--font-body); color: var(--foreground); margin: 0; }
	.computers-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; min-width: 0; }
</style>
