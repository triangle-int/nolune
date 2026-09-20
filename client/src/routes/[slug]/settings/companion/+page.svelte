<script lang="ts">
	import * as Select from "$lib/components/ui/select/index.js";
	import { page } from "$app/state";
	import {
		fetchTimezone,
		updateTimezone,
		fetchScheduledTasks,
		cancelScheduledTask,
		fetchRhythmTracking,
		updateRhythmTracking,
		fetchProactivePolicy,
		updateProactivePolicy,
		fetchResume,
		updateResumePolicy,
		snoozeResume,
		invokeResume,
		ResumeDisabled,
		type ScheduledTask,
	} from "$lib/api/client.js";
	import type { ProactivePolicy, ResumeRitualPolicy } from "$lib/api/types.js";
	import { BREAK_OPTIONS, COOLDOWN_OPTIONS, heldMessage, optionLabel, ritualSummary, snoozePresets, snoozeStatus } from "$lib/continuity/resume.js";
	import { getToasts } from "$lib/stores/toast.svelte.js";
	import { SKINS } from "$lib/stores/skin.svelte.js";

	// Everything here is companion-specific (#98): how Little Moon shows up,
	// when it may reach out, and which clock it lives by.
	const slug = $derived(page.params.slug!);

	// --- initiative (#92/#94): one policy for everything the companion starts itself ---
	let policy = $state<ProactivePolicy | null>(null);
	let policySaving = $state(false);
	const HOURS = Array.from({ length: 24 }, (_, h) => h);
	const CHECK_IN_OPTIONS = [
		{ value: 0.5, label: "Every 30 minutes" },
		{ value: 1, label: "Every hour" },
		{ value: 2, label: "Every 2 hours" },
		{ value: 4, label: "Every 4 hours" },
		{ value: 12, label: "Twice a day" },
		{ value: 24, label: "Once a day" },
	];
	$effect(() => {
		fetchProactivePolicy(slug).then((p) => (policy = p)).catch(() => {});
	});
	async function savePolicy(next: ProactivePolicy) {
		if (policySaving) return;
		policySaving = true;
		const previous = policy;
		policy = next;
		try { await updateProactivePolicy(slug, next); }
		catch { policy = previous; getToasts().error("Could not save initiative settings."); }
		finally { policySaving = false; }
	}
	function setQuietHours(start: number | null, end: number | null) {
		if (!policy) return;
		if (start === null || end === null) return savePolicy({ ...policy, quiet_hours: null });
		return savePolicy({ ...policy, quiet_hours: { start_hour: start, end_hour: end } });
	}

	// --- Resume my work (#83): opt-in, one suggestion per trigger, answered by the user ---
	let resume = $state<ResumeRitualPolicy | null>(null);
	let resumeSaving = $state(false);
	let resumeNow = $state(Math.floor(Date.now() / 1000));
	$effect(() => {
		fetchResume(slug).then((r) => (resume = r.policy)).catch(() => {});
		const tick = setInterval(() => (resumeNow = Math.floor(Date.now() / 1000)), 30_000);
		return () => clearInterval(tick);
	});
	async function saveResume(next: Pick<ResumeRitualPolicy, "enabled" | "break_minutes" | "cooldown_secs">) {
		if (resumeSaving) return;
		resumeSaving = true;
		try { resume = await updateResumePolicy(slug, next); }
		catch { getToasts().error("Could not save Resume my work settings."); }
		finally { resumeSaving = false; }
	}
	async function snoozeRitual(until: number | null) {
		if (resumeSaving) return;
		resumeSaving = true;
		try { resume = await snoozeResume(slug, until); }
		catch { getToasts().error("Could not change the snooze."); }
		finally { resumeSaving = false; }
	}
	async function suggestNow() {
		if (resumeSaving) return;
		resumeSaving = true;
		try {
			const outcome = await invokeResume(slug);
			if (outcome.suggestion) getToasts().success(`Suggested: ${outcome.suggestion.goal}`);
			else getToasts().info(heldMessage(outcome.held, resumeNow));
		} catch (e) {
			getToasts().error(e instanceof ResumeDisabled ? heldMessage({ kind: "disabled" }, resumeNow) : "Could not look for work to resume.");
		} finally {
			resumeSaving = false;
		}
	}

	// --- interaction rhythm (#95): bounded aggregate with opt-out ---
	let rhythmEnabled = $state(true);
	let rhythmLoaded = $state(false);
	let rhythmSaving = $state(false);
	$effect(() => {
		fetchRhythmTracking(slug)
			.then((r) => { rhythmEnabled = r.enabled; rhythmLoaded = true; })
			.catch(() => { rhythmLoaded = true; });
	});
	async function setRhythmTracking(next: boolean) {
		if (rhythmSaving) return;
		rhythmSaving = true;
		try { await updateRhythmTracking(slug, next); rhythmEnabled = next; }
		catch { getToasts().error("Could not update rhythm tracking."); }
		finally { rhythmSaving = false; }
	}

	// --- timezone ---
	let tzValue = $state("");
	let tzLoading = $state(true);
	let tzSaving = $state(false);
	const COMMON_TIMEZONES = [
		"Asia/Bishkek", "Asia/Almaty", "Asia/Tashkent",
		"Europe/Moscow", "Europe/London", "Europe/Berlin", "Europe/Paris",
		"America/New_York", "America/Chicago", "America/Denver", "America/Los_Angeles",
		"Asia/Tokyo", "Asia/Shanghai", "Asia/Kolkata", "Asia/Dubai",
		"Australia/Sydney", "Pacific/Auckland",
	];
	async function loadTimezone() {
		tzLoading = true;
		try {
			const res = await fetchTimezone(slug);
			tzValue = res.timezone || "";
		} catch {
			// not critical
		} finally {
			tzLoading = false;
		}
	}
	async function saveTimezone(tz: string) {
		tzSaving = true;
		try {
			await updateTimezone(slug, tz);
			tzValue = tz;
		} catch {
			// ignore
		} finally {
			tzSaving = false;
		}
	}

	// --- scheduled messages the companion will deliver later ---
	let scheduledTasks = $state<ScheduledTask[]>([]);
	let scheduledLoading = $state(true);
	let cancellingId = $state<string | null>(null);
	async function loadScheduled() {
		scheduledLoading = true;
		try {
			scheduledTasks = await fetchScheduledTasks(slug);
		} catch {
			// not critical
		} finally {
			scheduledLoading = false;
		}
	}
	async function cancelTask(id: string) {
		cancellingId = id;
		try {
			await cancelScheduledTask(slug, id);
			scheduledTasks = scheduledTasks.filter((t) => t.id !== id);
		} catch {
			// ignore
		} finally {
			cancellingId = null;
		}
	}
	function formatDeliverAt(ts: number): string {
		const d = new Date(ts * 1000);
		const diff = ts * 1000 - Date.now();
		if (diff <= 0) return "delivering...";
		if (diff < 60_000) return `in ${Math.ceil(diff / 1000)}s`;
		if (diff < 3600_000) return `in ${Math.ceil(diff / 60_000)}m`;
		if (diff < 86400_000) {
			const h = Math.floor(diff / 3600_000);
			const m = Math.ceil((diff % 3600_000) / 60_000);
			return m > 0 ? `in ${h}h ${m}m` : `in ${h}h`;
		}
		return d.toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
	}

	$effect(() => {
		slug;
		loadTimezone();
		loadScheduled();
	});
