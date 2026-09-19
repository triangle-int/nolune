<script lang="ts">
	import { fetchMachines, type MachineInfo } from "$lib/api/client.js";

	let { slug, compact = false }: { slug: string; compact?: boolean } = $props();

	let machines = $state<MachineInfo[]>([]);
	let loading = $state(true);
	let loadError = $state("");
	let now = $state(Math.floor(Date.now() / 1000));

	async function load() {
		loadError = "";
		try {
			machines = (await fetchMachines(slug)).machines;
			now = Math.floor(Date.now() / 1000);
		} catch {
			loadError = "Could not check which computers are connected.";
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		slug;
		loading = true;
		load();
		const timer = setInterval(load, 15_000);
		return () => clearInterval(timer);
	});

	function osLabel(os: string): string {
		const key = os.toLowerCase();
		if (key.includes("mac") || key.includes("darwin")) return "macOS";
		if (key.includes("win")) return "Windows";
		if (key.includes("linux")) return "Linux";
		return os;
	}

	function lastSeen(unixSeconds: number): string {
		const delta = Math.max(0, now - unixSeconds);
		if (delta < 90) return "online now";
		if (delta < 3600) return `seen ${Math.round(delta / 60)} min ago`;
		if (delta < 86400 * 2) return `seen ${Math.round(delta / 3600)} h ago`;
		return `seen ${Math.round(delta / 86400)} days ago`;
	}
</script>

<div class="computers" class:computers-compact={compact}>
	{#if loading}
		<p role="status" class="computers-muted">Checking connected computers…</p>
	{:else if loadError}
		<div class="computers-error" role="alert">
			<p>{loadError}</p>
			<button class="nl-button-secondary" onclick={load}>Try again</button>
		</div>
	{:else if machines.length === 0}
		<div class="computers-empty">
			<p class="computers-empty-title">No computers connected</p>
			<p class="computers-muted">Install the Nolune desktop app on a computer and point it at this server. It appears here as soon as it connects.</p>
		</div>
	{:else}
		<ul class="computers-list">
			{#each machines as machine (machine.machine_id)}
				<li class="computers-row">
					<div class="computers-info">
						<span class="computers-name">{machine.hostname || machine.machine_id}</span>
						<span class="computers-meta">{osLabel(machine.os)} · {machine.screen_width}×{machine.screen_height}</span>
					</div>
					<span class="computers-seen" class:computers-online={now - machine.last_seen < 90}>{lastSeen(machine.last_seen)}</span>
				</li>
			{/each}
		</ul>
	{/if}
</div>

<style>
	.computers { display: flex; flex-direction: column; gap: 12px; }
	.computers-muted { font: 400 14px/1.6 var(--font-body); color: var(--text-secondary); margin: 0; }
	.computers-error { display: flex; flex-direction: column; align-items: flex-start; gap: 12px; color: var(--destructive); font: 400 14px/1.6 var(--font-body); }
	.computers-error p { margin: 0; }
	.computers-empty { display: flex; flex-direction: column; gap: 6px; padding: 16px; border: 1px dashed var(--border); border-radius: 12px; background: var(--card); }
	.computers-compact .computers-empty { padding: 12px; }
	.computers-empty-title { font: 500 15px var(--font-body); color: var(--foreground); margin: 0; }
	.computers-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
	.computers-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; padding: 12px 14px; border: 1px solid var(--border); border-radius: 12px; background: var(--card); }
	.computers-info { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
	.computers-name { font: 500 15px var(--font-body); color: var(--foreground); overflow-wrap: anywhere; }
	.computers-meta { font: 400 13px var(--font-body); color: var(--text-muted); }
	.computers-seen { font: 400 13px var(--font-body); color: var(--text-secondary); white-space: nowrap; }
	.computers-online { color: var(--primary); }
</style>
