<script lang="ts">
	import { invalidate } from '$app/navigation';
	import type { DisplayBlock, LiveBlock } from '@btw/core';
	import { ChatState } from '$lib/chat.svelte';
	import { formatTokens } from '$lib/format';

	interface Props {
		conversation: {
			id: string;
			title: string;
			presetName: string;
			effort: string;
			contextWindow: number | null;
			/** A background run nobody has continued yet. */
			hidden: boolean;
		};
		efforts: string[];
		me: string;
	}

	let { conversation, efforts, me }: Props = $props();

	const chat = new ChatState();
	let text = $state('');
	let sending = $state(false);
	let actionError = $state<string | null>(null);
	let stickToBottom = $state(true);
	/** Sending a message turns a background run into a normal conversation. */
	let continued = $state(false);

	$effect(() => chat.connect(conversation.id));

	const title = $derived(
		conversation.title ||
			chat.messages.find((m) => m.kind === 'human')?.text.slice(0, 80) ||
			'New conversation'
	);

	const contextUsed = $derived.by(() => {
		for (let i = chat.messages.length - 1; i >= 0; i--) {
			const m = chat.messages[i];
			if (m.kind === 'assistant' && m.usage) {
				return m.usage.input + m.usage.cacheRead + m.usage.cacheWrite + m.usage.output;
			}
		}
		return null;
	});

	const unanswered = $derived(
		!chat.running &&
			chat.queued.length === 0 &&
			chat.messages.length > 0 &&
			chat.messages[chat.messages.length - 1].kind !== 'assistant'
	);

	const contentSize = $derived(
		chat.messages.length +
			chat.queued.length +
			chat.live.reduce((n, b) => n + (b?.text.length ?? 0), 0) +
			(chat.toolOutput?.text.length ?? 0)
	);

	/** Keeps the view pinned to the newest content unless the reader scrolled up. */
	function autoscroll(node: HTMLElement) {
		void contentSize;
		if (stickToBottom) node.scrollTop = node.scrollHeight;
	}

	function onScroll(event: Event & { currentTarget: HTMLElement }) {
		const node = event.currentTarget;
		stickToBottom = node.scrollHeight - node.scrollTop - node.clientHeight < 80;
	}

	async function post(path: string, body?: unknown): Promise<boolean> {
		actionError = null;
		const res = await fetch(`/api/c/${conversation.id}/${path}`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: body === undefined ? undefined : JSON.stringify(body)
		});
		if (!res.ok) actionError = (await res.text()) || `Request failed (${res.status})`;
		return res.ok;
	}

	async function send() {
		const message = text.trim();
		if (!message || sending) return;
		sending = true;
		stickToBottom = true;
		if (await post('messages', { text: message })) {
			text = '';
			continued = true;
			invalidate('btw:conversations');
		}
		sending = false;
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
			event.preventDefault();
			send();
		}
	}

	function firstLine(command: string): string {
		const line = command.split('\n')[0];
		return line.length > 120 ? line.slice(0, 120) + '…' : line;
	}

	function resultStatus(result: {
		output: string;
		isError: boolean;
	}): 'done' | 'failed' | 'stopped' {
		if (!result.isError) return 'done';
		return /Stopped by [^\n]*$/.test(result.output) ? 'stopped' : 'failed';
	}
</script>

