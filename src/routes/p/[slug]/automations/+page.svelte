<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import type { SubmitFunction } from '@sveltejs/kit';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import WebhookIcon from '@lucide/svelte/icons/webhook';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Textarea } from '$lib/components/ui/textarea';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import PageHeader from '$lib/components/PageHeader.svelte';
	import { getPreferences } from '$lib/preferences.svelte';
	import { cn } from '$lib/utils';

	let { data, form } = $props();

	const prefs = getPreferences();

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

	// Keep what's typed in the prompt boxes; the reloaded data fills them in anyway.
	const keepFields: SubmitFunction =
		() =>
		async ({ update }) =>
			update({ reset: false });
</script>

<PageHeader>
	<span class="truncate text-lg font-medium">Automations</span>
</PageHeader>

<div class="min-h-0 flex-1 overflow-y-auto">
	<div class="mx-auto max-w-2xl space-y-6 px-4 py-6 sm:py-10">
		<p class="text-muted-foreground">
			Reminders, schedules and webhooks that run btw in the background. What they find shows up
			under the bell. To add one, just ask btw in a chat. Times are in the computer's time zone ({data.timeZone}).
		</p>

		{#if form?.message}
			<p class="rounded-2xl bg-muted px-4 py-3 text-sm">{form.message}</p>
		{/if}

		{#each data.triggers as t (t.id)}
			<section class="space-y-4 rounded-3xl border p-4 text-sm sm:p-5">
				<div class="flex items-start gap-3">
					<span
						class="flex size-9 shrink-0 items-center justify-center rounded-xl bg-muted text-muted-foreground"
					>
						{#if t.webhookUrl}<WebhookIcon class="size-4" />{:else}<ClockIcon class="size-4" />{/if}
					</span>
					<div class="min-w-0 flex-1">
						<h2 class="font-medium">{t.name}</h2>
						<p class="text-muted-foreground">
							{t.when}{#if t.next}&nbsp;· next {t.next}{/if}
						</p>
					</div>
					{#if t.state !== 'on'}
						<Badge variant="secondary">{t.state}</Badge>
					{/if}
				</div>

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

				<form method="POST" action="?/edit" use:enhance={keepFields} class="space-y-3">
					<input type="hidden" name="id" value={t.id} />
					<label class="block space-y-1.5">
						<span class="text-xs text-muted-foreground">
							{#if t.action === 'agent'}
								What btw does{prefs.technical ? ` (${t.model}, reasoning ${t.effort})` : ''}
							{:else}
								Script: runs without the model and calls <code>btw wake</code> when btw is needed
							{/if}
						</span>
						<Textarea
							name="text"
							rows={3}
							value={t.text}
							class={cn('rounded-2xl', t.action === 'script' && 'font-mono text-xs')}
						/>
					</label>
					<div class="flex flex-wrap gap-2">
						<Button type="submit" variant="outline" size="sm">Save</Button>
						<Button type="submit" formaction="?/run" variant="outline" size="sm">Run now</Button>
						{#if t.state !== 'done'}
							<Button type="submit" formaction="?/toggle" variant="outline" size="sm"
								>{t.state === 'paused' ? 'Resume' : 'Pause'}</Button
							>
						{/if}
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
			</section>
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
	</div>
</div>
