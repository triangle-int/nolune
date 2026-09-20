<script lang="ts">
	// One reviewable handoff (#82). Everything shown comes from the server's
	// card, derived from the continuity record and the known machines, so it
	// reads the same while the origin computer is offline. The four decisions
	// and the preview go through the callbacks, so the card renders in the
	// design-system gallery without a backend. Nothing starts until the user
	// confirms a previewed continuation.
	import type { ContinuationPreview, HandoffAccepted, HandoffCard, MachineInfo } from "$lib/api/types.js";
	import {
		cardActions,
		decisionLabel,
		groupChecks,
		machineChoices,
		originCopy,
		requiredSummary,
	} from "$lib/continuity/handoff.js";
	import { relativeTime } from "$lib/activity/receipts.js";

	let {
		card,
		machines,
		hereId,
		now,
		onpreview,
		onaccept,
		onkeep,
		ondismiss,
		onupdated,
		onhere,
	}: {
		card: HandoffCard;
		machines: MachineInfo[];
		/** The computer "Continue here" means, when it is known; else the card asks. */
		hereId: string | null;
		now: number;
		onpreview: (machineId: string) => Promise<ContinuationPreview>;
		onaccept: (machineId: string) => Promise<HandoffAccepted>;
		onkeep: () => Promise<HandoffCard>;
		ondismiss: () => Promise<HandoffCard>;
		onupdated?: (card: HandoffCard) => void;
		/** The user said which connected computer is this one. */
		onhere?: (machineId: string) => void;
	} = $props();

	type Mode = "idle" | "pick" | "preview";
	let mode = $state<Mode>("idle");
	let pickingHere = $state(false);
	let destination = $state<string | null>(null);
	let preview = $state<ContinuationPreview | null>(null);
	let pending = $state<"" | "preview" | "accept" | "keep" | "dismiss">("");
	let error = $state("");
	let refusal = $state<string[]>([]);
	let notice = $state("");

	const actions = $derived(cardActions(card));
	const choices = $derived(machineChoices(machines, card.origin?.machine_id ?? null));
	const grouped = $derived(preview ? groupChecks(preview.checks) : { blocking: [], approvals: [], notes: [] });
	const requirements = $derived(requiredSummary(card.required));
	const decision = $derived(decisionLabel(card));
	const stateLabel = $derived(
		card.state === "waiting" ? "Waiting" : card.state === "ready_to_resume" ? "Ready to resume" : "",
	);

	function reset() {
		mode = "idle";
		preview = null;
		destination = null;
		refusal = [];
		error = "";
	}

	function continueHere() {
		notice = "";
		if (hereId) {
			openPreview(hereId);
		} else {
			pickingHere = true;
			mode = "pick";
			error = "";
		}
	}

	function continueOn() {
		notice = "";
		pickingHere = false;
		mode = "pick";
		error = "";
	}

	function choose(machineId: string) {
		if (pickingHere) onhere?.(machineId);
		openPreview(machineId);
	}

	async function openPreview(machineId: string) {
		pending = "preview";
		error = "";
		refusal = [];
		destination = machineId;
		try {
			preview = await onpreview(machineId);
			mode = "preview";
		} catch (e) {
			error = e instanceof Error && e.message ? e.message : "Could not check that computer.";
			mode = "idle";
		} finally {
			pending = "";
		}
	}

	async function confirm() {
		if (!destination) return;
		pending = "accept";
		error = "";
		refusal = [];
		try {
			const accepted = await onaccept(destination);
			notice = accepted.already_running
				? `Already continuing on ${accepted.card.bound_to?.display_name ?? destination}; nothing new was started.`
				: "";
			onupdated?.(accepted.card);
			reset();
		} catch (e) {
			// A refusal carries the server's checks; anything else is one line.
			const checks = e instanceof Error && "checks" in e ? (e as { checks: { detail: string }[] }).checks : null;
			if (checks && checks.length > 0) {
				refusal = checks.map((c) => c.detail);
			} else {
				error = e instanceof Error && e.message ? e.message : "Could not continue the task.";
			}
		} finally {
			pending = "";
		}
	}

	async function keep() {
		pending = "keep";
		error = "";
		notice = "";
		try {
			onupdated?.(await onkeep());
			reset();
		} catch (e) {
			error = e instanceof Error && e.message ? e.message : "Could not keep the task there.";
		} finally {
			pending = "";
		}
	}

	async function dismiss() {
		pending = "dismiss";
		error = "";
		notice = "";
		try {
			onupdated?.(await ondismiss());
			reset();
		} catch (e) {
			error = e instanceof Error && e.message ? e.message : "Could not dismiss the task.";
		} finally {
			pending = "";
		}
	}
