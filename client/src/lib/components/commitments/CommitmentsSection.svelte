<script lang="ts">
	import {
		CommitmentError,
		cancelCommitment,
		completeCommitment,
		fetchCommitments,
		fetchCompanionName,
		snoozeCommitment,
		updateCommitment,
	} from "$lib/api/client.js";
	import type { Commitment, CommitmentDeadline, CommitmentPatch, ServerEvent } from "$lib/api/types.js";
	import {
		MAX_EVIDENCE_CHARS,
		MAX_PROMISE_CHARS,
		canChange,
		completionEvidence,
		dueLabel,
		isOpen,
		isOverdue,
		lastCheckLabel,
		localDateTimeValue,
		nextCheckLabel,
		ownerLabel,
		parseLocalDateTime,
		provenanceLabel,
		refusalMessage,
		snoozeLabel,
		snoozePresets,
		sortOpen,
		statusLabel,
		upsertCommitment,
		validateSnooze,
		waitLabel,
	} from "$lib/commitments/commitments.js";
	import { getWebSocket } from "$lib/stores/websocket.svelte.js";
	import { getToasts } from "$lib/stores/toast.svelte.js";

	// Promises the companion tracks as first-class state (#85). The server's
	// record is the only source; every control here answers with the record it
	// wrote, and completion is never silent: the user confirms or gives evidence.
	let { slug, now }: { slug: string; now: number } = $props();

	const toast = getToasts();
	const ws = getWebSocket();

	let commitments = $state<Commitment[]>([]);
	let filter = $state<"open" | "closed">("open");
	let loading = $state(true);
	let loadError = $state("");
	let companionName = $state("");
	let busy = $state<string | null>(null);

	type Panel = "details" | "edit" | "snooze" | "complete" | "cancel";
	let open = $state<{ id: string; panel: Panel } | null>(null);
	let formError = $state("");
	// Edit form
	let editPromise = $state("");
	let editDeadlineKind = $state<"none" | "at" | "window">("none");
	let editAt = $state("");
	let editStart = $state("");
	let editEnd = $state("");
	// Snooze form
	let snoozeCustom = $state("");
	// Complete form
	let confirmDone = $state(false);
	let evidence = $state("");

	const visible = $derived(
		filter === "open" ? sortOpen(commitments, now) : commitments.filter((c) => !isOpen(c.status)),
	);
	const openCount = $derived(commitments.filter((c) => isOpen(c.status)).length);
	const closedCount = $derived(commitments.length - openCount);
	const evidenceCheck = $derived(completionEvidence({ confirmed: confirmDone, summary: evidence }));

	async function load() {
		loading = true;
		loadError = "";
		try {
			commitments = await fetchCommitments(slug, "all");
		} catch {
			loadError = "Could not load commitments. Please try again.";
		} finally {
			loading = false;
		}
	}

	$effect(() => {
		load();
		fetchCompanionName(slug)
			.then((res) => { if (res.name) companionName = res.name; })
			.catch(() => {});
		const unsub = ws.subscribe((event: ServerEvent) => {
			if (event.type === "commitment_updated" && event.instance_slug === slug) {
				commitments = upsertCommitment(commitments, event.commitment);
			}
		});
		return unsub;
	});

	function isOpenPanel(c: Commitment, panel: Panel) {
		return open?.id === c.id && open.panel === panel;
	}

	function toggle(c: Commitment, panel: Panel) {
		formError = "";
		if (isOpenPanel(c, panel)) {
			open = null;
			return;
		}
		if (panel === "edit") {
			editPromise = c.promise;
			const d = c.deadline ?? null;
			editDeadlineKind = d ? d.kind : "none";
			editAt = d?.kind === "at" ? localDateTimeValue(d.at) : "";
			editStart = d?.kind === "window" ? localDateTimeValue(d.start) : "";
			editEnd = d?.kind === "window" ? localDateTimeValue(d.end) : "";
		} else if (panel === "snooze") {
			snoozeCustom = "";
		} else if (panel === "complete") {
			confirmDone = false;
			evidence = "";
		}
		open = { id: c.id, panel };
	}

	function explain(error: unknown, fallback: string): string {
		return error instanceof CommitmentError ? refusalMessage(error.body) : fallback;
	}

	async function apply(c: Commitment, action: () => Promise<Commitment>, fallback: string) {
		busy = c.id;
		formError = "";
		try {
			const next = await action();
			commitments = upsertCommitment(commitments, next);
			open = null;
		} catch (error) {
			formError = explain(error, fallback);
		} finally {
			busy = null;
		}
	}

	function editPatch(c: Commitment): CommitmentPatch | string {
		const patch: CommitmentPatch = {};
		const promise = editPromise.trim();
		if (promise === "") return "The promise cannot be empty.";
		if (promise.length > MAX_PROMISE_CHARS) return `Keep the promise under ${MAX_PROMISE_CHARS} characters.`;
		if (promise !== c.promise) patch.promise = promise;
		let deadline: CommitmentDeadline | null = null;
		if (editDeadlineKind === "at") {
			const at = parseLocalDateTime(editAt);
			if (at === null) return "Pick when it is due.";
			deadline = { kind: "at", at };
		} else if (editDeadlineKind === "window") {
			const start = parseLocalDateTime(editStart);
			const end = parseLocalDateTime(editEnd);
			if (start === null || end === null) return "Pick both ends of the window.";
			if (end <= start) return "The window must end after it starts.";
			deadline = { kind: "window", start, end };
		}
		const before = JSON.stringify(c.deadline ?? null);
		if (JSON.stringify(deadline) !== before) {
			if (deadline) patch.deadline = deadline;
			else patch.clear_deadline = true;
		}
		return patch;
	}

	function saveEdit(c: Commitment) {
		const patch = editPatch(c);
		if (typeof patch === "string") {
			formError = patch;
			return;
		}
		if (Object.keys(patch).length === 0) {
			open = null;
			return;
		}
		return apply(c, () => updateCommitment(slug, c.id, patch), "Could not save the change.");
	}

	function snooze(c: Commitment, until: number | null) {
		const check = validateSnooze(until, now);
		if (!check.ok) {
			formError = check.reason;
			return;
		}
		return apply(c, () => snoozeCommitment(slug, c.id, check.until), "Could not snooze that.");
	}

	function complete(c: Commitment) {
		const check = completionEvidence({ confirmed: confirmDone, summary: evidence });
		if (!check.ok) {
			formError = check.reason;
			return;
		}
		return apply(c, () => completeCommitment(slug, c.id, check.evidence), "Could not mark that complete.");
	}

	function cancel(c: Commitment) {
		return apply(c, () => cancelCommitment(slug, c.id), "Could not cancel that.");
	}

	function facts(c: Commitment): string {
		return [dueLabel(c, now), waitLabel(c, now), snoozeLabel(c, now), nextCheckLabel(c, now)].filter(Boolean).join(" · ");
	}

	function completionNote(c: Commitment): string {
		const done = c.completion;
		if (!done) return "";
		const parts = [];
		if (done.confirmed_by_user) parts.push("confirmed by you");
		if (done.summary) parts.push(done.summary);
		if (done.run_id) parts.push("shown by a check-in");
		return parts.length > 0 ? `Done: ${parts.join(", ")}` : "";
	}

	function formatMoment(unixSeconds: number): string {
		return new Date(unixSeconds * 1000).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
	}
