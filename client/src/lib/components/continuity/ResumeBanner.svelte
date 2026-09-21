<script lang="ts">
	// One resume suggestion (#83): the record the ritual picked, why now,
	// and why this one, in the server's words. "Review and continue" leads
	// to the task's handoff card, where continuation is decided; nothing
	// here starts work. Not now, Snooze, and Never this task answer the
	// ritual itself through the callbacks, so the banner renders in the
	// design-system gallery without a backend.
	import type { ResumeOffer } from "$lib/api/types.js";
	import { snoozePresets, suggestedLabel } from "$lib/continuity/resume.js";

	let {
		offer,
		now,
		reviewHref,
		onrefuse,
		onsnooze,
		ondismiss,
	}: {
		offer: ResumeOffer;
		now: number;
		/** Where the task's handoff card is. */
		reviewHref: string;
		onrefuse: () => Promise<void>;
		onsnooze: (until: number) => Promise<void>;
		ondismiss: () => Promise<void>;
	} = $props();

	let snoozing = $state(false);
	let pending = $state<"" | "refuse" | "snooze" | "dismiss">("");
	let error = $state("");

	const presets = $derived(snoozePresets(now));

	async function run(kind: "refuse" | "snooze" | "dismiss", action: () => Promise<void>) {
		pending = kind;
		error = "";
		try {
			await action();
		} catch {
			error = "Could not answer that right now. Please try again.";
		} finally {
			pending = "";
		}
	}
</script>

<aside class="resume" aria-labelledby={`resume-${offer.id}`}>
	<p class="nl-eyebrow resume-eyebrow">Resume my work</p>
	<h3 id={`resume-${offer.id}`}>{offer.goal}</h3>
	<p class="resume-why-now">{offer.why_now}</p>
	<p class="resume-why-this"><span class="resume-label">Why this one</span> {offer.why_this}</p>
	{#if offer.card.next_step}
		<p class="resume-next"><span class="resume-label">Next step</span> {offer.card.next_step}</p>
	{/if}
	<p class="resume-meta">{suggestedLabel(offer, now)} · Nothing continues until you accept it on the card.</p>

	{#if snoozing}
		<div class="resume-step" role="group" aria-label="Snooze for how long?">
			<p class="resume-step-title">No suggestions for…</p>
			<div class="resume-controls">
				{#each presets as preset (preset.label)}
					<button class="nl-button-secondary" disabled={pending !== ""} onclick={() => run("snooze", () => onsnooze(preset.until))}>{preset.label}</button>
				{/each}
				<button class="nl-button-secondary" disabled={pending !== ""} onclick={() => (snoozing = false)}>Back</button>
			</div>
		</div>
	{:else}
		<div class="resume-controls">
			<a class="nl-button" href={reviewHref}>Review and continue</a>
			<button class="nl-button-secondary" disabled={pending !== ""} onclick={() => run("refuse", onrefuse)}>{pending === "refuse" ? "…" : "Not now"}</button>
			<button class="nl-button-secondary" disabled={pending !== ""} onclick={() => (snoozing = true)}>Snooze</button>
			<button class="nl-button-secondary" disabled={pending !== ""} onclick={() => run("dismiss", ondismiss)}>{pending === "dismiss" ? "…" : "Never this task"}</button>
		</div>
	{/if}
	{#if error}<p class="resume-error" role="alert">{error}</p>{/if}
</aside>

<style>
	.resume { background: var(--card); border: 1px solid var(--border); border-radius: 16px; padding: 16px 20px; display: flex; flex-direction: column; gap: 6px; }
	.resume-eyebrow { margin: 0; }
	.resume h3 { margin: 0; font: 500 18px/1.3 var(--font-body); color: var(--foreground); overflow-wrap: anywhere; }
	.resume-why-now { margin: 0; font: 400 14px/1.5 var(--font-body); color: var(--text-secondary); }
	.resume-why-this, .resume-next { margin: 0; font: 400 14px/1.5 var(--font-body); color: var(--text-secondary); overflow-wrap: anywhere; }
	.resume-label { font-weight: 500; color: var(--foreground); }
	.resume-label::after { content: ":"; }
	.resume-meta { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--text-muted); }
	.resume-step { display: flex; flex-direction: column; gap: 8px; padding-top: 10px; border-top: 1px solid var(--border); }
	.resume-step-title { margin: 0; font: 400 14px/1.5 var(--font-body); color: var(--foreground); }
	.resume-controls { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 6px; }
	.resume-controls .nl-button { text-decoration: none; }
	.resume-error { margin: 0; font: 400 13px/1.5 var(--font-body); color: var(--destructive); }
	@media (max-width: 720px) {
		.resume { padding: 14px 16px; }
		.resume-controls .nl-button, .resume-controls .nl-button-secondary { flex: 1 1 calc(50% - 8px); }
	}
</style>