</script>

<article class="handoff" class:handoff-continuing={actions.continuing} aria-labelledby={`handoff-${card.record_id}`}>
	<header class="handoff-head">
		<p class="nl-eyebrow">Unfinished task{stateLabel ? ` · ${stateLabel}` : ""}</p>
		<h3 id={`handoff-${card.record_id}`}>{card.goal}</h3>
		<p class="handoff-origin">{originCopy(card, now)} · updated {relativeTime(card.updated_at, now)}</p>
		{#if decision}<p class="handoff-decision" class:handoff-decision-live={actions.continuing}>{decision}</p>{/if}
	</header>

	<div class="handoff-body">
		{#if card.completed_steps.length > 0}
			<section>
				<h4>Done so far</h4>
				<ul>{#each card.completed_steps as step, i (i)}<li>{step}</li>{/each}</ul>
			</section>
		{/if}
		{#if card.next_step}
			<section>
				<h4>Next step</h4>
				<p>{card.next_step}</p>
			</section>
		{/if}
		{#if card.resources.length > 0}
			<section>
				<h4>Resources</h4>
				<ul>
					{#each card.resources as item, i (i)}
						<li><code>{item.label}</code>{#if !item.available}<span class="handoff-missing"> · cannot be found</span>{/if}</li>
					{/each}
				</ul>
			</section>
		{/if}
		{#if card.blockers.length > 0}
			<section>
				<h4>Blockers</h4>
				<ul>{#each card.blockers as blocker, i (i)}<li>{blocker}</li>{/each}</ul>
			</section>
		{/if}
		{#if requirements}
			<p class="handoff-requires">{requirements}</p>
		{/if}
	</div>

	{#if mode === "pick"}
		<div class="handoff-step" role="group" aria-label={pickingHere ? "Which computer is this one?" : "Continue on which computer?"}>
			<p class="handoff-step-title">{pickingHere ? "Which of these computers is this one?" : "Continue on which computer?"}</p>
			{#if choices.length === 0}
				<p class="handoff-muted">No computer has connected to this companion yet. Open the desktop app on the computer you want to use.</p>
			{:else}
				<ul class="handoff-choices">
					{#each choices as choice (choice.machine_id)}
						<li>
							<button
								class="nl-button-secondary handoff-choice"
								disabled={!choice.available || pending !== ""}
								aria-disabled={!choice.available}
								onclick={() => choose(choice.machine_id)}
							>
								{choice.label}
							</button>
						</li>
					{/each}
				</ul>
			{/if}
			<div class="handoff-controls">
				<button class="nl-button-secondary" onclick={reset} disabled={pending !== ""}>Back</button>
			</div>
		</div>
	{:else if mode === "preview" && preview}
		<div class="handoff-step" role="group" aria-label="Before continuing">
			<p class="handoff-step-title">
				Continue on <strong>{preview.destination.display_name}</strong>
				{#if preview.destination.platform}<span class="handoff-muted"> · {preview.destination.platform}</span>{/if}
				<span class="handoff-muted"> · {preview.destination.online ? (preview.destination.health === "degraded" ? "connected, not responding" : "connected") : "offline"}</span>
			</p>
			{#if grouped.blocking.length > 0}
				<div class="handoff-checks handoff-checks-stop" role="alert">
					<h4>Cannot continue yet</h4>
					<ul>{#each grouped.blocking as detail, i (i)}<li>{detail}</li>{/each}</ul>
				</div>
			{/if}
			{#if grouped.approvals.length > 0}
				<div class="handoff-checks">
					<h4>Will ask you first</h4>
					<ul>{#each grouped.approvals as detail, i (i)}<li>{detail}</li>{/each}</ul>
				</div>
			{/if}
			{#if grouped.notes.length > 0}
				<div class="handoff-checks">
					<h4>Worth knowing</h4>
					<ul>{#each grouped.notes as detail, i (i)}<li>{detail}</li>{/each}</ul>
				</div>
			{/if}
			{#if preview.ready}
				<p class="handoff-muted">The task is handed to its conversation naming this computer. Nothing runs until you confirm.</p>
			{/if}
			{#if refusal.length > 0}
				<div class="handoff-checks handoff-checks-stop" role="alert">
					<h4>Refused just now</h4>
					<ul>{#each refusal as detail, i (i)}<li>{detail}</li>{/each}</ul>
				</div>
			{/if}
			<div class="handoff-controls">
				<button class="nl-button" onclick={confirm} disabled={!preview.ready || pending !== ""}>
					{pending === "accept" ? "Continuing…" : `Continue on ${preview.destination.display_name}`}
				</button>
				<button class="nl-button-secondary" onclick={reset} disabled={pending !== ""}>Back</button>
			</div>
		</div>
	{:else if actions.continuing}
		<p class="handoff-muted">Continuing now. Progress lands on this task and in the activity trail.</p>
	{:else if card.offered}
		<div class="handoff-controls">
			<button class="nl-button" onclick={continueHere} disabled={!actions.continueHere || pending !== ""}>
				{pending === "preview" ? "Checking…" : "Continue here"}
			</button>
			<button class="nl-button-secondary" onclick={continueOn} disabled={!actions.continueOn || pending !== ""}>Continue on…</button>
			<button class="nl-button-secondary" onclick={keep} disabled={!actions.keepThere || pending !== ""}>
				{pending === "keep" ? "Keeping…" : "Keep there"}
			</button>
			<button class="nl-button-secondary" onclick={dismiss} disabled={!actions.dismiss || pending !== ""}>
				{pending === "dismiss" ? "Dismissing…" : "Dismiss"}
			</button>
		</div>
	{/if}

	{#if error}<p class="handoff-error" role="alert">{error}</p>{/if}
	{#if notice}<p class="handoff-muted" role="status">{notice}</p>{/if}
</article>

<style>
	.handoff { background: var(--card); border: 1px solid var(--border); border-radius: 16px; padding: 20px; display: flex; flex-direction: column; gap: 14px; }
	.handoff-continuing { border-color: var(--primary); }
	.handoff-head { display: flex; flex-direction: column; gap: 6px; }
	.handoff-head h3 { margin: 0; font: 500 18px/1.3 var(--font-body); color: var(--foreground); overflow-wrap: anywhere; }
	.handoff-origin { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--text-secondary); }
	.handoff-decision { margin: 0; font: 500 13px/1.5 var(--font-body); color: var(--text-secondary); }
	.handoff-decision-live { color: var(--primary); }
	.handoff-body { display: flex; flex-direction: column; gap: 10px; }
	.handoff-body section { display: flex; flex-direction: column; gap: 4px; }
	.handoff-body h4, .handoff-checks h4 { margin: 0; font: 500 12px/1.5 var(--font-body); letter-spacing: 0.04em; text-transform: uppercase; color: var(--text-muted); }
	.handoff-body p, .handoff-body li, .handoff-checks li { font: 400 14px/1.5 var(--font-body); color: var(--text-secondary); margin: 0; overflow-wrap: anywhere; }
	.handoff-body ul, .handoff-checks ul { margin: 0; padding-left: 18px; display: flex; flex-direction: column; gap: 2px; }
	.handoff-body code { font: 400 13px var(--font-mono); color: var(--foreground); }
	.handoff-missing { color: var(--destructive); }
	.handoff-requires { font-size: 13px; color: var(--text-muted); }
	.handoff-step { display: flex; flex-direction: column; gap: 12px; padding-top: 12px; border-top: 1px solid var(--border); }
	.handoff-step-title { margin: 0; font: 400 15px/1.5 var(--font-body); color: var(--foreground); }
	.handoff-step-title strong { font-weight: 500; }
	.handoff-choices { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
	.handoff-choice { width: 100%; justify-content: flex-start; text-align: left; }
	.handoff-checks { display: flex; flex-direction: column; gap: 4px; }
	.handoff-checks-stop h4, .handoff-checks-stop li { color: var(--destructive); }
	.handoff-controls { display: flex; flex-wrap: wrap; gap: 8px; }
	.handoff-muted { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--text-muted); }
	.handoff-error { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--destructive); }
	@media (max-width: 720px) {
		.handoff { padding: 16px; }
		.handoff-controls .nl-button, .handoff-controls .nl-button-secondary { flex: 1 1 calc(50% - 8px); }
	}
</style>
