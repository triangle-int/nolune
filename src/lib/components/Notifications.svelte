<script lang="ts">
	import { goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import type { NotificationItem } from '@btw/core';

	interface Props {
		items: NotificationItem[];
		/** When this user last opened the menu; newer items count as unread. */
		seenAt: number;
	}

	let { items, seenAt }: Props = $props();

	let open = $state(false);
	/** `seenAt` as it was when the menu opened, so new items stay marked while it's open. */
	let newAfter = $state(0);
	let expanded = $state<string | null>(null);
	let busy = $state<string | null>(null);
	let actionError = $state<string | null>(null);
	let container = $state<HTMLElement>();

	const unseen = $derived(items.filter((n) => n.createdAt > seenAt).length);

	$effect(() => {
		const source = new EventSource('/api/notifications/events');
		source.onmessage = () => invalidate('btw:notifications');
		return () => source.close();
	});

	async function post(path: string): Promise<Response> {
		const res = await fetch(`/api/notifications/${path}`, { method: 'POST' });
		if (!res.ok) {
			const body = await res.json().catch(() => null);
			actionError = body?.message ?? `Request failed (${res.status})`;
		}
		return res;
	}

	async function markSeen() {
		if (unseen === 0) return;
		await post('seen');
		await invalidate('btw:notifications');
	}

	function toggle() {
		if (open) return close();
		open = true;
		newAfter = seenAt;
		expanded = null;
		actionError = null;
		markSeen();
	}

	function close() {
		open = false;
		markSeen();
	}

	async function dismiss(id: string) {
		busy = id;
		await post(`${id}/dismiss`);
		await invalidate('btw:notifications');
		busy = null;
	}

	async function clearAll() {
		await post('clear');
		await invalidate('btw:notifications');
	}

	async function continueInChat(item: NotificationItem) {
		busy = item.id;
		actionError = null;
		const res = await post(`${item.id}/continue`);
		if (res.ok) {
			const target = (await res.json()) as { slug: string; conversationId: string };
			open = false;
			await goto(resolve('/p/[slug]/c/[id]', { slug: target.slug, id: target.conversationId }));
			await Promise.all([invalidate('btw:conversations'), invalidate('btw:notifications')]);
		}
		busy = null;
	}

	function ago(ms: number): string {
		const seconds = (Date.now() - ms) / 1000;
		if (seconds < 60) return 'just now';
		if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
		if (seconds < 86_400) return `${Math.floor(seconds / 3600)} h ago`;
		return new Date(ms).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
	}

	function onWindowClick(event: MouseEvent) {
		if (open && container && !container.contains(event.target as Node)) close();
	}
</script>

<svelte:window
	onclick={onWindowClick}
	onkeydown={(event) => {
		if (open && event.key === 'Escape') close();
	}}
/>

<div class="relative" bind:this={container}>
	<button
		onclick={toggle}
		class="relative flex items-center rounded-md p-1.5 text-stone-600 hover:bg-stone-100 hover:text-stone-900"
		aria-label={unseen ? `Notifications, ${unseen} new` : 'Notifications'}
		aria-expanded={open}
		title="Notifications"
	>
		<svg
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			stroke-width="1.8"
			stroke-linecap="round"
			stroke-linejoin="round"
			class="size-5"
			aria-hidden="true"
		>
			<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9" />
			<path d="M10.3 21a1.94 1.94 0 0 0 3.4 0" />
		</svg>
		{#if unseen}
			<span
				class="absolute -top-0.5 -right-0.5 min-w-4 rounded-full bg-red-600 px-1 text-center text-[10px] leading-4 font-semibold text-white"
			>
				{unseen > 9 ? '9+' : unseen}
			</span>
		{/if}
	</button>

	{#if open}
		<div
			class="fixed inset-x-2 top-12 z-30 flex max-h-[75vh] flex-col overflow-hidden rounded-xl border border-stone-200 bg-white text-stone-900 shadow-lg sm:absolute sm:inset-x-auto sm:top-10 sm:right-0 sm:w-96"
		>
			<div class="flex items-center justify-between border-b border-stone-200 px-4 py-2 text-sm">
				<span class="font-medium">Notifications</span>
				{#if items.length}
					<button onclick={clearAll} class="text-stone-500 hover:text-stone-900">Clear all</button>
				{/if}
			</div>
			<ul class="min-h-0 flex-1 divide-y divide-stone-100 overflow-y-auto">
				{#each items as item (item.id)}
					{@const isNew = item.createdAt > newAfter}
					<li class={['px-4 py-3 text-sm', isNew && 'bg-amber-50']}>
						<div class="flex items-start gap-2">
							{#if isNew}
								<span class="mt-1.5 size-2 shrink-0 rounded-full bg-red-600" title="New"></span>
							{/if}
							<button
								class="min-w-0 flex-1 text-left"
								onclick={() => (expanded = expanded === item.id ? null : item.id)}
								title={expanded === item.id ? 'Show less' : 'Show all'}
							>
								<span class="flex items-baseline gap-2">
									<span
										class={[
											'truncate font-medium',
											item.level === 'error' ? 'text-red-700' : 'text-stone-900'
										]}>{item.title}</span
									>
									<span class="ml-auto shrink-0 text-xs text-stone-400">{ago(item.createdAt)}</span>
								</span>
								<span
									class={[
										'mt-0.5 block whitespace-pre-wrap text-stone-600',
										expanded !== item.id && 'line-clamp-3'
									]}>{item.body}</span
								>
							</button>
							<button
								onclick={() => dismiss(item.id)}
								disabled={busy === item.id}
								class="-mt-0.5 shrink-0 rounded px-1 text-lg leading-none text-stone-400 hover:text-stone-900"
								aria-label="Dismiss"
								title="Dismiss">×</button
							>
						</div>
						<div class="mt-2 flex items-center gap-3 text-xs">
							{#if item.conversationId}
								<a
									href={resolve('/p/[slug]/c/[id]', {
										slug: item.profile.slug,
										id: item.conversationId
									})}
									onclick={() => (open = false)}
									class="font-medium text-stone-900 underline">Open chat</a
								>
							{:else}
								<button
									onclick={() => continueInChat(item)}
									disabled={busy === item.id}
									class="font-medium text-stone-900 underline disabled:opacity-50"
									>Continue in chat</button
								>
							{/if}
							<span class="truncate text-stone-400">{item.profile.name}</span>
						</div>
					</li>
				{:else}
					<li class="px-4 py-8 text-center text-sm text-stone-500">
						Nothing yet. Ask btw for a reminder or a daily check, and what it finds shows up here.
					</li>
				{/each}
			</ul>
			{#if actionError}
				<p class="border-t border-stone-200 px-4 py-2 text-sm text-red-600">{actionError}</p>
			{/if}
		</div>
	{/if}
</div>
