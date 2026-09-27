<script lang="ts">
	import { untrack } from 'svelte';
	import { enhance } from '$app/forms';
	import { invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import type { DisplayAttachment, Usage } from '@btw/core';
	import type { Avatar } from '@btw/core/avatars';
	import { cacheHitRate, cacheMissTokens, cacheTtlMs, promptTokens } from '@btw/core/usage';
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import BotIcon from '@lucide/svelte/icons/bot';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import InfoIcon from '@lucide/svelte/icons/info';
	import LoaderIcon from '@lucide/svelte/icons/loader-circle';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import * as AlertDialog from '$lib/components/ui/alert-dialog';
	import * as Collapsible from '$lib/components/ui/collapsible';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { Button } from '$lib/components/ui/button';
	import { ChatState } from '$lib/chat.svelte';
	import { firstLine } from '$lib/commands';
	import { moveChat, type FolderItem } from '$lib/folders';
	import { formatPercent, formatTokens } from '$lib/format';
	import { getPreferences } from '$lib/preferences.svelte';
	import { activeStepLabel, buildTranscript, replyText, type Reply } from '$lib/transcript';
	import { Attachments } from '$lib/uploads.svelte';
	import { cn } from '$lib/utils';
	import AssistantAvatar, { type Mood } from './AssistantAvatar.svelte';
	import Activity from './chat/Activity.svelte';
	import Composer from './chat/Composer.svelte';
	import ComposerDock from './chat/ComposerDock.svelte';
	import CopyButton from './chat/CopyButton.svelte';
	import MoveToFolderMenu from './folders/MoveToFolderMenu.svelte';
	import NewFolderDialog from './folders/NewFolderDialog.svelte';
	import Markdown from './chat/Markdown.svelte';
	import MediaViewer, { pictureClicks, type ViewedPicture } from './chat/MediaViewer.svelte';
	import MessageAttachments from './chat/MessageAttachments.svelte';
	import ModelMenu from './chat/ModelMenu.svelte';
	import RenameChatDialog from './chat/RenameChatDialog.svelte';
	import PageHeader from './PageHeader.svelte';
	import TypedText from './TypedText.svelte';
	import UserAvatar from './UserAvatar.svelte';

	interface Props {
		conversation: {
			id: string;
			title: string;
			presetName: string;
			effort: string;
			contextWindow: number | null;
			/** A background run nobody has continued yet. */
			hidden: boolean;
			cacheTtl: '5m' | '1h';
			/** A subagent's own chat: only the agent that started it writes here. */
			subagent: { name: string; parentId: string; parentTitle: string } | null;
		};
		efforts: string[];
		me: string;
		/** The profile's folders, and the one this chat is in. */
		folders: FolderItem[];
		folderId: string | null;
		/** The profile's assistant avatar, shown with every reply. */
		avatar: Avatar;
	}

	let { conversation, efforts, me, folders, folderId, avatar }: Props = $props();

	const prefs = getPreferences();
	const chat = new ChatState();
	let text = $state('');
	const attachments = new Attachments(() => page.params.slug ?? '');
	let sending = $state(false);
	let actionError = $state<string | null>(null);
	let stickToBottom = $state(true);
	/** Sending a message turns a background run into a normal conversation. */
	let continued = $state(false);
	let effort = $state(untrack(() => conversation.effort));
	let renaming = $state<{ id: string; title: string } | null>(null);
	let deleteOpen = $state(false);
	let creatingFolder = $state(false);
	const folder = $derived(folders.find((f) => f.id === folderId));
	let viewing = $state<ViewedPicture | null>(null);
	let scroller = $state<HTMLElement>();
	/** The messages scroll under the composer, so they end this far up. */
	let composerHeight = $state(160);
	let textarea = $state<HTMLTextAreaElement | null>(null);

	$effect(() => chat.connect(conversation.id));

	const title = $derived(
		chat.title ||
			conversation.title ||
			chat.messages.find((m) => m.kind === 'human')?.text.slice(0, 80) ||
			'New chat'
	);

	// btw names a chat shortly after its first message; the sidebar lists the title too.
	let listedTitle = untrack(() => conversation.title);
	$effect(() => {
		if (!chat.title || chat.title === listedTitle) return;
		listedTitle = chat.title;
		invalidate('btw:conversations');
	});

	const entries = $derived(buildTranscript(chat.messages, chat.live, chat.running));

	/** Usage of the last reply, and summed over the whole conversation. */
	const usage = $derived.by(() => {
		let last: Usage | null = null;
		const total: Usage = { input: 0, cacheRead: 0, cacheWrite: 0, output: 0 };
		for (const m of chat.messages) {
			if (m.kind !== 'assistant' || !m.usage) continue;
			last = m.usage;
			total.input += m.usage.input;
			total.cacheRead += m.usage.cacheRead;
			total.cacheWrite += m.usage.cacheWrite;
			total.output += m.usage.output;
		}
		return last && { last, total };
	});

	const contextUsed = $derived(usage && promptTokens(usage.last) + usage.last.output);

	/**
	 * Replies whose request processed again what the previous request had cached, keyed by id.
	 * `expired`: the conversation sat idle past the cache's lifetime before that request.
	 */
	const cacheMisses = $derived.by(() => {
		const misses: Record<number, { tokens: number; expired: boolean }> = {};
		let previous: { usage: Usage; at: number } | null = null;
		// When the rows that led to the next request (a message, command output) arrived.
		let resumedAt = 0;
		for (const m of chat.messages) {
			if (m.kind !== 'assistant') {
				resumedAt = Math.max(resumedAt, m.createdAt);
				continue;
			}
			if (!m.usage) continue;
			if (previous) {
				const tokens = cacheMissTokens(previous.usage, m.usage);
				const expired = resumedAt - previous.at > cacheTtlMs(conversation.cacheTtl);
				if (tokens > 0) misses[m.id] = { tokens, expired };
			}
			previous = { usage: m.usage, at: m.createdAt };
			resumedAt = 0;
		}
		return misses;
	});

	function cacheSummary(label: string, u: Usage): string {
		return `${label}: ${formatPercent(cacheHitRate(u))} cached (${formatTokens(u.cacheRead)} read, ${formatTokens(u.cacheWrite)} written, ${formatTokens(u.input)} uncached)`;
	}

	/** Cache misses within one reply, which may span several model calls. */
	function replyMiss(reply: Reply): { tokens: number; reason: string } | null {
		const misses = reply.messageIds.flatMap((id) => cacheMisses[id] ?? []);
		if (!misses.length) return null;
		return {
			tokens: misses.reduce((n, m) => n + m.tokens, 0),
			reason: misses.some((m) => m.expired)
				? `Over ${conversation.cacheTtl === '5m' ? '5 minutes' : 'an hour'} passed since the previous step, so the cached conversation expired and was processed again (slower and costlier).`
				: 'Context that should have come from the cache was processed again (slower and costlier). Changing the reasoning level, moving the chat to another folder or changing its folder cause this once.'
		};
	}

	const unanswered = $derived(
		!chat.running &&
			chat.queued.length === 0 &&
			chat.messages.length > 0 &&
			chat.messages[chat.messages.length - 1].kind !== 'assistant'
	);

	/** The newest entry when it's a reply: its avatar shows what btw is doing. Older ones hold still. */
	const liveReply = $derived.by(() => {
		const last = entries.at(-1);
		return last?.type === 'reply' ? last : null;
	});

	/** The step btw is on while it runs, in the words its group of steps uses. */
	const step = $derived.by(() => {
		const tail = chat.running ? liveReply?.parts.at(-1) : undefined;
		if (!tail || tail.type === 'text') {
			return chat.running ? { label: tail ? 'Writing' : 'Thinking', command: false } : null;
		}
		const last = tail.steps.at(-1);
		return {
			label: activeStepLabel(tail, chat.results, prefs.technical),
			// Running once the model has finished writing it, until its result arrives.
			command:
				last?.type === 'command' &&
				!chat.results[last.id] &&
				!chat.live.some((block) => block?.type === 'tool' && block.id === last.id)
		};
	});

	/** True for a moment after btw finishes a turn, for the avatar's happy squash. */
	let finished = $state(false);
	let wasRunning = false;
	$effect(() => {
		const running = chat.running;
		const justFinished = wasRunning && !running && !untrack(() => chat.error);
		wasRunning = running;
		if (running) finished = false;
		if (!justFinished) return;
		finished = true;
		const timer = setTimeout(() => (finished = false), 1200);
		return () => clearTimeout(timer);
	});

	const mood: Mood = $derived.by(() => {
		if (chat.error || unanswered) return 'blocked';
		if (chat.running) {
			if (chat.queued.length) return 'waiting';
			return step?.command ? 'working' : 'thinking';
		}
		return finished ? 'done' : 'idle';
	});

	/** How close to the end the chat has to be to count as scrolled to the bottom. */
	const BOTTOM_SLACK = 80;
	/** Where the view was, and how tall the chat, when last placed or scrolled. */
	let lastScrollTop = 0;
	let lastScrollHeight = 0;
	/** A pointer is down in the chat: dragging its scrollbar, or selecting text. */
	let pointerDown = false;

	/**
	 * Keeps the view pinned to the newest content while the reader is at the bottom, whenever
	 * anything changes size: new messages and streamed text, but also pictures that finish loading
	 * and the composer growing. Starting to scroll up (wheel, trackpad, finger or keys) lets go
	 * right away, before the view has moved far.
	 */
	function autoscroll(node: HTMLElement) {
		const observer = new ResizeObserver(() => {
			if (stickToBottom) node.scrollTop = node.scrollHeight;
			// Content that got shorter pulled the view up with it. Its scroll event only comes a
			// frame later, maybe after the content grew back, and must not read as the reader
			// scrolling up.
			lastScrollTop = node.scrollTop;
			lastScrollHeight = node.scrollHeight;
		});
		observer.observe(node);
		// The border box, so the padding the composer sets counts too.
		for (const child of node.children) observer.observe(child, { box: 'border-box' });

		const release = () => {
			if (node.scrollTop > 0) stickToBottom = false;
		};
		let touchY = 0;
		const onWheel = (event: WheelEvent) => {
			if (event.deltaY < 0) release();
		};
		const onTouchStart = (event: TouchEvent) => (touchY = event.touches[0]?.clientY ?? 0);
		const onTouchMove = (event: TouchEvent) => {
			const y = event.touches[0]?.clientY ?? touchY;
			if (y > touchY) release(); // a finger moving down scrolls up
			touchY = y;
		};
		const onKeyDown = (event: KeyboardEvent) => {
			if (event.defaultPrevented || !['ArrowUp', 'PageUp', 'Home'].includes(event.key)) return;
			// Not in the composer or a menu, where the key does something else.
			const target = event.target as Node;
			if (target === document.body || node.contains(target)) release();
		};
		const onPointerDown = () => (pointerDown = true);
		const onPointerUp = () => (pointerDown = false);
		node.addEventListener('wheel', onWheel, { passive: true });
		node.addEventListener('touchstart', onTouchStart, { passive: true });
		node.addEventListener('touchmove', onTouchMove, { passive: true });
		node.addEventListener('pointerdown', onPointerDown);
		window.addEventListener('pointerup', onPointerUp);
		window.addEventListener('pointercancel', onPointerUp);
		window.addEventListener('keydown', onKeyDown);
		return () => {
			observer.disconnect();
			node.removeEventListener('wheel', onWheel);
			node.removeEventListener('touchstart', onTouchStart);
			node.removeEventListener('touchmove', onTouchMove);
			node.removeEventListener('pointerdown', onPointerDown);
			window.removeEventListener('pointerup', onPointerUp);
			window.removeEventListener('pointercancel', onPointerUp);
			window.removeEventListener('keydown', onKeyDown);
		};
	}

	/**
	 * Only the reader decides: scrolling down to the end sticks to the bottom, scrolling up lets
	 * go. When the browser moves the view by itself (content changing size, focus), the choice
	 * stays, so the chat never jumps to the bottom on its own.
	 */
	function onScroll(event: Event & { currentTarget: HTMLElement }) {
		const node = event.currentTarget;
		const top = node.scrollTop;
		const atBottom = node.scrollHeight - top - node.clientHeight < BOTTOM_SLACK;
		if (top > lastScrollTop && atBottom) stickToBottom = true;
		else if (top < lastScrollTop && !atBottom && stickToBottom) {
			// The chat changed size since the view was placed: it got shorter, which pulled the view
			// up, and grew again before this event (Safari and Firefox can lay it out in between).
			// That was the browser, not the reader, so back to the end.
			if (node.scrollHeight !== lastScrollHeight && !pointerDown)
				node.scrollTop = node.scrollHeight;
			else stickToBottom = false;
		}
		lastScrollTop = node.scrollTop;
		lastScrollHeight = node.scrollHeight;
	}

	/** Follows the newest content from now on, even what arrives while the view is on its way. */
	function scrollToBottom() {
		stickToBottom = true;
		scroller?.scrollTo({ top: scroller.scrollHeight, behavior: 'smooth' });
	}

	async function post(path: string, body?: unknown): Promise<boolean> {
		actionError = null;
		const res = await fetch(`/api/c/${conversation.id}/${path}`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: body === undefined ? undefined : JSON.stringify(body)
		});
		if (!res.ok) {
			const body = await res.text();
			let message = body;
			try {
				message = (JSON.parse(body) as { message?: string }).message ?? body;
			} catch {
				// plain text
			}
			actionError = message || `Request failed (${res.status})`;
		}
		return res.ok;
	}

	async function send() {
		const message = text.trim();
		const uploads = attachments.ids;
		if ((!message && !uploads.length) || sending || attachments.uploading) return;
		sending = true;
		stickToBottom = true;
		if (await post('messages', { text: message, uploads })) {
			text = '';
			attachments.clear();
			continued = true;
			invalidate('btw:conversations');
		}
		sending = false;
		textarea?.focus();
	}

	async function move(target: string | null) {
		actionError = null;
		try {
			await moveChat(conversation.id, target);
		} catch (err) {
			actionError = err instanceof Error ? err.message : String(err);
		}
	}

	async function changeEffort(value: string) {
		const previous = effort;
		effort = value;
		if (!(await post('effort', { effort: value }))) effort = previous;
	}
