<script lang="ts">
	// Connected Spaces (#80): every place the one companion can act. The
	// server home is the first row; desktops that ever connected through the
	// Nolune desktop app follow, online first, and a disconnected one stays
	// listed as offline. Rows update from `machine_updated` and
	// `machine_forgotten` events; routine heartbeats are silent on the
	// socket, so the listing is also refreshed by polling, and a 5 s clock
	// re-derives health and last-seen labels between refreshes.
	import { fetchMachines, fetchMeta, isDesktopRelay, renameMachine, type MachineInfo } from "$lib/api/client.js";
	import type { ServerEvent } from "$lib/api/types.js";
	import { applyMachineEvent, buildSpaces, homeSpace } from "$lib/computers/spaces.js";
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

	function nowSeconds(): number {
		return Math.floor(Date.now() / 1000);
	}

	async function load() {
		try {
			machines = (await fetchMachines(slug)).machines;
			now = nowSeconds();
			freshAt = now;
			loadError = "";
			refreshError = false;
		} catch {
			if (loading) loadError = "Could not check which computers are connected.";
			else refreshError = true;
		} finally {
			loading = false;
		}
	}

	function retry() {
		loading = true;
		loadError = "";
		load();
	}

	$effect(() => {
		slug;
		loading = true;
		load();
		fetchMeta()
			.then((meta) => (version = meta.version))
			.catch(() => {});
		const unsub = ws.subscribe((event: ServerEvent) => {
			if ((event.type === "machine_updated" || event.type === "machine_forgotten") && event.instance_slug === slug) {
				machines = applyMachineEvent(machines, event);
				now = nowSeconds();
				freshAt = now;
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
	const spaces = $derived(buildSpaces(machines, clock, home));
	const desktops = $derived(spaces.filter((space) => space.kind === "desktop").length);

	function renamer(machineId: string): (name: string | null) => Promise<void> {
		return async (name) => {
			const updated = await renameMachine(slug, machineId, name);
			machines = applyMachineEvent(machines, { type: "machine_updated", instance_slug: slug, machine: updated });
		};
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
				<SpaceRow {space} {compact} onrename={space.canRename ? renamer(space.id) : undefined} />
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
