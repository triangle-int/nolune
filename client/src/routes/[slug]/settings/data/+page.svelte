<script lang="ts">
	import { page } from "$app/state";
	import { tick } from "svelte";
	import { exportInstance, importInstance, ImportError, type ImportOutcome } from "$lib/api/client.js";

	// Data ownership (#98): everything here belongs to the one companion and
	// can leave with you as one archive.
	const slug = $derived(page.params.slug!);

	let exporting = $state(false);
	let exportBytes = $state(0);
	let exportError = $state("");
	let importFileInput: HTMLInputElement | undefined = $state();
	// Import (#74) is a replacement, so it asks once, inline, before anything
	// leaves the browser: the chosen file waits here until Replace or Keep.
	let pendingFile: File | null = $state(null);
	let confirmButton: HTMLButtonElement | undefined = $state();
	let importing = $state(false);
	let importSent = $state(0);
	let importTotal = $state(0);
	let importResult: ImportOutcome | null = $state(null);
	let importError = $state("");

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
		importResult = null;
		importError = "";
		pendingFile = file;
		await tick();
		confirmButton?.focus();
	}

	function cancelImport() {
		pendingFile = null;
		if (importFileInput) importFileInput.value = "";
	}

	async function confirmImport() {
		const file = pendingFile;
		if (!file || importing) return;
		pendingFile = null;
		importing = true;
		importSent = 0;
		importTotal = file.size;
		importError = "";
		try {
			importResult = await importInstance(slug, file, (sent, total) => {
				importSent = sent;
				importTotal = total;
			});
		} catch (e) {
			if (e instanceof ImportError && e.code === "companion_busy") {
				importError = `Your companion is busy right now, so nothing was changed. Wait for it to finish, then try again. (${e.message})`;
			} else {
				importError = e instanceof Error ? `Import failed and nothing was changed: ${e.message}` : "import failed";
			}
		} finally {
			importing = false;
			if (importFileInput) importFileInput.value = "";
		}
	}

	const importProgress = $derived.by(() => {
		if (!importing) return "";
		if (importTotal > 0 && importSent < importTotal) {
			return `Uploading… ${formatBytes(importSent)} of ${formatBytes(importTotal)}`;
		}
		return "Restoring… the server is checking the archive and rebuilding the search index. This can take a while for a large archive.";
	});
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
			<button type="button" class="data-btn data-btn-import" onclick={() => importFileInput?.click()} disabled={importing || pendingFile !== null}>
				{#if importing}
					Importing…
				{:else}
					Import…
				{/if}
			</button>
			<input
				type="file"
				accept=".tar.gz,.tgz,application/gzip"
				bind:this={importFileInput}
				onchange={handleImport}
				hidden
				disabled={importing}
			/>
	</div>
	<p class="data-hint">Export downloads a .tar.gz of your companion’s data. Import replaces your companion with an archive: afterwards its memory, personality, drops, and chat history are the archive’s, and what it keeps now is not kept. Export first if you want to hold on to it.</p>

	{#if pendingFile}
		<div class="data-confirm" role="group" aria-labelledby="import-confirm-label">
			<p id="import-confirm-label" class="data-confirm-text">Replace your companion with {pendingFile.name} ({formatBytes(pendingFile.size)})? Everything it keeps now is replaced by the archive and not kept.</p>
			<div class="data-confirm-row">
				<button bind:this={confirmButton} class="nl-button-secondary data-confirm-btn" type="button" onclick={confirmImport} onkeydown={(e) => { if (e.key === "Escape") cancelImport(); }}>Replace</button>
				<button class="nl-button-secondary" type="button" onclick={cancelImport} onkeydown={(e) => { if (e.key === "Escape") cancelImport(); }}>Keep current</button>
			</div>
		</div>
	{/if}
	{#if importing}
		<p class="data-status" role="status" aria-live="polite">{importProgress}</p>
	{:else if importResult}
		<p class="data-status" role="status">
			Restored {importResult.files} files ({formatBytes(importResult.bytes)}).
			{#if importResult.derived_index === "rebuilt"}
				Search index rebuilt.
			{:else}
				Search index pending: it is rebuilt the next time the server starts{#if importResult.pending_reason}&nbsp;({importResult.pending_reason}){/if}.
			{/if}
		</p>
	{/if}

	{#if importError}
		<p class="error-msg" role="alert">{importError}</p>
	{/if}
	{#if exportError}
		<p class="error-msg" role="alert">{exportError}</p>
	{/if}
	</div>
</section>
