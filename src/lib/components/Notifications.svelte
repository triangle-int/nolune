<script lang="ts">
	import { goto, invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import type { NotificationItem } from '@btw/core';
	import BellIcon from '@lucide/svelte/icons/bell';
	import XIcon from '@lucide/svelte/icons/x';
	import * as Popover from '$lib/components/ui/popover';
	import { cn } from '$lib/utils';

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

	function onOpenChange(value: boolean) {
		if (value) {
			newAfter = seenAt;
			expanded = null;
			actionError = null;
		}
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
</script>

<Popover.Root bind:open {onOpenChange}>
	<Popover.Trigger
		class="relative flex size-10 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground aria-expanded:bg-muted aria-expanded:text-foreground"
		aria-label={unseen ? `Notifications, ${unseen} new` : 'Notifications'}
	>
		<BellIcon class="size-5" />
		{#if unseen}
			<span
				class="absolute top-1 right-1 min-w-4 rounded-full bg-red-600 px-1 text-center text-[10px] leading-4 font-semibold text-white"
			>
				{unseen > 9 ? '9+' : unseen}
			</span>
		{/if}
	</Popover.Trigger>
	<Popover.Content
		align="end"
		collisionPadding={8}
		class="flex max-h-[min(75vh,36rem)] w-[calc(100vw-1rem)] flex-col gap-0 overflow-hidden p-0 sm:w-96"
	>
		<div class="flex items-center justify-between px-4 pt-3 pb-2">
			<span class="font-semibold">Notifications</span>
			{#if items.length}
				<button onclick={clearAll} class="text-sm text-muted-foreground hover:text-foreground">
					Clear all
				</button>
			{/if}
		</div>
		<ul class="min-h-0 flex-1 space-y-1 overflow-y-auto px-1.5 pb-1.5">
			{#each items as item (item.id)}
				{@const isNew = item.createdAt > newAfter}
				<li class={cn('rounded-2xl px-3 py-2.5 text-sm', isNew ? 'bg-muted' : 'hover:bg-muted/60')}>
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
									class={cn('truncate font-medium', item.level === 'error' && 'text-destructive')}
									>{item.title}</span
								>
								<span class="ml-auto shrink-0 text-xs text-muted-foreground"
									>{ago(item.createdAt)}</span
								>
							</span>
							<span
								class={cn(
									'mt-0.5 block whitespace-pre-wrap text-muted-foreground',
									expanded !== item.id && 'line-clamp-3'
								)}>{item.body}</span
							>
						</button>
						<button
							onclick={() => dismiss(item.id)}
							disabled={busy === item.id}
							class="-mt-0.5 -mr-1 flex size-6 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-background hover:text-foreground"
							aria-label="Dismiss"
							title="Dismiss"
						>
							<XIcon class="size-3.5" />
						</button>
					</div>
					<div class="mt-2 flex items-center gap-3 text-xs">
						{#if item.conversationId}
							<a
								href={resolve('/p/[slug]/c/[id]', {
									slug: item.profile.slug,
									id: item.conversationId
								})}
								onclick={() => (open = false)}
								class="rounded-full border bg-background px-3 py-1 font-medium hover:bg-muted"
								>Open chat</a
							>
						{:else}
							<button
								onclick={() => continueInChat(item)}
								disabled={busy === item.id}
								class="rounded-full border bg-background px-3 py-1 font-medium hover:bg-muted disabled:opacity-50"
								>Continue in chat</button
							>
						{/if}
						<span class="truncate text-muted-foreground">{item.profile.name}</span>
					</div>
				</li>
			{:else}
				<li class="px-4 py-8 text-center text-sm text-muted-foreground">
					Nothing yet. Ask btw for a reminder or a daily check, and what it finds shows up here.
				</li>
			{/each}
		</ul>
		{#if actionError}
			<p class="border-t px-4 py-2 text-sm text-destructive">{actionError}</p>
		{/if}
	</Popover.Content>
</Popover.Root>
