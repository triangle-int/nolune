<script lang="ts">
	import { onDestroy, untrack } from 'svelte';
	import { enhance } from '$app/forms';
	import { invalidate } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import type {
		ChatCommands,
		ChatModel,
		CommandMode,
		DisplayAttachment,
		Usage
	} from '@nolune/core';
	import type { Avatar } from '@nolune/core/avatars';
	import { cacheHitRate, cacheMissTokens, cacheTtlMs, promptTokens } from '@nolune/core/usage';
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
	import { errorMessage } from '$lib/http';
	import { getI18n } from '$lib/i18n';
	import { getPreferences } from '$lib/preferences.svelte';
	import { activeStepLabel, buildTranscript, replyText, type Reply } from '$lib/transcript';
	import { TypingReporter } from '$lib/typing';
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
	import MemoryLook from './chat/MemoryLook.svelte';
	import MediaViewer, { pictureClicks, type PictureGallery } from './chat/MediaViewer.svelte';
	import MessageAttachments from './chat/MessageAttachments.svelte';
	import CommandModeMenu from './chat/CommandModeMenu.svelte';
	import ModelMenu, { shortModelName } from './chat/ModelMenu.svelte';
	import RenameChatDialog from './chat/RenameChatDialog.svelte';
	import TypingIndicator from './chat/TypingIndicator.svelte';
	import PageHeader from './PageHeader.svelte';
	import Rich from './Rich.svelte';
	import TypedText from './TypedText.svelte';
	import UserAvatar from './UserAvatar.svelte';

	interface Props {
		conversation: {
			id: string;
			title: string;
			presetId: string | null;
			presetName: string;
			provider: ChatModel['provider'];
			effort: ChatModel['effort'];
			contextWindow: number | null;
			/** Providers holding pictures and PDFs of this chat that another can't open. */
			heldBy: ChatModel['provider'][];
			/** A background run nobody has continued yet. */
			hidden: boolean;
			cacheTtl: '5m' | '1h';
			/** How its commands run, until the chat says otherwise live. */
			commands: ChatCommands;
			/** A subagent's own chat: only the agent that started it writes here. */
			subagent: { name: string; parentId: string; parentTitle: string } | null;
		};
		efforts: string[];
		/** The models the chat can switch to. */
		presets: { id: string; name: string; provider: ChatModel['provider'] }[];
		defaultPresetId: string;
		/** Who is looking: their own messages go without a name. */
		me: string;
		/** Members' profile pictures by user id. */
		pictures: Record<string, string>;
		/** The profile's folders, and the one this chat is in. */
		folders: FolderItem[];
		folderId: string | null;
		/** The profile's assistant avatar, shown with every reply. */
		avatar: Avatar;
	}

	let {
		conversation,
		efforts,
		presets,
		defaultPresetId,
		me,
		pictures,
		folders,
		folderId,
		avatar
	}: Props = $props();

	const prefs = getPreferences();
	const { m } = getI18n();
	/** Reasoning levels' names, for the dialog that asks before changing it. */
	const effortLabels: Record<string, { label: string } | undefined> = m.model.efforts;
	const chat = new ChatState();
	let text = $state('');
	const attachments = new Attachments(() => page.params.slug ?? '', m);
	let sending = $state(false);
	let actionError = $state<string | null>(null);
	let stickToBottom = $state(true);
	/** Sending a message turns a background run into a normal conversation. */
	let continued = $state(false);
	let renaming = $state<{ id: string; title: string } | null>(null);
	let deleteOpen = $state(false);
	let creatingFolder = $state(false);
	const folder = $derived(folders.find((f) => f.id === folderId));
	let viewing = $state<PictureGallery | null>(null);
	let scroller = $state<HTMLElement>();
	/** The messages scroll under the composer, so they end this far up. */
	let composerHeight = $state(160);
	let textarea = $state<HTMLTextAreaElement | null>(null);

	$effect(() => chat.connect(conversation.id));

	/** Tells the others in the chat when this person is writing in it. */
	const typing = new TypingReporter((typing) =>
		fetch(`/api/c/${conversation.id}/typing`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ typing }),
			// So that they stopped still gets there when the page closes.
			keepalive: true
		})
	);
	onDestroy(() => typing.stop());
	/** The others writing in the chat, shown where their message will land. */
	const typists = $derived(chat.typing.filter((typist) => typist.id !== page.data.user?.id));

	/** The model and reasoning level: anyone in the profile can change them, which arrives live. */
	const model: ChatModel = $derived(
		chat.model ?? {
			presetId: conversation.presetId,
			presetName: conversation.presetName,
			provider: conversation.provider,
			effort: conversation.effort,
			contextWindow: conversation.contextWindow
		}
	);
	/** How its commands run: anyone can have them checked, only admins can turn that off. */
	const commands = $derived(chat.commands ?? conversation.commands);

	async function changeCommands(mode: CommandMode) {
		if (mode === (commands.mode ?? commands.fallback)) return;
		const res = await post('commands', { mode });
		if (res) chat.apply({ type: 'commands', commands: (await res.json()) as ChatCommands });
	}

	/** A removed preset isn't in the list, but the chat still runs on its model: it shows as `current`. */
	const listed = $derived(presets.some((p) => p.id === model.presetId));
	const menuPresets = $derived(
		listed
			? presets
			: [{ id: 'current', name: model.presetName, provider: model.provider }, ...presets]
	);

	type ModelChange = { presetId: string } | { effort: ChatModel['effort'] };
	/** A change waiting for the person to accept that the chat's cache starts over. */
	let confirming = $state<ModelChange | null>(null);
	/** The model menu, closed while a change is asked about so it doesn't show one not yet made. */
	let modelMenuOpen = $state(false);
	const switchingTo = $derived.by(() => {
		const change = confirming;
		return change && 'presetId' in change
			? presets.find((p) => p.id === change.presetId)
			: undefined;
	});

	const title = $derived(
		chat.title ||
			conversation.title ||
			chat.messages.find((message) => message.kind === 'human')?.text.slice(0, 80) ||
			m.common.newChat
	);

	// nolune names a chat shortly after its first message; the sidebar lists the title too.
	let listedTitle = untrack(() => conversation.title);
	$effect(() => {
		if (!chat.title || chat.title === listedTitle) return;
		listedTitle = chat.title;
		invalidate('nolune:conversations');
	});

	const entries = $derived(buildTranscript(chat.messages, chat.live, chat.running, chat.memory));
	/** The newest entry, past what the note-taker saved after it. */
	const newest = $derived(entries.findLast((entry) => entry.type !== 'memory'));

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
		return m.chat.cacheSummary(
			label,
			formatPercent(cacheHitRate(u)),
			formatTokens(u.cacheRead),
			formatTokens(u.cacheWrite),
			formatTokens(u.input)
		);
	}

	/** Cache misses within one reply, which may span several model calls. */
	function replyMiss(reply: Reply): { tokens: number; reason: string } | null {
		const misses = reply.messageIds.flatMap((id) => cacheMisses[id] ?? []);
		if (!misses.length) return null;
		return {
			tokens: misses.reduce((n, miss) => n + miss.tokens, 0),
			reason: misses.some((miss) => miss.expired)
				? m.chat.cacheExpired(conversation.cacheTtl)
				: m.chat.cacheBroken
		};
	}

	const unanswered = $derived(
		!chat.running &&
			chat.queued.length === 0 &&
			chat.messages.length > 0 &&
			chat.messages[chat.messages.length - 1].kind !== 'assistant'
	);

	/** The newest entry when it's a reply: its avatar shows what nolune is doing. Older ones hold still. */
	const liveReply = $derived(newest?.type === 'reply' ? newest : null);

	/** The step nolune is on while it runs, in the words its group of steps uses. */
	const step = $derived.by(() => {
		const tail = chat.running ? liveReply?.parts.at(-1) : undefined;
		if (!tail || tail.type === 'text') {
			return chat.running
				? { label: tail ? m.chat.writing : m.chat.thinking, command: false }
				: null;
		}
		const last = tail.steps.at(-1);
		return {
			label: activeStepLabel(tail, chat.results, prefs.technical, m),
			// Running once the model has finished writing it, until its result arrives.
			command:
				last?.type === 'command' &&
				!chat.results[last.id] &&
				!chat.live.some((block) => block?.type === 'tool' && block.id === last.id)
		};
	});

	/** True for a moment after nolune finishes a turn, for the avatar's happy squash. */
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
	/** How long a wheel, keys or a flick may still be moving the view after their last event. */
	const SETTLE_MS = 250;
	/** What a gesture sends to the element it began on, wherever that element has gone. */
	const GESTURE_EVENTS = ['wheel', 'touchmove', 'touchend', 'touchcancel'];

	/**
	 * Keeps the view pinned to the newest content while the reader is at the bottom, whenever
	 * anything changes size: new messages and streamed text, but also pictures that finish loading
	 * and the composer growing.
	 *
	 * Only the reader decides: starting to scroll up (wheel, trackpad, finger or keys) lets go
	 * right away, before the view has moved far, and scrolling down to the end sticks to the bottom.
	 * While they move the view it's theirs, and the chat follows again once it settles. When the
	 * browser moves the view by itself (content changing size, focus), the choice stays, so the
	 * chat never jumps to the bottom on its own.
	 */
	function autoscroll(node: HTMLElement) {
		/** Where the view was, and how tall the chat, when last placed or scrolled. */
		let lastTop = node.scrollTop;
		let lastHeight = node.scrollHeight;
		let fingers = 0;
		let touchY = 0;
		/** A mouse or pen is down in the chat: dragging its scrollbar, or selecting text. */
		let pointerDown = false;
		/** Pending while a wheel, keys, a flick or a dragged scrollbar may still be moving the view. */
		let settling: ReturnType<typeof setTimeout> | undefined;
		const holding = () => fingers > 0 || settling !== undefined;

		const remember = () => {
			lastTop = node.scrollTop;
			lastHeight = node.scrollHeight;
		};
		const follow = () => {
			if (stickToBottom) node.scrollTop = node.scrollHeight;
			remember();
		};
		const settle = () => {
			clearTimeout(settling);
			settling = setTimeout(() => {
				settling = undefined;
				if (holding()) return;
				unwatch();
				follow();
			}, SETTLE_MS);
		};
		const release = () => {
			if (node.scrollTop > 0) stickToBottom = false;
		};

		const observer = new ResizeObserver(() => {
			if (!holding()) follow();
			// Content that got shorter pulled the view up with it. Its scroll event only comes a
			// frame later, maybe after the content grew back, and must not read as the reader
			// scrolling up.
			else remember();
		});
		observer.observe(node);
		// The border box, so the padding the composer sets counts too.
		for (const child of node.children) observer.observe(child, { box: 'border-box' });

		/**
		 * Touch events, and in Safari all of a wheel gesture's, go to the element the gesture began
		 * on even once it's gone from the page, and from there they no longer reach the chat. The
		 * reply being written is drawn anew with every word, so a gesture that starts on it is
		 * listened to where it began too, until the view settles.
		 */
		const origins: EventTarget[] = [];
		const watch = (target: EventTarget | null) => {
			if (!target || target === node || origins.includes(target)) return;
			origins.push(target);
			for (const type of GESTURE_EVENTS) target.addEventListener(type, onOrigin, { passive: true });
		};
		const unwatch = () => {
			for (const target of origins.splice(0))
				for (const type of GESTURE_EVENTS) target.removeEventListener(type, onOrigin);
		};
		const onOrigin = (event: Event) => {
			if (node.contains(event.target as Node)) return; // it reaches the chat as well
			if (event.type === 'wheel') onWheel(event as WheelEvent);
			else if (event.type === 'touchmove') onTouchMove(event as TouchEvent);
			else onTouchEnd(event as TouchEvent);
		};

		const onWheel = (event: WheelEvent) => {
			watch(event.target);
			if (event.deltaY < 0) release();
			settle();
		};
		/** The fingers down that began on the chat, wherever the elements they began on went. */
		const countFingers = (event: TouchEvent) =>
			Array.from(event.touches).filter(
				(touch) => touch.target === node || origins.includes(touch.target)
			).length;
		const onTouchStart = (event: TouchEvent) => {
			watch(event.target);
			fingers = countFingers(event);
			touchY = event.touches[0]?.clientY ?? 0;
		};
		const onTouchMove = (event: TouchEvent) => {
			const y = event.touches[0]?.clientY ?? touchY;
			if (y > touchY) release(); // a finger moving down scrolls up
			touchY = y;
		};
		const onTouchEnd = (event: TouchEvent) => {
			fingers = countFingers(event);
			settle(); // a flick glides on after the finger lifts
		};
		const onKeyDown = (event: KeyboardEvent) => {
			if (event.defaultPrevented || !['ArrowUp', 'PageUp', 'Home'].includes(event.key)) return;
			// Not in the composer or a menu, where the key does something else.
			const target = event.target as Node;
			if (target !== document.body && !node.contains(target)) return;
			release();
			settle();
		};
		const onPointerDown = (event: PointerEvent) => {
			// A finger's pointer events stop once the chat scrolls; its touch events don't.
			if (event.pointerType === 'touch') return;
			pointerDown = true;
			settle();
		};
		const onPointerUp = () => (pointerDown = false);
		const onScroll = () => {
			const top = node.scrollTop;
			const atBottom = node.scrollHeight - top - node.clientHeight < BOTTOM_SLACK;
			if (top > lastTop && atBottom) stickToBottom = true;
			else if (top < lastTop && !atBottom && stickToBottom) {
				// The chat changed size since the view was placed: it got shorter, which pulled the
				// view up, and grew again before this event (Safari and Firefox can lay it out in
				// between). That was the browser, not the reader, so back to the end.
				if (node.scrollHeight !== lastHeight && !pointerDown && !holding())
					node.scrollTop = node.scrollHeight;
				else stickToBottom = false;
			}
			if (holding()) settle(); // still moving: a flick gliding, keys repeating
			remember();
		};

		node.addEventListener('scroll', onScroll, { passive: true });
		node.addEventListener('wheel', onWheel, { passive: true });
		node.addEventListener('touchstart', onTouchStart, { passive: true });
		node.addEventListener('touchmove', onTouchMove, { passive: true });
		node.addEventListener('touchend', onTouchEnd);
		node.addEventListener('touchcancel', onTouchEnd);
		node.addEventListener('pointerdown', onPointerDown);
		window.addEventListener('pointerup', onPointerUp);
		window.addEventListener('pointercancel', onPointerUp);
		window.addEventListener('keydown', onKeyDown);
		return () => {
			observer.disconnect();
			clearTimeout(settling);
			unwatch();
			node.removeEventListener('scroll', onScroll);
			node.removeEventListener('wheel', onWheel);
			node.removeEventListener('touchstart', onTouchStart);
			node.removeEventListener('touchmove', onTouchMove);
			node.removeEventListener('touchend', onTouchEnd);
			node.removeEventListener('touchcancel', onTouchEnd);
			node.removeEventListener('pointerdown', onPointerDown);
			window.removeEventListener('pointerup', onPointerUp);
			window.removeEventListener('pointercancel', onPointerUp);
			window.removeEventListener('keydown', onKeyDown);
		};
	}

	/** Follows the newest content from now on, even what arrives while the view is on its way. */
	function scrollToBottom() {
		stickToBottom = true;
		scroller?.scrollTo({ top: scroller.scrollHeight, behavior: 'smooth' });
	}

	/** The response if the request went through; otherwise null, with the reason shown. */
	async function post(path: string, body?: unknown): Promise<Response | null> {
		actionError = null;
		const res = await fetch(`/api/c/${conversation.id}/${path}`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: body === undefined ? undefined : JSON.stringify(body)
		});
		if (!res.ok) {
			const message = errorMessage(await res.text(), res.headers.get('content-type'));
			actionError = message ?? m.errors.requestFailed(res.status);
		}
		return res.ok ? res : null;
	}

	async function send() {
		const message = text.trim();
		const uploads = attachments.ids;
		if ((!message && !uploads.length) || sending || attachments.uploading) return;
		sending = true;
		stickToBottom = true;
		// A "typing" still on its way would otherwise show them typing again after the message.
		await typing.settled();
		if (await post('messages', { text: message, uploads })) {
			typing.sent();
			text = '';
			attachments.clear();
			continued = true;
			invalidate('nolune:conversations');
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

	/**
	 * Another model or reasoning level. Once the chat has a reply, its cache starts over with the
	 * change, so that is asked first.
	 */
	function requestChange(change: ModelChange) {
		const same =
			'presetId' in change
				? change.presetId === model.presetId || change.presetId === 'current'
				: change.effort === model.effort;
		if (same) return;
		if (chat.messages.some((m) => m.kind === 'assistant')) {
			confirming = change;
			modelMenuOpen = false;
		} else applyChange(change);
	}

	async function applyChange(change: ModelChange) {
		confirming = null;
		const res = await post('presetId' in change ? 'preset' : 'effort', change);
		if (res) chat.apply({ type: 'model', model: (await res.json()) as ChatModel });
	}
</script>

{#snippet humanBubble(
	sender: { id: string | null; name: string },
	body: string,
	files: DisplayAttachment[],
	pending: boolean
)}
	{@const senderName = sender.name}
	<!-- By id: the name is the one it was sent with, which may not be theirs any more. -->
	{@const mine = sender.id === me}
	<div data-media-group class="group/human flex flex-col items-end gap-1">
		{#if !mine || pending}
			<div class="flex items-center gap-1.5 px-1 text-xs text-muted-foreground">
				{#if pending}
					<ClockIcon class="size-3" />
					{mine ? m.chat.readsAfterStep : `${senderName} · ${m.chat.readsAfterStep}`}
				{:else}
					<UserAvatar
						name={senderName}
						picture={sender.id && pictures[sender.id]}
						class="size-4 text-[9px]"
					/>
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
	<div data-media-group class="group/reply relative flex flex-col gap-3">
		{@render assistant(last ? mood : undefined, last ? step?.label : undefined)}
		{#each r.parts as part, i (part.key)}
			{#if part.type === 'text'}
				<Markdown
					text={part.text}
					media={{ conversationId: conversation.id, media: part.media, pending: part.pending }}
				/>
			{:else}
				<Activity
					conversationId={conversation.id}
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
				<span class="sr-only">{m.chat.working}</span>
			</span>
		{/if}

		{#if r.stopReasons.includes('max_tokens')}
			<p class="text-sm text-warning">{m.chat.cutOff}</p>
		{/if}
		{#if r.stopReasons.includes('refusal')}
			<p class="text-sm text-warning">{m.chat.refused}</p>
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
							aria-label={m.chat.usage}
						>
							<InfoIcon class="size-4" />
						</Tooltip.Trigger>
						<Tooltip.Content class="max-w-xs flex-col items-start gap-0.5">
							{#if r.models.length}
								<span>{m.chat.models(r.models)}</span>
							{/if}
							<span
								>{m.chat.tokensInOut(
									formatTokens(promptTokens(r.usage)),
									formatTokens(r.usage.output)
								)}</span
							>
							<span>{cacheSummary(m.chat.cache, r.usage)}</span>
						</Tooltip.Content>
					</Tooltip.Root>
				{/if}
				{#if miss}
					<Tooltip.Root>
						<Tooltip.Trigger class="px-1.5 text-xs text-warning">
							{m.chat.cacheMiss(formatTokens(miss.tokens))}
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
				{m.chat.contextChip(
					formatTokens(contextUsed),
					formatTokens(model.contextWindow),
					formatPercent(cacheHitRate(usage.last))
				)}
			</Tooltip.Trigger>
			<Tooltip.Content class="max-w-sm flex-col items-start gap-0.5">
				<span
					>{m.chat.contextUsed(formatTokens(contextUsed), formatTokens(model.contextWindow))}</span
				>
				<span>{cacheSummary(m.chat.lastReply, usage.last)}</span>
				<span>{cacheSummary(m.chat.wholeConversation, usage.total)}</span>
			</Tooltip.Content>
		</Tooltip.Root>
	{/if}

	{#snippet actions()}
		<DropdownMenu.Root>
			<DropdownMenu.Trigger
				class="flex size-10 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-muted hover:text-foreground aria-expanded:bg-muted"
				aria-label={m.chat.options}
			>
				<EllipsisIcon class="size-5" />
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="end" class="w-60">
				{#if prefs.technical}
					<DropdownMenu.Label class="font-normal">
						<span class="block truncate text-foreground">{model.presetName}</span>
						{#if usage}
							<span class="block"
								>{m.chat.contextMenu(
									formatTokens(contextUsed),
									formatTokens(model.contextWindow)
								)}</span
							>
							<span class="block"
								>{m.chat.cachedOverall(formatPercent(cacheHitRate(usage.total)))}</span
							>
						{/if}
					</DropdownMenu.Label>
					<DropdownMenu.Separator />
				{/if}
				<DropdownMenu.Item onSelect={() => (renaming = { id: conversation.id, title })}>
					<PencilIcon />
					{m.common.rename}
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
					{m.common.delete}
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{/snippet}
</PageHeader>

{#if conversation.subagent}
	<div class="mx-auto w-full max-w-3xl px-4">
		<p class="rounded-2xl bg-muted px-4 py-2 text-center text-sm text-muted-foreground">
			<Rich text={m.chat.subagentBanner} name={conversation.subagent.name}>
				{#snippet parent()}<a
						href={resolve('/p/[slug]/c/[id]', {
							slug: page.params.slug ?? '',
							id: conversation.subagent?.parentId ?? ''
						})}
						class="text-foreground underline underline-offset-2"
						>{conversation.subagent?.parentTitle}</a
					>{/snippet}
			</Rich>
		</p>
	</div>
{:else if conversation.hidden && !continued}
	<div class="mx-auto w-full max-w-3xl px-4">
		<p class="rounded-2xl bg-muted px-4 py-2 text-center text-sm text-muted-foreground">
			{m.chat.hiddenBanner}
		</p>
	</div>
{/if}

<div class="relative min-h-0 flex-1">
	<!--
		Relative, so what's positioned in the chat (text only screen readers read, say) is placed in
		it. Placed outside it, deep down a long chat, it would stretch the page itself, and scrolling
		past the chat's end would carry the page up with it, leaving empty space under the composer.
	-->
	<div
		bind:this={scroller}
		{@attach autoscroll}
		{@attach pictureClicks((gallery) => (viewing = gallery))}
		class="@container/chat relative h-full overflow-y-auto [overflow-anchor:none]"
	>
		<div
			class="mx-auto flex max-w-3xl flex-col gap-7 px-4 pt-4 sm:px-6"
			style:padding-bottom="{composerHeight + 16}px"
		>
			{#if chat.loaded && chat.messages.length === 0 && chat.queued.length === 0 && !chat.running}
				<p class="py-16 text-center text-muted-foreground">{m.chat.empty}</p>
			{/if}

			{#each entries as entry (entry.key)}
				{#if entry.type === 'human'}
					{@render humanBubble(
						{ id: entry.message.senderId, name: entry.message.senderName },
						entry.message.text,
						entry.message.attachments,
						false
					)}
				{:else if entry.type === 'trigger'}
					<div class="rounded-2xl border px-4 py-3 text-sm">
						<div class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
							<ClockIcon class="size-3.5" />
							{m.chat.automation(entry.message.title)}
						</div>
						<div class="mt-1.5 leading-relaxed whitespace-pre-wrap">{entry.message.text}</div>
					</div>
				{:else if entry.type === 'agent_message'}
					<div class="rounded-2xl border px-4 py-3 text-sm">
						<div class="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
							<BotIcon class="size-3.5" />
							{m.chat.fromNolune(entry.message.title)}
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
								>{m.chat.finishedInBackground(entry.message.title)}</span
							>
							{#if entry.message.isError}
								<span class="shrink-0 text-destructive">
									{prefs.technical ? m.steps.failed : m.steps.didntWork}
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
				{:else if entry.type === 'memory'}
					<MemoryLook look={entry.look} slug={page.params.slug ?? ''} />
				{:else}
					{@render reply(entry, entry === newest)}
				{/if}
			{/each}

			{#if chat.background.length}
				<div class="flex flex-col gap-1.5 rounded-2xl border px-4 py-3 text-sm">
					<div class="flex items-center justify-between gap-3">
						<span class="text-xs font-medium text-muted-foreground">{m.chat.inBackground}</span>
						<Button size="sm" variant="outline" onclick={() => post('stop')}>{m.common.stop}</Button
						>
					</div>
					{#each chat.background as item (item.id)}
						<div class="flex min-w-0 items-center gap-2 text-muted-foreground">
							<LoaderIcon class="size-3.5 shrink-0 animate-spin" />
							{#if item.kind === 'command'}
								<span class={cn('min-w-0 truncate', prefs.technical && 'font-mono text-xs')}>
									{prefs.technical
										? `$ ${firstLine(item.command)}`
										: (item.summary ?? m.chat.aCommand)}
								</span>
							{:else}
								<a
									href={resolve('/p/[slug]/c/[id]', {
										slug: page.params.slug ?? '',
										id: item.conversationId
									})}
									class="min-w-0 truncate underline-offset-2 hover:text-foreground hover:underline"
								>
									{m.chat.subagent(item.name)}{item.status === 'stopping'
										? ` · ${m.chat.stopping}`
										: ''}
								</a>
							{/if}
						</div>
					{/each}
				</div>
			{/if}

			{#each chat.queued as message (message.id)}
				{#if message.kind === 'human'}
					{@render humanBubble(
						{ id: message.senderId, name: message.senderName },
						message.text,
						message.attachments,
						true
					)}
				{/if}
			{/each}

			{#if typists.length}
				<TypingIndicator {typists} {pictures} />
			{/if}

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
								<span class="block font-medium">{m.chat.errorTitle}</span>
								<span class="block text-muted-foreground">{chat.error}</span>
							</span>
							{#if !conversation.subagent}
								<Button size="sm" variant="outline" onclick={() => post('continue')}>
									<RotateCcwIcon />
									{m.common.tryAgain}
								</Button>
							{/if}
						</div>
					{:else}
						<div class="flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
							{m.chat.unanswered}
							<Button size="sm" variant="outline" onclick={() => post('continue')}
								>{m.common.continue}</Button
							>
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
				aria-label={m.chat.scrollToBottom}
			>
				<ArrowDownIcon class="size-4" />
			</button>
		{/if}
		{#if conversation.subagent}
			<div
				class="flex items-center justify-between gap-3 rounded-[26px] border bg-background px-5 py-3 text-sm text-muted-foreground shadow-sm"
			>
				<span>{m.chat.subagentOnly(conversation.subagent.name)}</span>
				{#if chat.running}
					<Button size="sm" variant="outline" onclick={() => post('stop')}>{m.common.stop}</Button>
				{/if}
			</div>
		{:else}
			<Composer
				bind:value={text}
				bind:textarea
				{attachments}
				running={chat.running}
				busy={sending}
				placeholder={chat.running ? m.chat.placeholderRunning : m.chat.placeholder}
				onsubmit={send}
				onstop={() => post('stop')}
				oninput={(value) => typing.input(value)}
			>
				{#snippet tools()}
					<ModelMenu
						{efforts}
						effort={model.effort}
						onEffortChange={(effort) => requestChange({ effort: effort as ChatModel['effort'] })}
						presets={menuPresets}
						presetId={(listed && model.presetId) || 'current'}
						onPresetChange={(presetId) => requestChange({ presetId })}
						{defaultPresetId}
						{avatar}
						bind:open={modelMenuOpen}
					/>
					<CommandModeMenu
						mode={commands.mode ?? commands.fallback}
						fallback={commands.fallback}
						canUnrestrict={page.data.user?.isAdmin === true}
						onchange={changeCommands}
					/>
				{/snippet}
			</Composer>
		{/if}
		{#if actionError}
			<p class="mt-2 text-center text-sm text-destructive">{actionError}</p>
		{:else if !chat.connected && chat.loaded}
			<p class="mt-2 text-center text-xs text-warning">{m.chat.reconnecting}</p>
		{:else}
			<p class="mt-2 hidden text-center text-xs text-muted-foreground sm:block">
				{m.chat.disclaimer}
			</p>
		{/if}
	</ComposerDock>
</div>

<!-- Someone who switched tabs or apps isn't typing, whatever the box still holds. -->
<svelte:document
	onvisibilitychange={() => {
		if (document.visibilityState === 'hidden') typing.stop();
	}}
/>

<MediaViewer bind:gallery={viewing} />

<RenameChatDialog bind:chat={renaming} slug={page.params.slug ?? ''} />

<NewFolderDialog
	bind:open={creatingFolder}
	slug={page.params.slug ?? ''}
	oncreated={(created) => move(created.id)}
/>

<AlertDialog.Root
	open={confirming !== null}
	onOpenChange={(open) => {
		if (!open) confirming = null;
	}}
>
	<AlertDialog.Content>
		{#if confirming}
			{@const change = confirming}
			<AlertDialog.Header>
				<AlertDialog.Title>
					{#if 'presetId' in change}
						{m.chat.switchTitle(
							switchingTo ? shortModelName(switchingTo.name) : m.chat.anotherModel
						)}
					{:else}
						{m.chat.effortTitle(effortLabels[change.effort]?.label ?? change.effort)}
					{/if}
				</AlertDialog.Title>
				<AlertDialog.Description class="flex flex-col gap-2">
					<span>
						{m.chat.switchCache(
							'presetId' in change,
							prefs.technical && contextUsed ? formatTokens(contextUsed) : null
						)}
					</span>
					{#if switchingTo && conversation.heldBy.some((p) => p !== switchingTo.provider)}
						<span>{m.chat.switchFiles}</span>
					{/if}
				</AlertDialog.Description>
			</AlertDialog.Header>
			<AlertDialog.Footer>
				<AlertDialog.Cancel>{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action onclick={() => applyChange(change)}>
					{'presetId' in change ? m.chat.switch : m.chat.change}
				</AlertDialog.Action>
			</AlertDialog.Footer>
		{/if}
	</AlertDialog.Content>
</AlertDialog.Root>

<AlertDialog.Root bind:open={deleteOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>{m.chat.deleteTitle}</AlertDialog.Title>
			<AlertDialog.Description>
				<Rich text={m.chat.deleteBody}>
					{#snippet name()}<strong class="text-foreground">{title}</strong>{/snippet}
				</Rich>
			</AlertDialog.Description>
		</AlertDialog.Header>
		<form method="POST" action="?/delete" use:enhance>
			<AlertDialog.Footer>
				<AlertDialog.Cancel type="button">{m.common.cancel}</AlertDialog.Cancel>
				<AlertDialog.Action type="submit" variant="destructive"
					>{m.common.delete}</AlertDialog.Action
				>
			</AlertDialog.Footer>
		</form>
	</AlertDialog.Content>
</AlertDialog.Root>
