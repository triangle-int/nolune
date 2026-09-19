<script lang="ts">
	import { page } from "$app/state";
	import { exportInstance, importInstance } from "$lib/api/client.js";

	// Data ownership (#98): everything here belongs to the one companion and
	// can leave with you as one archive.
	const slug = $derived(page.params.slug!);

	let exporting = $state(false);
	let exportBytes = $state(0);
	let exportError = $state("");
	let importing = $state(false);
	let importError = $state("");
	let importDone = $state(false);
	let importFileInput: HTMLInputElement | undefined = $state();

	function formatBytes(bytes: number): string {
		if (bytes < 1024) return `${bytes} B`;
		if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
		return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	}

	async function handleExport() {
		exporting = true;
		exportBytes = 0;
		exportError = "";
		try {
			const blob = await exportInstance(slug, (bytes) => { exportBytes = bytes; });
			const url = URL.createObjectURL(blob);
			const a = document.createElement("a");
			a.href = url;
			a.download = `${slug}.tar.gz`;
			a.click();
			URL.revokeObjectURL(url);
		} catch (e) {
			exportError = e instanceof Error ? e.message : "export failed";
		} finally {
			exporting = false;
		}
	}

	async function handleImport() {
		const file = importFileInput?.files?.[0];
		if (!file) return;
		importing = true;
		importError = "";
		importDone = false;
		try {
			await importInstance(slug, file);
			importDone = true;
			setTimeout(() => { importDone = false; }, 4000);
		} catch (e) {
			importError = e instanceof Error ? e.message : "import failed";
		} finally {
			importing = false;
			if (importFileInput) importFileInput.value = "";
		}
	}
</script>

<!-- What Nolune keeps -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">What your companion keeps</h3>
			<p class="section-desc">Everything lives on this server, in plain files you own. Nothing is sent anywhere except to the AI provider you chose.</p>
		</div>
	</div>
	<div class="section-body">
		<div class="setting-row">
			<span class="setting-label">Memory</span>
			<p class="setting-hint">Notes it wrote about you and your conversations. Inspect, correct, or forget any of them.</p>
			<div class="settings-links"><a class="nl-button-secondary" href={`/${slug}/memory`}>Open memory</a></div>
	</div>
	<div class="setting-row">
		<span class="setting-label">Activity and drops</span>
		<p class="setting-hint">Receipts for everything it did on its own, and the things it made along the way.</p>
		<div class="settings-links"><a class="nl-button-secondary" href={`/${slug}/activity`}>Open activity</a><a class="nl-button-secondary" href={`/${slug}/drops`}>Open drops</a></div>
	</div>
	<div class="setting-row">
		<span class="setting-label">Conversations</span>
		<p class="setting-hint">Chat history stays here too, and is part of every export.</p>
	</div>
	</div>
</section>

<!-- Export / Import -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Take it with you</h3>
			<p class="section-desc">One archive holds your companion’s personality, memory, drops, and chat history.</p>
		</div>
	</div>
	<div class="section-body">
		<div class="data-actions">
			<button class="data-btn" onclick={handleExport} disabled={exporting}>
				{#if exporting}
					Exporting… {formatBytes(exportBytes)}
				{:else}
					Export
				{/if}
			</button>
			<button type="button" class="data-btn data-btn-import" onclick={() => importFileInput?.click()} disabled={importing}>
				{#if importing}
					Importing...
				{:else if importDone}
					Imported!
				{:else}
					Import
				{/if}
			</button>
			<input
				type="file"
				accept=".tar.gz,.tgz"
				bind:this={importFileInput}
				onchange={handleImport}
				hidden
				disabled={importing}
			/>
	</div>
	<p class="data-hint">Export downloads a .tar.gz of your companion’s data. Import merges an archive into your companion.</p>

	{#if importError}
		<p class="error-msg" role="alert">{importError}</p>
	{/if}
	{#if exportError}
		<p class="error-msg" role="alert">{exportError}</p>
	{/if}
	</div>
</section>