{#snippet toolCard(id: string, command: string | null)}
	{@const result = chat.results[id]}
	{@const liveOutput = chat.toolOutput?.id === id ? chat.toolOutput.text : null}
	<details class="rounded-lg border border-stone-200 bg-stone-50 text-sm">
		<summary class="flex cursor-pointer items-center gap-2 px-3 py-2">
			<span class="min-w-0 flex-1 truncate font-mono text-xs text-stone-700">
				{command === null ? 'Preparing a command…' : `$ ${firstLine(command)}`}
			</span>
			{#if result}
				{@const status = resultStatus(result)}
				<span class={status === 'failed' ? 'text-red-600' : 'text-stone-400'}>{status}</span>
			{:else if liveOutput !== null}
				<span class="animate-pulse text-amber-600">running</span>
			{/if}
		</summary>
		<div class="border-t border-stone-200">
			{#if command && command.includes('\n')}
				<pre class="overflow-x-auto px-3 py-2 font-mono text-xs text-stone-700">{command}</pre>
			{/if}
			<pre
				class="max-h-80 overflow-auto px-3 py-2 font-mono text-xs whitespace-pre-wrap text-stone-600">{result?.output ??
					liveOutput ??
					''}</pre>
		</div>
	</details>
{/snippet}

{#snippet blocks(items: (DisplayBlock | LiveBlock | null)[])}
	{#each items as block, i (i)}
		{#if block?.type === 'text'}
			<div class="leading-relaxed whitespace-pre-wrap">{block.text}</div>
		{:else if block?.type === 'thinking' && block.text.trim()}
			<div class="text-sm whitespace-pre-wrap text-stone-500 italic">{block.text}</div>
		{:else if block?.type === 'tool'}
			{#if 'command' in block}
				{@render toolCard(block.id, block.command)}
			{:else if block.id}
				{@render toolCard(block.id, null)}
			{/if}
		{/if}
	{/each}
{/snippet}

<div class="flex h-full flex-col">
	<header class="flex items-center gap-3 border-b border-stone-200 bg-white px-4 py-2 text-sm">
		<h1 class="min-w-0 flex-1 truncate font-medium">{title}</h1>
		<span class="hidden text-stone-500 sm:inline">{conversation.presetName}</span>
		{#if contextUsed !== null}
			<span class="text-stone-400" title="Context used by the last reply">
				{formatTokens(contextUsed)} / {formatTokens(conversation.contextWindow)}
			</span>
		{/if}
		<label
			class="flex items-center gap-1 text-stone-500"
			title="Changing this makes the next reply re-read the whole conversation once (slower and costlier)."
		>
			Reasoning
			<select
				value={conversation.effort}
				onchange={(event) => post('effort', { effort: event.currentTarget.value })}
				class="rounded border border-stone-300 bg-white px-1 py-0.5"
			>
				{#each efforts as level (level)}
					<option value={level}>{level}</option>
				{/each}
			</select>
		</label>
		<form
			method="POST"
			action="?/delete"
			onsubmit={(event) => {
				if (!confirm('Delete this conversation for everyone in the profile?'))
					event.preventDefault();
			}}
		>
			<button class="text-stone-400 hover:text-red-600" title="Delete conversation">Delete</button>
		</form>
	</header>

	{#if conversation.hidden && !continued}
		<p class="border-b border-amber-200 bg-amber-50 px-4 py-2 text-center text-sm text-amber-800">
			A background run from an automation. It isn't in your conversation list; sending a message
			adds it there.
		</p>
	{/if}

	<div {@attach autoscroll} onscroll={onScroll} class="min-h-0 flex-1 overflow-y-auto">
		<div class="mx-auto max-w-3xl space-y-4 px-4 py-6">
			{#if !chat.loaded}
				<p class="text-center text-sm text-stone-400">Loading…</p>
			{:else if chat.messages.length === 0 && chat.queued.length === 0}
				<p class="text-center text-sm text-stone-400">Ask for something to get started.</p>
			{/if}

			{#each chat.messages as message (message.id)}
				{#if message.kind === 'human'}
					{@const mine = message.senderName === me}
					<div class={['flex', mine ? 'justify-end' : 'justify-start']}>
						<div
							class={[
								'max-w-[85%] rounded-2xl px-4 py-2',
								mine ? 'bg-stone-900 text-white' : 'border border-stone-200 bg-white'
							]}
						>
							{#if !mine}
								<div class="text-xs font-medium text-stone-500">{message.senderName}</div>
							{/if}
							<div class="whitespace-pre-wrap">{message.text}</div>
						</div>
					</div>
				{:else if message.kind === 'trigger'}
					<div class="rounded-lg border border-amber-200 bg-amber-50 px-4 py-2 text-sm">
						<div class="text-xs font-medium text-amber-800">Automation · {message.title}</div>
						<div class="whitespace-pre-wrap text-stone-700">{message.text}</div>
					</div>
				{:else if message.kind === 'assistant'}
					<div class="space-y-2">
						{@render blocks(message.blocks)}
						{#if message.stopReason === 'max_tokens'}
							<p class="text-sm text-amber-700">The reply was cut off because it got too long.</p>
						{:else if message.stopReason === 'refusal'}
							<p class="text-sm text-amber-700">The model declined to continue this request.</p>
						{/if}
					</div>
				{/if}
			{/each}

			{#if chat.live.length}
				<div class="space-y-2">{@render blocks(chat.live)}</div>
			{:else if chat.running && !chat.toolOutput}
				<p class="animate-pulse text-sm text-stone-400">Thinking…</p>
			{/if}

			{#each chat.queued as message (message.id)}
				{#if message.kind === 'human'}
					<div class="flex justify-end opacity-60">
						<div class="max-w-[85%] rounded-2xl border border-dashed border-stone-300 px-4 py-2">
							<div class="text-xs text-stone-500">
								{message.senderName} · waiting for the next step
							</div>
							<div class="whitespace-pre-wrap">{message.text}</div>
						</div>
					</div>
				{/if}
			{/each}

			{#if chat.error}
				<div
					class="flex items-center gap-3 rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-800"
				>
					<span class="flex-1">{chat.error}</span>
					<button class="font-medium underline" onclick={() => post('continue')}>Try again</button>
				</div>
			{:else if unanswered}
				<div class="text-center text-sm text-stone-500">
					Not answered yet. <button class="underline" onclick={() => post('continue')}
						>Continue</button
					>
				</div>
			{/if}
		</div>
	</div>

	<form
		class="border-t border-stone-200 bg-white p-3"
		onsubmit={(event) => {
			event.preventDefault();
			send();
		}}
	>
		<div class="mx-auto flex max-w-3xl items-end gap-2">
			<textarea
				bind:value={text}
				onkeydown={onKeydown}
				rows="2"
				placeholder={chat.running ? 'Add something while it works…' : 'Message'}
				class="min-h-11 flex-1 resize-y rounded-lg border border-stone-300 px-3 py-2 focus:border-stone-500 focus:outline-none"
			></textarea>
			{#if chat.running}
				<button
					type="button"
					onclick={() => post('stop')}
					class="rounded-lg border border-stone-300 px-4 py-2 text-sm hover:bg-stone-50"
				>
					Stop
				</button>
			{/if}
			<button
				disabled={!text.trim() || sending}
				class="rounded-lg bg-stone-900 px-4 py-2 text-sm font-medium text-white hover:bg-stone-700 disabled:opacity-40"
			>
				Send
			</button>
		</div>
		{#if actionError}
			<p class="mx-auto mt-2 max-w-3xl text-sm text-red-600">{actionError}</p>
		{:else if !chat.connected && chat.loaded}
			<p class="mx-auto mt-2 max-w-3xl text-sm text-amber-700">Reconnecting…</p>
		{/if}
	</form>
</div>
