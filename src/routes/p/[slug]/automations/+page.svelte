<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import type { SubmitFunction } from '@sveltejs/kit';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import WebhookIcon from '@lucide/svelte/icons/webhook';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import StepIcon from '$lib/components/chat/StepIcon.svelte';
	import { getPreferences } from '$lib/preferences.svelte';
	import { cn } from '$lib/utils';

	let { data, form } = $props();

	const prefs = getPreferences();

	/** The automation whose edit form is open. */
	let editing = $state<string | null>(null);

	/** The calendar day picked; another month falls back to that month's default day. */
	let picked = $state<string | null>(null);
	const day = $derived(
		data.calendar.cells.find((c) => c.key === picked) ??
			data.calendar.cells.find((c) => c.key === data.calendar.selected)
	);

	type Run = (typeof data.triggers)[number]['runs'][number];

	const STATUS: Record<Run['status'], string> = {
		pending: 'waiting',
		running: 'running',
		ok: 'done',
		notified: 'notified',
		silent: 'nothing to report',
		stopped: 'stopped',
		failed: 'failed'
	};

	const SOURCE: Record<Run['source'], string> = {
		cron: 'scheduled',
		once: 'scheduled',
		webhook: 'webhook',
		wake: 'woken by script',
		manual: 'run by hand'
	};

	// Keep what's typed if saving fails; close the form once it's saved.
	const save: SubmitFunction =
		() =>
		async ({ result, update }) => {
			await update({ reset: false });
			if (result.type === 'success') editing = null;
		};

	function fallbackIcon(kind: (typeof data.triggers)[number]['kind'] | null) {
		return kind === 'webhook' ? WebhookIcon : ClockIcon;
	}
</script>

<PageHeader>
	<span class="truncate text-lg font-medium">Automations</span>