</script>

<section class="cmt-section" aria-labelledby="commitments-heading">
	<div class="cmt-head">
		<div>
			<h3 id="commitments-heading">Commitments</h3>
			<p>Promises your companion is following through on. It looks at each one when its condition comes up, and every check-in below says what changed and why then.</p>
		</div>
		<div class="cmt-filter" role="group" aria-label="Show commitments">
			<button class="cmt-filter-btn" class:cmt-filter-active={filter === "open"} aria-pressed={filter === "open"} onclick={() => (filter = "open")}>Open{openCount > 0 ? ` · ${openCount}` : ""}</button>
			<button class="cmt-filter-btn" class:cmt-filter-active={filter === "closed"} aria-pressed={filter === "closed"} onclick={() => (filter = "closed")}>History{closedCount > 0 ? ` · ${closedCount}` : ""}</button>
		</div>
	</div>

	{#if loading}
		<p role="status" class="cmt-empty">Loading commitments…</p>
	{:else if loadError}
		<div class="cmt-empty" role="alert"><p>{loadError}</p><button class="nl-button-secondary" onclick={load}>Try again</button></div>
	{:else if visible.length === 0}
		<p class="cmt-empty">{filter === "open" ? "No open commitments. Ask your companion to hold you to something, or it will note its own promises here." : "Nothing completed or cancelled yet."}</p>
	{:else}
		<ul class="cmt-list">
			{#each visible as c (c.id)}
				{@const overdue = isOverdue(c, now)}
				{@const changeable = canChange(c)}
				<li id={`commitment-${c.id}`} class="cmt-item" class:cmt-overdue={overdue} class:cmt-closed={!changeable}>
					<div class="cmt-row">
						<span class="cmt-status">{overdue ? "Due" : statusLabel(c.status)}</span>
						<span class="cmt-meta">{ownerLabel(c.owner, companionName || "Nolune")} · {provenanceLabel(c.provenance)}</span>
					</div>
					<p class="cmt-promise">{c.promise}</p>
					{#if facts(c)}<p class="cmt-facts">{facts(c)}</p>{/if}
					{#if completionNote(c)}<p class="cmt-facts">{completionNote(c)}</p>{/if}
					<p class="cmt-check">
						{lastCheckLabel(c.last_check, now)}{#if c.last_check?.run_id}<span aria-hidden="true"> · </span><a class="cmt-link" href={`#run-${c.last_check.run_id}`}>View the check-in</a>{/if}
					</p>

					<div class="cmt-controls">
						<button class="cmt-btn" aria-expanded={isOpenPanel(c, "details")} onclick={() => toggle(c, "details")}>Details</button>
						{#if changeable}
							<button class="cmt-btn" aria-expanded={isOpenPanel(c, "edit")} disabled={busy === c.id} onclick={() => toggle(c, "edit")}>Edit</button>
							<button class="cmt-btn" aria-expanded={isOpenPanel(c, "snooze")} disabled={busy === c.id} onclick={() => toggle(c, "snooze")}>Snooze</button>
							<button class="cmt-btn" aria-expanded={isOpenPanel(c, "complete")} disabled={busy === c.id} onclick={() => toggle(c, "complete")}>Complete</button>
							<button class="cmt-btn" aria-expanded={isOpenPanel(c, "cancel")} disabled={busy === c.id} onclick={() => toggle(c, "cancel")}>Cancel</button>
						{/if}
					</div>

					{#if isOpenPanel(c, "details")}
						<dl class="cmt-details">
							<dt>Id</dt><dd><code>{c.id}</code></dd>
							<dt>Created</dt><dd>{formatMoment(c.created_at)}</dd>
							<dt>Status since</dt><dd>{formatMoment(c.status_changed_at)}</dd>
							{#if c.deadline}<dt>Deadline</dt><dd>{c.deadline.kind === "at" ? formatMoment(c.deadline.at) : `${formatMoment(c.deadline.start)} to ${formatMoment(c.deadline.end)}`}</dd>{/if}
							{#if c.next_check}<dt>Next check</dt><dd>{formatMoment(c.next_check)}</dd>{/if}
							{#if c.snoozed_until}<dt>Snoozed until</dt><dd>{formatMoment(c.snoozed_until)}</dd>{/if}
							{#if c.dependencies.length > 0}<dt>Depends on</dt><dd>{#each c.dependencies as dep (dep)}<a class="cmt-link" href={`#commitment-${dep}`}><code>{dep}</code></a> {/each}</dd>{/if}
							{#if c.continuity_ids.length > 0}<dt>Linked tasks</dt><dd>{#each c.continuity_ids as id (id)}<code>{id}</code> {/each}</dd>{/if}
							{#if c.last_check?.outcome.kind === "failed"}<dt>Last error</dt><dd class="cmt-error">{c.last_check.outcome.error}</dd>{/if}
						</dl>
					{:else if isOpenPanel(c, "edit")}
						<form class="cmt-form" onsubmit={(e) => { e.preventDefault(); saveEdit(c); }}>
							<label class="cmt-label" for={`promise-${c.id}`}>Promise</label>
							<textarea id={`promise-${c.id}`} class="nl-input cmt-textarea" rows="2" maxlength={MAX_PROMISE_CHARS} bind:value={editPromise}></textarea>
							<label class="cmt-label" for={`deadline-kind-${c.id}`}>Due</label>
							<select id={`deadline-kind-${c.id}`} class="nl-input" bind:value={editDeadlineKind}>
								<option value="none">No deadline</option>
								<option value="at">At a time</option>
								<option value="window">Between two times</option>
							</select>
							{#if editDeadlineKind === "at"}
								<label class="cmt-label" for={`deadline-at-${c.id}`}>When</label>
								<input id={`deadline-at-${c.id}`} class="nl-input" type="datetime-local" bind:value={editAt} />
							{:else if editDeadlineKind === "window"}
								<label class="cmt-label" for={`deadline-start-${c.id}`}>From</label>
								<input id={`deadline-start-${c.id}`} class="nl-input" type="datetime-local" bind:value={editStart} />
								<label class="cmt-label" for={`deadline-end-${c.id}`}>Until</label>
								<input id={`deadline-end-${c.id}`} class="nl-input" type="datetime-local" bind:value={editEnd} />
							{/if}
							{#if formError}<p class="cmt-error" role="alert">{formError}</p>{/if}
							<div class="cmt-form-actions">
								<button type="submit" class="nl-button" disabled={busy === c.id}>Save</button>
								<button type="button" class="nl-button-secondary" onclick={() => (open = null)}>Keep as is</button>
							</div>
						</form>
					{:else if isOpenPanel(c, "snooze")}
						<form class="cmt-form" onsubmit={(e) => { e.preventDefault(); snooze(c, parseLocalDateTime(snoozeCustom)); }}>
							<p class="cmt-label">Not before</p>
							<div class="cmt-presets">
								{#each snoozePresets(now) as preset (preset.label)}
									<button type="button" class="nl-button-secondary" disabled={busy === c.id} onclick={() => snooze(c, preset.until)}>{preset.label}</button>
								{/each}
							</div>
							<label class="cmt-label" for={`snooze-until-${c.id}`}>Or pick a time</label>
							<input id={`snooze-until-${c.id}`} class="nl-input" type="datetime-local" bind:value={snoozeCustom} />
							{#if formError}<p class="cmt-error" role="alert">{formError}</p>{/if}
							<div class="cmt-form-actions">
								<button type="submit" class="nl-button" disabled={busy === c.id || !snoozeCustom}>Snooze until then</button>
								<button type="button" class="nl-button-secondary" onclick={() => (open = null)}>Never mind</button>
							</div>
						</form>
					{:else if isOpenPanel(c, "complete")}
						<form class="cmt-form" onsubmit={(e) => { e.preventDefault(); complete(c); }}>
							<p class="cmt-hint">A commitment is only marked complete with your confirmation or recorded evidence. Say how you know, confirm it, or both.</p>
							<label class="cmt-checkbox">
								<input type="checkbox" bind:checked={confirmDone} />
								<span>I confirm this is done</span>
							</label>
							<label class="cmt-label" for={`evidence-${c.id}`}>How do you know?</label>
							<textarea id={`evidence-${c.id}`} class="nl-input cmt-textarea" rows="2" maxlength={MAX_EVIDENCE_CHARS} placeholder="A reply, a file, a receipt…" bind:value={evidence}></textarea>
							{#if formError}
								<p class="cmt-error" role="alert">{formError}</p>
							{:else if !evidenceCheck.ok}
								<p class="cmt-hint">{evidenceCheck.reason}</p>
							{/if}
							<div class="cmt-form-actions">
								<button type="submit" class="nl-button" disabled={busy === c.id || !evidenceCheck.ok}>Mark complete</button>
								<button type="button" class="nl-button-secondary" onclick={() => (open = null)}>Not yet</button>
							</div>
						</form>
					{:else if isOpenPanel(c, "cancel")}
						<div class="cmt-form">
							<p class="cmt-hint">Cancel this commitment? It stays in history as cancelled and is never checked again.</p>
							{#if formError}<p class="cmt-error" role="alert">{formError}</p>{/if}
							<div class="cmt-form-actions">
								<button type="button" class="nl-button nl-button-destructive" disabled={busy === c.id} onclick={() => cancel(c)}>Yes, cancel it</button>
								<button type="button" class="nl-button-secondary" onclick={() => (open = null)}>Keep it</button>
							</div>
						</div>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</section>

<style>
	.cmt-section { max-width: 720px; margin: 0 auto 32px; display: flex; flex-direction: column; gap: 12px; }
	.cmt-head { display: flex; align-items: flex-start; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
	.cmt-head h3 { font: 400 22px/1.2 var(--font-display); letter-spacing: -0.02em; color: var(--foreground); margin: 0 0 6px; }
	.cmt-head p { font: 400 14px/1.6 var(--font-body); color: var(--text-secondary); margin: 0; }
	.cmt-filter { display: inline-flex; gap: 4px; padding: 4px; background: var(--card); border: 1px solid var(--border); border-radius: 999px; flex-shrink: 0; }
	.cmt-filter-btn { min-height: 36px; padding: 0 14px; border: 0; border-radius: 999px; background: transparent; color: var(--text-secondary); font: 500 13px var(--font-body); cursor: pointer; }
	.cmt-filter-active { background: var(--primary); color: var(--primary-foreground); }
	.cmt-empty { display: flex; flex-direction: column; align-items: center; gap: 12px; padding: 24px 16px; text-align: center; color: var(--text-secondary); font: 400 14px/1.6 var(--font-body); background: var(--card); border: 1px solid var(--border); border-radius: 16px; margin: 0; }
	.cmt-empty p { margin: 0; }
	.cmt-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 12px; }
	.cmt-item { background: var(--card); border: 1px solid var(--border); border-radius: 16px; padding: 16px 20px; display: flex; flex-direction: column; gap: 6px; scroll-margin-top: 16px; }
	.cmt-overdue { border-color: var(--primary); }
	.cmt-closed { opacity: 0.8; }
	.cmt-item:target { outline: 2px solid var(--ring); outline-offset: 2px; }
	.cmt-row { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
	.cmt-status { font: 500 13px var(--font-body); color: var(--primary); }
	.cmt-closed .cmt-status { color: var(--text-secondary); }
	.cmt-meta { font: 400 12px var(--font-body); color: var(--text-muted); }
	.cmt-promise { margin: 0; font: 400 16px/1.5 var(--font-body); color: var(--foreground); overflow-wrap: anywhere; }
	.cmt-facts, .cmt-check { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); }
	.cmt-link { color: var(--primary); text-decoration: underline; text-underline-offset: 2px; }
	.cmt-controls { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 8px; }
	.cmt-btn { min-height: 36px; padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-control, 8px); background: var(--card); color: var(--foreground); font: 500 13px var(--font-body); cursor: pointer; }
	.cmt-btn:hover:not(:disabled) { background: var(--accent); }
	.cmt-btn[aria-expanded="true"] { background: var(--accent); border-color: var(--primary); }
	.cmt-btn:disabled { opacity: 0.5; cursor: not-allowed; }
	.cmt-details { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 4px 16px; margin: 8px 0 0; padding: 12px 0 0; border-top: 1px solid var(--border); font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); }
	.cmt-details dt { color: var(--text-muted); }
	.cmt-details dd { margin: 0; overflow-wrap: anywhere; }
	.cmt-details code { font: 400 12px var(--font-mono); color: var(--foreground); }
	.cmt-form { display: flex; flex-direction: column; gap: 8px; margin-top: 8px; padding-top: 12px; border-top: 1px solid var(--border); }
	.cmt-label { font: 500 13px var(--font-body); color: var(--foreground); margin: 0; }
	.cmt-hint { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); }
	.cmt-error { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--destructive); }
	.cmt-textarea { resize: vertical; min-height: 64px; }
	.cmt-checkbox { display: flex; align-items: center; gap: 10px; min-height: 44px; font: 400 14px var(--font-body); color: var(--foreground); cursor: pointer; }
	.cmt-checkbox input { width: 20px; height: 20px; accent-color: var(--primary); }
	.cmt-presets { display: flex; gap: 8px; flex-wrap: wrap; }
	.cmt-form-actions { display: flex; gap: 8px; flex-wrap: wrap; margin-top: 4px; }
	@media (max-width: 720px) {
		.cmt-item { padding: 14px 16px; }
		.cmt-details { grid-template-columns: 1fr; gap: 2px; }
		.cmt-details dt { margin-top: 6px; }
	}
</style>
