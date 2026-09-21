<script lang="ts">
	import { page } from "$app/state";
	import * as AlertDialog from "$lib/components/ui/alert-dialog/index.js";
	import { exportInstance, importInstance, ImportError, type ImportOutcome } from "$lib/api/client.js";
	import { formatBytes, importStatusText } from "$lib/settings/import-status.js";

	// Data ownership (#98): everything here belongs to the one companion and
	// can leave with you as one archive.
	const slug = $derived(page.params.slug!);

	let exporting = $state(false);
	let exportBytes = $state(0);
	let exportError = $state("");
	let importFileInput: HTMLInputElement | undefined = $state();
	// Import (#74) is a replacement, so it asks once, in the destructive
	// confirmation dialog the design system prescribes, before anything
	// leaves the browser: the chosen file waits here until Replace or Keep.
	let pendingFile: File | null = $state(null);
	let confirmOpen = $state(false);
	let importing = $state(false);
	let importSent = $state(0);
	let importTotal = $state(0);
	let importUploaded = $state(false);
	let importResult: ImportOutcome | null = $state(null);
	let importError = $state("");

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

	function handleImport() {
		const file = importFileInput?.files?.[0];
		if (!file) return;
		importResult = null;
		importError = "";
		pendingFile = file;
		confirmOpen = true;
	}

	// Keep current, Escape, or a click outside: the file is forgotten and
	// focus returns to the Import button through the dialog.
	function cancelImport() {
		confirmOpen = false;
		pendingFile = null;
		if (importFileInput) importFileInput.value = "";
	}

	// The dialog closes itself on Escape and outside clicks; a close that
	// did not start the import keeps the current data.
	$effect(() => {
		if (!confirmOpen && pendingFile && !importing) cancelImport();
	});

	async function confirmImport() {
		const file = pendingFile;
		if (!file || importing) return;
		confirmOpen = false;
		pendingFile = null;
		importing = true;
		importSent = 0;
		importTotal = 0;
		importUploaded = false;
		importError = "";
		try {
			importResult = await importInstance(slug, file, (sent, total, uploaded) => {
				importSent = sent;
				importTotal = total;
				importUploaded = uploaded;
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

	// Uploading until the browser says the body is sent (an empty file has
	// nothing to count), then Restoring while the server works.
	const importProgress = $derived(
		importStatusText({ importing, sent: importSent, total: importTotal, uploaded: importUploaded }),
	);
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

	<AlertDialog.Root bind:open={confirmOpen}>
		<AlertDialog.Content class="data-dialog">
			<AlertDialog.Header>
				<AlertDialog.Title class="data-dialog-title">Replace your companion?</AlertDialog.Title>
				<AlertDialog.Description class="data-dialog-desc">
					{#if pendingFile}
						<span class="data-dialog-file">{pendingFile.name} ({formatBytes(pendingFile.size)})</span> replaces everything your companion keeps now: its memory, personality, drops, and chat history become the archive’s, and the current data is not kept.
					{/if}
				</AlertDialog.Description>
			</AlertDialog.Header>
			<AlertDialog.Footer class="data-dialog-footer">
				<AlertDialog.Cancel class="data-dialog-btn data-dialog-cancel" onclick={cancelImport}>Keep current</AlertDialog.Cancel>
				<AlertDialog.Action class="data-dialog-btn data-dialog-confirm" onclick={confirmImport}>Replace</AlertDialog.Action>
			</AlertDialog.Footer>
		</AlertDialog.Content>
	</AlertDialog.Root>
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

<style>
	/* The replace confirmation is the shadcn AlertDialog from the design
	   system, portaled outside .settings-page, so its skin lives here: card
	   surface, Fraunces-free body type, destructive primary action. */
	:global(.data-dialog) {
		background: var(--card) !important;
		border: 1px solid var(--border) !important;
		border-radius: 16px !important;
		padding: 1.5rem !important;
		box-shadow: none;
		max-width: min(28rem, calc(100% - 2rem));
	}
	:global(.data-dialog-title) {
		font-family: var(--font-body);
		font-size: 18px;
		letter-spacing: 0;
		color: var(--foreground);
		margin: 0;
	}
	:global(.data-dialog-desc) {
		font-family: var(--font-body);
		font-size: 14px;
		line-height: 1.5;
		color: var(--text-secondary);
		margin-top: 0.25rem;
		overflow-wrap: anywhere;
	}
	:global(.data-dialog-file) {
		color: var(--foreground);
	}
	:global(.data-dialog-footer) {
		display: flex;
		flex-wrap: wrap;
		justify-content: flex-end;
		gap: 0.5rem;
		margin-top: 1.25rem;
	}
	:global(.data-dialog-btn) {
		font-family: var(--font-body);
		font-size: 14px;
		letter-spacing: 0;
		min-height: 44px;
		padding: 0.4rem 1rem;
		border-radius: 8px;
		cursor: pointer;
		transition: all 0.2s ease;
	}
	:global(.data-dialog-cancel),
	:global(.data-dialog-cancel:hover) {
		color: var(--foreground);
		background: var(--popover);
		border: 1px solid var(--border);
	}
	:global(.data-dialog-cancel:hover) {
		background: var(--accent);
	}
	:global(.data-dialog-confirm) {
		color: var(--primary-foreground);
		background: var(--destructive);
		border: 1px solid var(--destructive);
	}
	:global(.data-dialog-confirm:hover) {
		background: var(--destructive);
		filter: brightness(0.95);
	}
</style>