</PageHeader>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto max-w-2xl space-y-8 px-4 py-6 sm:py-10">
		<p class="text-muted-foreground">
			Things btw does on its own: reminders, regular checks and replies to other apps. What it finds
			shows up under the bell. To add or change one, just ask btw in a chat.
		</p>

		{#if form?.message}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm">{form.message}</p>
		{/if}

		{#if data.triggers.length}
			<section class="space-y-3">
				<div class="flex items-center gap-1">
					<h2 class="mr-auto font-medium">{data.calendar.title}</h2>
					{#if data.calendar.current}
						<Button
							href="?month={data.calendar.current}"
							data-sveltekit-noscroll
							variant="ghost"
							size="sm">Today</Button
						>
					{/if}
					<Button
						href={data.calendar.prev ? `?month=${data.calendar.prev}` : undefined}
						data-sveltekit-noscroll
						disabled={!data.calendar.prev}
						variant="ghost"
						size="icon-sm"
						aria-label="Previous month"><ChevronLeftIcon /></Button
					>
					<Button
						href="?month={data.calendar.next}"
						data-sveltekit-noscroll
						variant="ghost"
						size="icon-sm"
						aria-label="Next month"><ChevronRightIcon /></Button
					>
				</div>

				<div class="rounded-3xl border p-2 text-sm sm:p-3">
					{#if data.calendar.frequent.length}
						<ul class="mb-2 space-y-1 rounded-2xl bg-muted/50 px-3 py-2">
							{#each data.calendar.frequent as item (item.id)}
								<li>
									<a href="#{item.id}" class="flex items-start gap-2 hover:underline">
										<StepIcon
											name={item.icon}
											fallback={fallbackIcon(item.kind)}
											class="mt-0.5 size-4 shrink-0 text-muted-foreground"
										/>
										<span class="min-w-0">
											{item.name}
											<span class="text-muted-foreground">· {item.schedule.toLowerCase()}</span>
										</span>
									</a>
								</li>
							{/each}
						</ul>
					{/if}

					<div class="grid grid-cols-7 text-center text-xs text-muted-foreground">
						{#each data.calendar.weekdays as weekday (weekday)}
							<div class="py-1.5">{weekday}</div>
						{/each}
					</div>
					<div class="grid grid-cols-7 gap-0.5">
						{#each data.calendar.cells as cell (cell.key)}
							<button
								type="button"
								onclick={() => (picked = cell.key)}
								aria-pressed={day?.key === cell.key}
								aria-label="{cell.title}: {cell.entries.length
									? `${cell.entries.length} automation runs`
									: 'nothing'}"
								class={cn(
									'flex h-14 flex-col items-center gap-1 rounded-xl pt-1.5 transition-colors hover:bg-muted sm:h-16',
									day?.key === cell.key && 'bg-muted',
									(!cell.inMonth || cell.isPast) && 'text-muted-foreground/60'
								)}
							>
								<span
									class={cn(
										'flex size-6 items-center justify-center rounded-full text-xs tabular-nums',
										cell.isToday && 'bg-primary font-medium text-primary-foreground'
									)}>{cell.day}</span
								>
								<span class="flex items-center gap-0.5 text-muted-foreground">
									{#each cell.icons as item (item.key)}
										<StepIcon
											name={item.icon}
											fallback={fallbackIcon(item.kind)}
											class="size-3 sm:size-3.5"
										/>
									{/each}
									{#if cell.more}
										<span class="text-[10px] leading-none">+{cell.more}</span>
									{/if}
								</span>
							</button>
						{/each}
					</div>

					{#if day}
						<div class="mt-2 border-t px-1 pt-3 sm:px-2">
							<p class="px-2 pb-1 font-medium">
								{day.relative ? `${day.relative} · ` : ''}{day.title}
							</p>
							{#if day.entries.length}
								<ul>
									{#each day.entries as entry, i (i)}
										{@const href = entry.conversationId
											? resolve('/p/[slug]/c/[id]', {
													slug: data.profile.slug,
													id: entry.conversationId
												})
											: entry.triggerId
												? `#${entry.triggerId}`
												: undefined}
										<li>
											<svelte:element
												this={href ? 'a' : 'div'}
												{href}
												class={cn(
													'flex items-center gap-3 rounded-xl px-2 py-1.5',
													href && 'hover:bg-muted'
												)}
											>
												<span class="w-10 shrink-0 text-muted-foreground tabular-nums"
													>{entry.time}</span
												>
												<StepIcon
													name={entry.icon}
													fallback={fallbackIcon(entry.kind)}
													class="size-4 shrink-0 text-muted-foreground"
												/>
												<span class="min-w-0 flex-1 truncate">{entry.name}</span>
												{#if entry.status}
													<span
														class={cn(
															'shrink-0 text-xs',
															entry.status === 'failed'
																? 'text-destructive'
																: 'text-muted-foreground'
														)}>{STATUS[entry.status]}</span
													>
												{/if}
											</svelte:element>
										</li>
									{/each}
								</ul>
							{:else}
								<p class="px-2 pb-1 text-muted-foreground">
									{day.isPast ? 'Nothing ran on this day.' : 'Nothing runs on this day.'}
								</p>
							{/if}
						</div>
					{/if}
				</div>
				<p class="text-xs text-muted-foreground">
					Times are in the computer's time zone ({data.timeZone}).
				</p>
			</section>
		{/if}

		<section class="space-y-4">
			{#if data.triggers.length}
				<h2 class="font-medium">All automations</h2>
			{/if}
			{#each data.triggers as t (t.id)}
				<article id={t.id} class="scroll-mt-6 space-y-3 rounded-3xl border p-4 text-sm sm:p-5">
					<div class="flex items-start gap-3">
						<span
							class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground"
						>
							<StepIcon name={t.icon} fallback={fallbackIcon(t.kind)} />
						</span>
						<div class="min-w-0 flex-1">
							<h3 class="font-medium">{t.name}</h3>
							<p class="text-muted-foreground">
								{t.schedule}{#if t.next}&nbsp;· next {t.next}{/if}
							</p>
							{#if prefs.technical && t.cron}
								<p class="font-mono text-xs text-muted-foreground">{t.cron}</p>
							{/if}
						</div>
						{#if t.state !== 'on'}
							<Badge variant="secondary">{t.state}</Badge>
						{/if}
					</div>

					{#if t.summary}
						<p>{t.summary}</p>
					{/if}

					<form method="POST" use:enhance class="flex flex-wrap gap-2">
						<input type="hidden" name="id" value={t.id} />
						<Button type="submit" formaction="?/run" variant="outline" size="sm">Run now</Button>
						{#if t.state !== 'done'}
							<Button type="submit" formaction="?/toggle" variant="outline" size="sm"
								>{t.state === 'paused' ? 'Resume' : 'Pause'}</Button
							>
						{/if}
						<Button
							type="button"
							variant={editing === t.id ? 'secondary' : 'outline'}
							size="sm"
							aria-expanded={editing === t.id}
							onclick={() => (editing = editing === t.id ? null : t.id)}>Edit</Button
						>
					</form>

					{#if editing === t.id}
						<form method="POST" action="?/edit" use:enhance={save} class="space-y-3 border-t pt-3">
							<input type="hidden" name="id" value={t.id} />
							<label class="block space-y-1.5">
								<span class="text-xs text-muted-foreground">Description</span>
								<Textarea
									name="summary"
									rows={2}
									value={t.summary ?? ''}
									placeholder="What it does, in one sentence"
									class="rounded-2xl"
								/>
							</label>
							<label class="block space-y-1.5">
								<span class="text-xs text-muted-foreground">
									{#if t.action === 'agent'}
										Instructions for btw{prefs.technical
											? ` (${t.model}, reasoning ${t.effort})`
											: ''}
									{:else}
										Script: runs without the model and calls <code>btw wake</code> when btw is needed
									{/if}
								</span>
								<Textarea
									name="text"
									rows={4}
									value={t.text}
									class={cn('rounded-2xl', t.action === 'script' && 'font-mono text-xs')}
								/>
							</label>
							{#if t.webhookUrl}
								<label class="block space-y-1.5">
									<span class="text-xs text-muted-foreground"
										>Webhook URL. Keep it secret: anyone with it can start a run. POST JSON to it.</span
									>
									<input
										readonly
										value={t.webhookUrl}
										onfocus={(event) => event.currentTarget.select()}
										class="w-full rounded-xl border bg-muted/50 px-3 py-2 font-mono text-xs outline-none"
									/>
								</label>
							{/if}
							<div class="flex flex-wrap gap-2">
								<Button type="submit" size="sm">Save</Button>
								<Button type="button" variant="outline" size="sm" onclick={() => (editing = null)}
									>Cancel</Button
								>
								<Button
									type="submit"
									formaction="?/remove"
									variant="ghost"
									size="sm"
									class="ml-auto text-muted-foreground hover:text-destructive"
									onclick={(event) => {
										if (!confirm(`Delete "${t.name}"?`)) event.preventDefault();
									}}>Delete</Button
								>
							</div>
						</form>
					{/if}

					<Collapsible.Root class="border-t pt-3">
						<Collapsible.Trigger
							class="group/runs flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
						>
							{t.runs.length ? `Recent runs (${t.runs.length})` : 'No runs yet'}
							{#if t.runs.length}
								<ChevronRightIcon
									class="size-3.5 transition-transform group-data-[state=open]/runs:rotate-90"
								/>
							{/if}
						</Collapsible.Trigger>
						<Collapsible.Content>
							<ul class="mt-2 space-y-1.5">
								{#each t.runs as run (run.id)}
									<li class="flex flex-wrap items-baseline gap-x-2 text-xs">
										<span class="text-muted-foreground">{run.at}</span>
										<span class={run.status === 'failed' ? 'text-destructive' : ''}>
											{run.action === 'script' ? 'script ' : ''}{STATUS[run.status]}
										</span>
										<span class="text-muted-foreground">{SOURCE[run.source]}</span>
										{#if run.conversationId}
											<a
												href={resolve('/p/[slug]/c/[id]', {
													slug: data.profile.slug,
													id: run.conversationId
												})}
												class="text-muted-foreground underline hover:text-foreground">view</a
											>
										{/if}
										{#if run.output}
											<details class="w-full">
												<summary class="cursor-pointer text-muted-foreground">output</summary>
												<pre
													class="mt-1 max-h-60 overflow-auto rounded-xl bg-muted/50 p-2 font-mono whitespace-pre-wrap text-muted-foreground">{run.output}</pre>
											</details>
										{/if}
									</li>
								{/each}
							</ul>
						</Collapsible.Content>
					</Collapsible.Root>
				</article>
			{:else}
				<div class="space-y-3 rounded-3xl border border-dashed p-6 text-sm text-muted-foreground">
					<p>No automations yet. Ask btw in a chat, for example:</p>
					<ul class="list-disc space-y-1 pl-5">
						<li>"Every weekday at 7:30, check the weather and tell us if we need umbrellas."</li>
						<li>"Remind Anna tomorrow at 17:00 to pick up the parcel."</li>
						<li>"Check my email every 10 minutes and tell me when the school writes."</li>
					</ul>
				</div>
			{/each}
		</section>
	</div>
</div>
