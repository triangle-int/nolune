<script lang="ts">
	import { enhance } from '$app/forms';
	import { resolve } from '$app/paths';
	import type { SubmitFunction } from '@sveltejs/kit';

	let { data, form } = $props();

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

<main class="mx-auto h-full max-w-2xl space-y-6 overflow-y-auto p-6">
	<div class="space-y-1">
		<h1 class="text-xl font-semibold">Automations</h1>
		<p class="text-sm text-stone-600">
			Reminders, schedules and webhooks that run btw in the background. What they find shows up
			under the bell at the top. To add one, ask btw in a conversation. Times are in the computer's
			time zone ({data.timeZone}).
		</p>
	</div>

	{#if form?.message}
		<p class="rounded-md bg-stone-100 px-3 py-2 text-sm">{form.message}</p>
	{/if}

	{#each data.triggers as t (t.id)}
		<section class="space-y-3 rounded-xl border border-stone-200 bg-white p-4 text-sm">
			<div class="flex items-start gap-2">
				<div class="min-w-0 flex-1">
					<h2 class="font-medium">{t.name}</h2>
					<p class="text-stone-500">
						{t.when}{#if t.next}&nbsp;· next {t.next}{/if}
					</p>
				</div>
				{#if t.state !== 'on'}
					<span class="rounded-full bg-stone-100 px-2 py-0.5 text-xs text-stone-600">{t.state}</span
					>
				{/if}
			</div>

			{#if t.webhookUrl}
				<label class="block space-y-1">
					<span class="text-xs text-stone-500"
						>Webhook URL. Keep it secret: anyone with it can start a run. POST JSON to it.</span
					>
					<input
						readonly
						value={t.webhookUrl}
						onfocus={(event) => event.currentTarget.select()}
						class="w-full rounded-md border border-stone-300 bg-stone-50 px-3 py-1.5 font-mono text-xs"
					/>
				</label>
			{/if}

			<form method="POST" action="?/edit" use:enhance={keepFields} class="space-y-2">
				<input type="hidden" name="id" value={t.id} />
				<label class="block space-y-1">
					<span class="text-xs text-stone-500">
						{#if t.action === 'agent'}
							What btw does ({t.model}, reasoning {t.effort})
						{:else}
							Script: runs without the model and calls <code>btw wake</code> when btw is needed
						{/if}
					</span>
					<textarea
						name="text"
						rows="3"
						value={t.text}
						class={[
							'w-full resize-y rounded-md border border-stone-300 px-3 py-2 focus:border-stone-500 focus:outline-none',
							t.action === 'script' && 'font-mono text-xs'
						]}></textarea>
				</label>
				<div class="flex flex-wrap gap-2">
					<button class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-50"
						>Save</button
					>
					<button
						formaction="?/run"
						class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-50">Run now</button
					>
					{#if t.state !== 'done'}
						<button
							formaction="?/toggle"
							class="rounded-md border border-stone-300 px-3 py-1.5 hover:bg-stone-50"
							>{t.state === 'paused' ? 'Resume' : 'Pause'}</button
						>
					{/if}
					<button
						formaction="?/remove"
						onclick={(event) => {
							if (!confirm(`Delete "${t.name}"?`)) event.preventDefault();
						}}
						class="ml-auto rounded-md px-3 py-1.5 text-stone-500 hover:text-red-600">Delete</button
					>
				</div>
			</form>

			{#if t.runs.length}
				<ul class="space-y-1 border-t border-stone-100 pt-3">
					{#each t.runs as run (run.id)}
						<li class="flex flex-wrap items-baseline gap-x-2 text-xs">
							<span class="text-stone-400">{run.at}</span>
							<span class={run.status === 'failed' ? 'text-red-600' : 'text-stone-700'}>
								{run.action === 'script' ? 'script ' : ''}{STATUS[run.status]}
							</span>
							<span class="text-stone-400">{SOURCE[run.source]}</span>
							{#if run.conversationId}
								<a
									href={resolve('/p/[slug]/c/[id]', {
										slug: data.profile.slug,
										id: run.conversationId
									})}
									class="text-stone-500 underline hover:text-stone-900">view</a
								>
							{/if}
							{#if run.output}
								<details class="w-full">
									<summary class="cursor-pointer text-stone-500">output</summary>
									<pre
										class="mt-1 max-h-60 overflow-auto rounded bg-stone-50 p-2 font-mono whitespace-pre-wrap text-stone-600">{run.output}</pre>
								</details>
							{/if}
						</li>
					{/each}
				</ul>
			{:else}
				<p class="border-t border-stone-100 pt-3 text-xs text-stone-400">No runs yet.</p>
			{/if}
		</section>
	{:else}
		<div
			class="space-y-2 rounded-xl border border-dashed border-stone-300 p-6 text-sm text-stone-600"
		>
			<p>No automations yet. Ask btw in a conversation, for example:</p>
			<ul class="list-disc space-y-1 pl-5">
				<li>"Every weekday at 7:30, check the weather and tell us if we need umbrellas."</li>
				<li>"Remind Anna tomorrow at 17:00 to pick up the parcel."</li>
				<li>"Check my email every 10 minutes and tell me when the school writes."</li>
			</ul>
		</div>
	{/each}
</main>