</script>

{#snippet humanBubble(
	senderName: string,
	body: string,
	files: DisplayAttachment[],
	pending: boolean
)}
	{@const mine = senderName === me}
	<div class="group/human flex flex-col items-end gap-1">
		{#if !mine || pending}
			<div class="flex items-center gap-1.5 px-1 text-xs text-muted-foreground">
				{#if pending}
					<ClockIcon class="size-3" />
					{mine ? '' : `${senderName} · `}btw reads this after its current step
				{:else}
					<UserAvatar name={senderName} class="size-4 text-[9px]" />
					{senderName}
				{/if}
			</div>
		{/if}
		{#if files.length}
			<MessageAttachments conversationId={conversation.id} attachments={files} />
		{/if}
		{#if body}
			<div
				class={cn(
					'max-w-[85%] rounded-[22px] px-4 py-2.5 leading-relaxed break-words whitespace-pre-wrap sm:max-w-[70%]',
					pending ? 'border border-dashed opacity-70' : 'bg-bubble'
				)}
			>
				{body}
			</div>
		{/if}
		{#if !pending && body}
			<div
				class="-mr-1.5 opacity-100 transition-opacity md:opacity-0 md:group-hover/human:opacity-100"
			>
				<CopyButton text={body} />
			</div>
		{/if}
	</div>
{/snippet}

<!-- In the margin left of the reply when the chat is wide enough, else on a line of its own. -->
{#snippet assistant(avatarMood: Mood | undefined, label?: string)}
	<AssistantAvatar
		{avatar}
		mood={avatarMood}
		{label}
		size={24}
		class="@min-[54rem]/chat:absolute @min-[54rem]/chat:top-0.5 @min-[54rem]/chat:-left-11"
	/>
{/snippet}

{#snippet reply(r: Reply, last: boolean)}
	{@const copyable = replyText(r)}
	{@const miss = prefs.technical ? replyMiss(r) : null}
	{@const tail = r.parts.at(-1)}
	<div class="group/reply relative flex flex-col gap-3">
		{@render assistant(last ? mood : undefined, last ? step?.label : undefined)}
		{#each r.parts as part, i (part.key)}
			{#if part.type === 'text'}
				<Markdown
					text={part.text}
					media={{ conversationId: conversation.id, media: part.media, pending: part.pending }}
				/>
			{:else}
				<Activity
					{part}
					results={chat.results}
					toolOutput={chat.toolOutput}
					active={r.live && i === r.parts.length - 1}
					running={chat.running}
				/>
			{/if}
		{/each}

		{#if r.live && (!tail || (tail.type === 'text' && chat.live.every((b) => !b)))}
			<span class="my-1 block size-3.5 animate-pulse rounded-full bg-foreground" role="status">
				<span class="sr-only">btw is working</span>
			</span>
		{/if}

		{#if r.stopReasons.includes('max_tokens')}
			<p class="text-sm text-warning">The reply was cut off because it got too long.</p>
		{/if}
		{#if r.stopReasons.includes('refusal')}
			<p class="text-sm text-warning">btw declined to continue this request.</p>
		{/if}

		{#if !r.live && (copyable || (prefs.technical && r.usage))}
			<div
				class={cn(
					'-mt-1 -ml-1.5 flex items-center gap-0.5 transition-opacity',
					!last && 'md:opacity-0 md:group-hover/reply:opacity-100 md:focus-within:opacity-100'
				)}
			>
				{#if copyable}
					<CopyButton text={copyable} />
				{/if}
				{#if prefs.technical && r.usage}
					<Tooltip.Root>
						<Tooltip.Trigger
							class="flex size-8 items-center justify-center rounded-lg text-muted-foreground hover:bg-muted hover:text-foreground"
							aria-label="Usage"
						>
							<InfoIcon class="size-4" />
						</Tooltip.Trigger>
						<Tooltip.Content class="max-w-xs flex-col items-start gap-0.5">
							<span
								>{formatTokens(promptTokens(r.usage))} tokens in, {formatTokens(r.usage.output)} out</span
							>
							<span>{cacheSummary('Cache', r.usage)}</span>
						</Tooltip.Content>
					</Tooltip.Root>
				{/if}
				{#if miss}
					<Tooltip.Root>
						<Tooltip.Trigger class="px-1.5 text-xs text-warning">
							Cache miss · {formatTokens(miss.tokens)} tokens processed again
						</Tooltip.Trigger>
						<Tooltip.Content class="max-w-xs">{miss.reason}</Tooltip.Content>
					</Tooltip.Root>
				{/if}
			</div>
		{/if}
	</div>
{/snippet}

<PageHeader>
	{#if folder}
		<a
			href={resolve('/p/[slug]/f/[folder]', { slug: page.params.slug ?? '', folder: folder.id })}
			class="hidden max-w-48 shrink-0 items-center gap-1.5 truncate rounded-lg text-base text-muted-foreground hover:text-foreground sm:flex sm:text-lg"
		>
			<FolderIcon class="size-4 shrink-0" />
			<span class="truncate">{folder.name}</span>
		</a>
		<span class="hidden text-muted-foreground sm:inline">/</span>
	{/if}
	<h1 class="min-w-0 truncate text-base font-medium sm:text-lg"><TypedText text={title} /></h1>
	{#if prefs.technical && usage}
		<Tooltip.Root>
			<Tooltip.Trigger
				class="hidden shrink-0 rounded-full bg-muted px-2.5 py-1 text-xs text-muted-foreground sm:block"
			>
				{formatTokens(contextUsed)} / {formatTokens(conversation.contextWindow)} · {formatPercent(
					cacheHitRate(usage.last)
				)} cached
			</Tooltip.Trigger>
			<Tooltip.Content class="max-w-sm flex-col items-start gap-0.5">
				<span
					>Context used by the last reply: {formatTokens(contextUsed)} of {formatTokens(
						conversation.contextWindow
					)}</span
				>
				<span>{cacheSummary('Last reply', usage.last)}</span>
				<span>{cacheSummary('Whole conversation', usage.total)}</span>
			</Tooltip.Content>
		</Tooltip.Root>
	{/if}

	{#snippet actions()}
		<DropdownMenu.Root>
			<DropdownMenu.Trigger
				class="flex size-10 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground aria-expanded:bg-muted"
				aria-label="Chat options"
			>
				<EllipsisIcon class="size-5" />
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="end" class="w-60">
				{#if prefs.technical}
					<DropdownMenu.Label class="font-normal">
						<span class="block truncate text-foreground">{conversation.presetName}</span>
						{#if usage}
							<span class="block"
								>Context {formatTokens(contextUsed)} / {formatTokens(
									conversation.contextWindow
								)}</span
							>
							<span class="block">{formatPercent(cacheHitRate(usage.total))} cached overall</span>
						{/if}
					</DropdownMenu.Label>
					<DropdownMenu.Separator />
				{/if}
				<DropdownMenu.Item onSelect={() => (renaming = { id: conversation.id, title })}>
					<PencilIcon />
					Rename
				</DropdownMenu.Item>
				<MoveToFolderMenu
					{folders}
					{folderId}
					onmove={move}
					onnew={() => (creatingFolder = true)}
				/>
				<DropdownMenu.Separator />
				<DropdownMenu.Item variant="destructive" onSelect={() => (deleteOpen = true)}>
					<Trash2Icon />
					Delete
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{/snippet}
</PageHeader>

{#if conversation.subagent}
	<div class="mx-auto w-full max-w-3xl px-4">
		<p class="rounded-2xl bg-muted px-4 py-2 text-center text-sm text-muted-foreground">
			Subagent {conversation.subagent.name}: btw started it from
			<a
				href={resolve('/p/[slug]/c/[id]', {
					slug: page.params.slug ?? '',
					id: conversation.subagent.parentId
				})}
				class="text-foreground underline underline-offset-2">{conversation.subagent.parentTitle}</a
			>, and it reports back there.
		</p>
	</div>
{:else if conversation.hidden && !continued}
	<div class="mx-auto w-full max-w-3xl px-4">
		<p class="rounded-2xl bg-muted px-4 py-2 text-center text-sm text-muted-foreground">
			A background run from an automation. Send a message to keep it in your chats.
		</p>
	</div>
{/if}

<div class="relative min-h-0 flex-1">
	<div
		bind:this={scroller}
		{@attach autoscroll}
		{@attach pictureClicks((picture) => (viewing = picture))}
		onscroll={onScroll}
		class="@container/chat h-full overflow-y-auto [overflow-anchor:none]"
	>
		<div
			class="mx-auto flex max-w-3xl flex-col gap-7 px-4 pt-4 sm:px-6"
			style:padding-bottom="{composerHeight + 16}px"
		>
			{#if chat.loaded && chat.messages.length === 0 && chat.queued.length === 0 && !chat.running}
				<p class="py-16 text-center text-muted-foreground">Ask for something to get started.</p>
			{/if}

			{#each entries as entry, index (entry.key)}
				{#if entry.type === 'human'}
					{@render humanBubble(
						entry.message.senderName,
						entry.message.text,
						entry.message.attachments,
						false
					)}
				{:else if entry.type === 'trigger'}
					<div class="rounded-2xl border px-4 py-3 text-sm">
						<div class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
							<ClockIcon class="size-3.5" />
							Automation · {entry.message.title}
						</div>
						<div class="mt-1.5 leading-relaxed whitespace-pre-wrap">{entry.message.text}</div>
					</div>
				{:else if entry.type === 'agent_message'}
					<div class="rounded-2xl border px-4 py-3 text-sm">
						<div class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
							<BotIcon class="size-3.5" />
							From btw, to {entry.message.title}
						</div>
						<div class="mt-1.5 leading-relaxed whitespace-pre-wrap">{entry.message.text}</div>
					</div>
				{:else if entry.type === 'task_result'}
					<Collapsible.Root class="rounded-2xl border px-4 py-2.5 text-sm">
						<Collapsible.Trigger
							class="group/result flex w-full min-w-0 items-center gap-1.5 text-left text-xs font-medium text-muted-foreground hover:text-foreground"
						>
							<SquareTerminalIcon class="size-3.5 shrink-0" />
							<span class="min-w-0 truncate"
								>Finished in the background · {entry.message.title}</span
							>
							{#if entry.message.isError}
								<span class="shrink-0 text-destructive">
									{prefs.technical ? 'failed' : "didn't work"}
								</span>
							{/if}
							<ChevronRightIcon
								class="size-3.5 shrink-0 transition-transform group-data-[state=open]/result:rotate-90"
							/>
						</Collapsible.Trigger>
						<Collapsible.Content>
							<pre
								class="mt-2 max-h-72 overflow-auto rounded-xl bg-muted/40 px-3 py-2 font-mono text-xs break-all whitespace-pre-wrap text-muted-foreground">{entry
									.message.output}</pre>
						</Collapsible.Content>
					</Collapsible.Root>
				{:else}
					{@render reply(entry, index === entries.length - 1)}
				{/if}
			{/each}

			{#if chat.background.length}
				<div class="flex flex-col gap-1.5 rounded-2xl border px-4 py-3 text-sm">
					<div class="flex items-center justify-between gap-3">
						<span class="text-xs font-medium text-muted-foreground">Working in the background</span>
						<Button size="sm" variant="outline" onclick={() => post('stop')}>Stop</Button>
					</div>
					{#each chat.background as item (item.id)}
						<div class="flex min-w-0 items-center gap-2 text-muted-foreground">
							<LoaderIcon class="size-3.5 shrink-0 animate-spin" />
							{#if item.kind === 'command'}
								<span class={cn('min-w-0 truncate', prefs.technical && 'font-mono text-xs')}>
									{prefs.technical ? `$ ${firstLine(item.command)}` : (item.summary ?? 'A command')}
								</span>
							{:else}
								<a
									href={resolve('/p/[slug]/c/[id]', {
										slug: page.params.slug ?? '',
										id: item.conversationId
									})}
									class="min-w-0 truncate underline-offset-2 hover:text-foreground hover:underline"
								>
									Subagent {item.name}{item.status === 'stopping' ? ' · stopping' : ''}
								</a>
							{/if}
						</div>
					{/each}
				</div>
			{/if}

			{#each chat.queued as message (message.id)}
				{#if message.kind === 'human'}
					{@render humanBubble(message.senderName, message.text, message.attachments, true)}
				{/if}
			{/each}

			{#if chat.error || (unanswered && !conversation.subagent)}
				<div class="relative flex flex-col gap-3">
					{#if !liveReply}
						<!-- No reply to carry the avatar, so it waits here, by what to do next. -->
						{@render assistant(mood)}
					{/if}
					{#if chat.error}
						<div
							class="flex flex-wrap items-center gap-3 rounded-2xl border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm"
						>
							<CircleAlertIcon class="size-4 shrink-0 text-destructive" />
							<span class="min-w-0 flex-1">
								<span class="block font-medium">Something went wrong while btw was answering.</span>
								<span class="block text-muted-foreground">{chat.error}</span>
							</span>
							{#if !conversation.subagent}
								<Button size="sm" variant="outline" onclick={() => post('continue')}>
									<RotateCcwIcon />
									Try again
								</Button>
							{/if}
						</div>
					{:else}
						<div class="flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
							btw hasn't answered this yet.
							<Button size="sm" variant="outline" onclick={() => post('continue')}>Continue</Button>
						</div>
					{/if}
				</div>
			{/if}
		</div>
	</div>

	<ComposerDock bind:height={composerHeight}>
		{#if !stickToBottom}
			<button
				onclick={scrollToBottom}
				class="absolute bottom-full left-1/2 mb-3 flex size-9 -translate-x-1/2 items-center justify-center rounded-full border bg-background text-foreground shadow-md hover:bg-muted"
				aria-label="Scroll to the newest message"
			>
				<ArrowDownIcon class="size-4" />
			</button>
		{/if}
		{#if conversation.subagent}
			<div
				class="flex items-center justify-between gap-3 rounded-[26px] border bg-background px-5 py-3 text-sm text-muted-foreground shadow-sm"
			>
				<span>Only the agent that started {conversation.subagent.name} writes here.</span>
				{#if chat.running}
					<Button size="sm" variant="outline" onclick={() => post('stop')}>Stop</Button>
				{/if}
			</div>
		{:else}
			<Composer
				bind:value={text}
				bind:textarea
				{attachments}
				running={chat.running}
				busy={sending}
				placeholder={chat.running ? 'Add something while btw works…' : 'Ask btw'}
				onsubmit={send}
				onstop={() => post('stop')}
			>
				{#snippet tools()}
					<ModelMenu
						{efforts}
						{effort}
						onEffortChange={changeEffort}
						presets={[{ id: 'current', name: conversation.presetName }]}
						presetId="current"
					/>
				{/snippet}
			</Composer>
		{/if}
		{#if actionError}
			<p class="mt-2 text-center text-sm text-destructive">{actionError}</p>
		{:else if !chat.connected && chat.loaded}
			<p class="mt-2 text-center text-xs text-warning">Reconnecting…</p>
		{:else}
			<p class="mt-2 hidden text-center text-xs text-muted-foreground sm:block">
				btw can make mistakes, and it can change files on this computer.
			</p>
		{/if}
	</ComposerDock>
</div>

<MediaViewer bind:picture={viewing} />

<RenameChatDialog bind:chat={renaming} slug={page.params.slug ?? ''} />

<NewFolderDialog
	bind:open={creatingFolder}
	slug={page.params.slug ?? ''}
	oncreated={(created) => move(created.id)}
/>

<AlertDialog.Root bind:open={deleteOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Delete chat?</AlertDialog.Title>
			<AlertDialog.Description>
				This deletes <strong class="text-foreground">{title}</strong> for everyone in the profile.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form method="POST" action="?/delete" use:enhance>
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">Cancel</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive">Delete</AlertDialog.Action>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
