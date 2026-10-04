import {
	ACTIVITY,
	GATEWAYS_PATH,
	type ActivityPush,
	type ActivityState,
	type PushResult
} from '@nolune/relay/protocol';
import { ChatView } from './chat-view.ts';
import { configExists, readConfig } from './config.ts';
import { notificationText } from './push.ts';
import { getSnapshot, subscribe, type LiveEvent, type Snapshot } from './runner.ts';
import type { Entry, ToolResult } from './transcript.ts';

/*
 * Live Activities: a reply nolune is working on, on the lock screen and in the Dynamic Island of
 * the iPhone that asked for it (nolune for iOS starts one when its person sends a message, and
 * gives its token from Apple, `POST /api/c/<id>/activity`). The gateway follows the chat and sends
 * what nolune does now through the relay (packages/relay/src/push.ts), at most every 10 seconds,
 * then the reply's first words once it's done. Nothing is kept: a gateway that restarts leaves the
 * activity as it was, and iOS ends it after a while.
 */

/** At most an update this often: Apple takes few, and the lock screen needs no more. */
const EVERY_MS = 10_000;
/** A reply that hasn't started this long after the activity did isn't coming. */
const START_WITHIN_MS = 60_000;
/** An ended activity stays on the lock screen this long. */
const LINGER_S = 15 * 60;
/** The most of the reply's first words an ended activity shows. */
const MAX_STEP = 160;

type Send = (change: ActivityPush) => Promise<string[]>;

interface Followed {
	/** Each activity's token, and whether it's a development build's. */
	activities: Map<string, boolean>;
	view: ChatView;
	title: string;
	running: boolean;
	/** The reply started: once it stops, the activity ends. */
	started: boolean;
	sentAt: number;
	last: string;
	timer: ReturnType<typeof setTimeout> | null;
	stop: () => void;
	send: Send;
	now: () => number;
}

const followed = new Map<string, Followed>();

/**
 * Follows a chat for a Live Activity until its reply is done. `options` are the tests': the chat
 * as it is, its events, and where changes go (the relay by default).
 */
export function followLiveActivity(
	conversationId: string,
	activity: { token: string; sandbox: boolean },
	options: {
		snapshot?: Snapshot;
		subscribe?: (listener: (event: LiveEvent) => void) => () => void;
		send?: Send;
		now?: () => number;
	} = {}
): void {
	let chat = followed.get(conversationId);
	if (!chat) {
		const snapshot = options.snapshot ?? getSnapshot(conversationId);
		const now = options.now ?? Date.now;
		const following: Followed = {
			activities: new Map(),
			view: new ChatView(snapshot),
			title: snapshot.title,
			running: snapshot.running,
			started: snapshot.running,
			sentAt: 0,
			last: '',
			timer: null,
			stop: () => {},
			send: options.send ?? sendToRelay,
			now
		};
		following.stop = (options.subscribe ?? ((l) => subscribe(conversationId, l)))((event) => {
			if (event.type === 'title') following.title = event.title;
			if (event.type === 'status') {
				following.running = event.running;
				if (event.running) following.started = true;
			}
			if (following.view.apply(event) || event.type === 'title' || event.type === 'status') {
				changed(conversationId, following);
			}
		});
		followed.set(conversationId, following);
		chat = following;
		// No reply after all (the message was queued behind another, say): the activity ends.
		setTimeout(() => {
			if (!following.started && followed.get(conversationId) === following) end(conversationId);
		}, START_WITHIN_MS).unref?.();
	}
	chat.activities.set(activity.token, activity.sandbox);
	changed(conversationId, chat);
}

/** Forgets an activity the person ended. */
export function forgetLiveActivity(conversationId: string, token: string): void {
	const chat = followed.get(conversationId);
	if (!chat) return;
	chat.activities.delete(token);
	if (chat.activities.size === 0) stopFollowing(conversationId);
}