</script>

<!-- Presence, rhythm, initiative -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Companion</h3>
			<p class="section-desc">Little Moon, your familiar presence across devices.</p>
		</div>
	</div>
	<div class="section-body">
		<div class="model-mode-options">
			{#each SKINS as skin (skin.id)}
				<div class="mode-option skin-option mode-active">
					<img src={skin.thumbnail} alt={skin.label} class="skin-thumb" />
					<div>
						<span class="mode-name">{skin.label}</span>
						<span class="mode-desc">Little Moon companion</span>
					</div>
				</div>
			{/each}
	</div>

	<div class="setting-row">
		<span class="setting-label" id="rhythm-label">Learn my rhythm</span>
		<div class="setting-input-row">
			<button
				class="setting-btn"
				role="switch"
				aria-checked={rhythmEnabled}
				aria-labelledby="rhythm-label"
				disabled={!rhythmLoaded || rhythmSaving}
				onclick={() => setRhythmTracking(!rhythmEnabled)}
			>
				{rhythmSaving ? "…" : rhythmEnabled ? "On" : "Off"}
			</button>
		</div>
		<p class="setting-hint">Keeps only a bounded summary of when you tend to write and how quickly you reply, so check-ins land at good moments. Turning it off deletes the summary.</p>
	</div>

	{#if policy}
		<div class="setting-row">
			<span class="setting-label" id="initiative-label">Initiative</span>
			<div class="setting-input-row">
				<button class="setting-btn" role="switch" aria-checked={policy.enabled} aria-labelledby="initiative-label" disabled={policySaving} onclick={() => policy && savePolicy({ ...policy, enabled: !policy.enabled })}>
					{policy.enabled ? "On" : "Off"}
				</button>
			</div>
			<p class="setting-hint">Lets your companion check in, follow schedules, and react when a computer connects. Everything it does on its own is listed under <a class="settings-link" href={`/${slug}/activity`}>Activity</a>.</p>
		</div>

		<div class="setting-row">
			<label class="setting-label" for="check-in-interval">Check-in</label>
			<select id="check-in-interval" class="setting-input" disabled={!policy.enabled || policySaving} value={String(policy.check_in_interval_hours)} onchange={(e) => policy && savePolicy({ ...policy, check_in_interval_hours: Number((e.currentTarget as HTMLSelectElement).value) })}>
				{#each CHECK_IN_OPTIONS as option (option.value)}
					<option value={String(option.value)}>{option.label}</option>
				{/each}
			</select>
		</div>

		<div class="setting-row">
			<span class="setting-label" id="quiet-hours-label">Quiet hours</span>
			<div class="setting-input-row" aria-labelledby="quiet-hours-label">
				<label class="sr-only" for="quiet-start">Quiet from</label>
				<select id="quiet-start" class="setting-input" disabled={policySaving} value={policy.quiet_hours ? String(policy.quiet_hours.start_hour) : ""} onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; setQuietHours(v === "" ? null : Number(v), policy?.quiet_hours?.end_hour ?? 7); }}>
					<option value="">Off</option>
					{#each HOURS as h (h)}<option value={String(h)}>from {h}:00</option>{/each}
				</select>
				<label class="sr-only" for="quiet-end">Quiet until</label>
				<select id="quiet-end" class="setting-input" disabled={!policy.quiet_hours || policySaving} value={policy.quiet_hours ? String(policy.quiet_hours.end_hour) : "7"} onchange={(e) => setQuietHours(policy?.quiet_hours?.start_hour ?? 22, Number((e.currentTarget as HTMLSelectElement).value))}>
					{#each HOURS as h (h)}<option value={String(h)}>until {h}:00</option>{/each}
				</select>
			</div>
			<p class="setting-hint">No spontaneous check-ins or messages during quiet hours, in your companion's timezone. A commitment that comes due then waits for the morning.</p>
		</div>

		<div class="setting-row">
			<label class="setting-label" for="reach-out-budget">Messages per day</label>
			<input id="reach-out-budget" class="setting-input" type="number" min="0" max="48" disabled={policySaving} value={policy.daily_reach_out_budget} onchange={(e) => policy && savePolicy({ ...policy, daily_reach_out_budget: Math.max(0, Math.min(48, Number((e.currentTarget as HTMLInputElement).value) || 0)) })} />
			<p class="setting-hint">How many times a day your companion may message you first, commitment check-ins included. See what it is holding under <a class="settings-link" href={`/${slug}/activity`}>Activity</a>.</p>
		</div>

		<div class="setting-row">
			<span class="setting-label" id="reflection-label">Reflection</span>
			<div class="setting-input-row">
				<button class="setting-btn" role="switch" aria-checked={policy.reflection_enabled} aria-labelledby="reflection-label" disabled={!policy.enabled || policySaving} onclick={() => policy && savePolicy({ ...policy, reflection_enabled: !policy.reflection_enabled })}>
					{policy.reflection_enabled ? "On" : "Off"}
				</button>
			</div>
			<p class="setting-hint">Every few days your companion writes a reflection into its memory. It can add and connect memories, never delete them.</p>
		</div>
	{/if}
	</div>
</section>

<!-- Resume my work (#83) -->
{#if resume}
	<section class="settings-section">
		<div class="section-header">
			<div>
				<h3 class="section-label">Resume my work</h3>
				<p class="section-desc">One way to pick unfinished work up again, from your task records only. Nothing continues until you accept it.</p>
			</div>
		</div>
		<div class="section-body">
			<div class="setting-row">
				<span class="setting-label" id="resume-label">Suggest resuming</span>
				<div class="setting-input-row">
					<button class="setting-btn" role="switch" aria-checked={resume.enabled} aria-labelledby="resume-label" disabled={resumeSaving} onclick={() => resume && saveResume({ ...resume, enabled: !resume.enabled })}>
						{resume.enabled ? "On" : "Off"}
					</button>
					<button class="setting-btn" disabled={!resume.enabled || resumeSaving} onclick={suggestNow}>Suggest now</button>
				</div>
				<p class="setting-hint">{ritualSummary(resume, Boolean(policy?.quiet_hours))} Each suggestion says which task, why it was picked, and why now; it leads to the task's card under <a class="settings-link" href={`/${slug}/activity`}>Activity</a>.</p>
			</div>

			<div class="setting-row">
				<label class="setting-label" for="resume-break">After a break of</label>
				<select id="resume-break" class="setting-input" disabled={!resume.enabled || resumeSaving} value={String(resume.break_minutes)} onchange={(e) => resume && saveResume({ ...resume, break_minutes: Number((e.currentTarget as HTMLSelectElement).value) })}>
					{#if !BREAK_OPTIONS.some((o) => o.value === resume?.break_minutes)}
						<option value={String(resume.break_minutes)}>{optionLabel(BREAK_OPTIONS, resume.break_minutes)}</option>
					{/if}
					{#each BREAK_OPTIONS as option (option.value)}
						<option value={String(option.value)}>{option.label}</option>
					{/each}
				</select>
				<p class="setting-hint">Opening Nolune after this long away counts as coming back. A computer reconnecting counts only when a waiting task names it.</p>
			</div>

			<div class="setting-row">
				<label class="setting-label" for="resume-cooldown">At most one every</label>
				<select id="resume-cooldown" class="setting-input" disabled={!resume.enabled || resumeSaving} value={String(resume.cooldown_secs)} onchange={(e) => resume && saveResume({ ...resume, cooldown_secs: Number((e.currentTarget as HTMLSelectElement).value) })}>
					{#if !COOLDOWN_OPTIONS.some((o) => o.value === resume?.cooldown_secs)}
						<option value={String(resume.cooldown_secs)}>{optionLabel(COOLDOWN_OPTIONS, resume.cooldown_secs)}</option>
					{/if}
					{#each COOLDOWN_OPTIONS as option (option.value)}
						<option value={String(option.value)}>{option.label}</option>
					{/each}
				</select>
				<p class="setting-hint">The gap after a suggestion, or after you say not now. Quiet hours above hold suggestions too; asking with Suggest now always answers.</p>
			</div>

			<div class="setting-row">
				<label class="setting-label" for="resume-snooze">Snooze</label>
				<div class="setting-input-row">
					{#if snoozeStatus(resume, resumeNow)}
						<span class="setting-hint" role="status">{snoozeStatus(resume, resumeNow)}</span>
						<button class="setting-btn" disabled={resumeSaving} onclick={() => snoozeRitual(null)}>End snooze</button>
					{:else}
						<select id="resume-snooze" class="setting-input" disabled={!resume.enabled || resumeSaving} value="" onchange={(e) => { const v = (e.currentTarget as HTMLSelectElement).value; if (v) snoozeRitual(Number(v)); }}>
							<option value="">Not snoozed</option>
							{#each snoozePresets(resumeNow) as preset (preset.label)}
								<option value={String(preset.until)}>For {preset.label.toLowerCase() === "tomorrow" ? "a day" : preset.label}</option>
							{/each}
						</select>
					{/if}
				</div>
				<p class="setting-hint">No suggestions on their own until the snooze ends. Tasks you answered with Never this task stay in Unfinished tasks{resume.dismissed_record_ids.length > 0 ? ` (${resume.dismissed_record_ids.length} so far)` : ""}.</p>
			</div>
		</div>
	</section>
{/if}

<!-- Timezone -->
<section class="settings-section">
	<div class="section-header">
		<div>
			<h3 class="section-label">Timezone</h3>
			<p class="section-desc">Set your local timezone so your companion knows the right time of day.</p>
		</div>
	</div>
	<div class="section-body">

		{#if tzLoading}
			<div class="ext-loading"><div class="loading-dot"></div></div>
		{:else}
			<div class="tz-picker">
				<Select.Root type="single" value={tzValue || "default"} disabled={tzSaving}
					onValueChange={(value) => saveTimezone(value === "default" ? "" : value)}>
					<Select.Trigger aria-label="Timezone" class="h-11 w-full min-w-0 border-input bg-card text-foreground dark:bg-card">
						<span data-slot="select-value">{tzValue ? tzValue.replace(/_/g, " ") : "UTC (default)"}</span>
					</Select.Trigger>
					<Select.Content class="max-h-80">
						<Select.Item value="default" label="UTC (default)" class="min-h-11">UTC (default)</Select.Item>
						{#each COMMON_TIMEZONES as tz (tz)}
							<Select.Item value={tz} label={tz.replace(/_/g, " ")} class="min-h-11">{tz.replace(/_/g, " ")}</Select.Item>
						{/each}
					</Select.Content>
				</Select.Root>
				{#if tzValue}
					<span class="tz-current">{tzValue.replace(/_/g, " ")}</span>
				{/if}
			</div>
		{/if}
	</div>
</section>

<!-- Scheduled messages -->
{#if !scheduledLoading && scheduledTasks.length > 0}
	<section class="settings-section">
		<div class="section-header">
			<div>
				<h3 class="section-label">Scheduled</h3>
				<p class="section-desc">{scheduledTasks.length} pending message{scheduledTasks.length === 1 ? "" : "s"} your companion will deliver later.</p>
			</div>
		</div>
		<div class="section-body">
			<div class="sched-list">
				{#each scheduledTasks as task (task.id)}
					<div class="sched-item">
						<div class="sched-content">
							<span class="sched-text">{task.task.length > 80 ? task.task.slice(0, 80) + "…" : task.task}</span>
							<span class="sched-time">{formatDeliverAt(task.deliver_at)}</span>
						</div>
						<button class="sched-cancel" disabled={cancellingId === task.id} onclick={() => cancelTask(task.id)}>
							{cancellingId === task.id ? "…" : "Cancel"}
						</button>
					</div>
				{/each}
		</div>
		</div>
	</section>
{/if}