/** The chats followed for Live Activities, for the tests. */
export function followedForActivities(): string[] {
	return [...followed.keys()];
}

/**
 * What an activity shows of a chat: its title, and while nolune works, the command it runs (in
 * the words the model chose) or nothing, which the app shows as working; once it's done, the
 * reply's first words.
 */
export function activityState(
	entries: Entry[],
	results: Record<string, ToolResult>,
	title: string,
	running: boolean
): ActivityState {
	const last = entries.at(-1);
	const reply = last?.type === 'reply' ? last : null;
	if (running) {
		const part = reply?.parts.at(-1);
		const step = part?.type === 'activity' ? part.steps.at(-1) : undefined;
		const command = step?.type === 'command' && !results[step.id] ? (step.summary ?? '') : '';
		return { title, step: command, running };
	}
	const text = reply?.parts.flatMap((p) => (p.type === 'text' ? [p.text] : [])).join('\n\n') ?? '';
	const words = notificationText(text).replace(/\s+/g, ' ').trim();
	return {
		title,
		step: words.length > MAX_STEP ? `${words.slice(0, MAX_STEP - 1).trimEnd()}…` : words,
		running
	};
}

function changed(conversationId: string, chat: Followed): void {
	if (chat.started && !chat.running) return end(conversationId);
	if (chat.timer) return;
	const wait = chat.sentAt + EVERY_MS - chat.now();
	if (wait <= 0) {
		void update(chat);
		return;
	}
	chat.timer = setTimeout(() => {
		chat.timer = null;
		if (followed.get(conversationId) === chat) void update(chat);
	}, wait);
}

async function update(chat: Followed): Promise<void> {
	const { entries, results } = chat.view.transcript();
	const state = activityState(entries, results, chat.title, true);
	const json = JSON.stringify(state);
	if (json === chat.last) return;
	chat.last = json;
	chat.sentAt = chat.now();
	await deliver(chat, { event: 'update', state });
}

function end(conversationId: string): void {
	const chat = followed.get(conversationId);
	if (!chat) return;
	stopFollowing(conversationId);
	const { entries, results } = chat.view.transcript();
	const state = activityState(entries, results, chat.title, false);
	void deliver(chat, {
		event: 'end',
		state,
		dismissAt: Math.floor(chat.now() / 1000) + LINGER_S
	});
}

function stopFollowing(conversationId: string): void {
	const chat = followed.get(conversationId);
	if (!chat) return;
	followed.delete(conversationId);
	chat.stop();
	if (chat.timer) clearTimeout(chat.timer);
}

async function deliver(chat: Followed, change: Omit<ActivityPush, 'activities'>): Promise<void> {
	const activities = [...chat.activities].map(([token, sandbox]) => ({ token, sandbox }));
	if (activities.length === 0) return;
	const gone = await chat.send({ activities, ...change }).catch(() => []);
	for (const token of gone) chat.activities.delete(token);
}

/** Through the relay, which holds the app's key from Apple. The tokens Apple says are gone. */
async function sendToRelay(change: ActivityPush): Promise<string[]> {
	if (!configExists()) return [];
	const relay = readConfig().relay;
	if (!relay) return [];
	try {
		const url = new URL(
			`${GATEWAYS_PATH}/${encodeURIComponent(relay.name)}/${ACTIVITY}`,
			relay.server
		);
		const res = await fetch(url, {
			method: 'POST',
			headers: { authorization: `Bearer ${relay.token}`, 'content-type': 'application/json' },
			body: JSON.stringify(change),
			signal: AbortSignal.timeout(30_000)
		});
		const body = (await res.json().catch(() => null)) as (PushResult & { error?: string }) | null;
		if (!res.ok || !body) {
			console.error(
				`[nolune] the relay didn't update a Live Activity: ${body?.error ?? res.status}`
			);
			return [];
		}
		return body.gone;
	} catch (err) {
		console.error('[nolune] could not update a Live Activity:', (err as Error).message);
		return [];
	}
}
