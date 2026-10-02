import { and, eq, inArray, ne } from 'drizzle-orm';
import { Marked, type Token, type Tokens } from 'marked';
import { GATEWAYS_PATH, PUSH, type Push, type PushResult } from '@nolune/relay/protocol';
import { configExists, readConfig } from './config.ts';
import { getDb } from './db/index.ts';
import { profile, profileMember, pushDevice } from './db/schema.ts';
import { mediaAsText } from './media-refs.ts';
import type { Notification } from './notifications.ts';

/*
 * The bell's notifications on the family's iPhones (nolune for iOS, ios/ in the repository). The
 * app registers its iPhone from the page once someone has signed in (`POST /api/push`), and each
 * new notification goes to the iPhones of its profile's members through the relay, which holds
 * the app's key from Apple (packages/relay/src/push.ts). A nolune that doesn't use the relay sends
 * none; the bell still has them.
 */

const DEVICE_TOKEN = /^[0-9a-f]{32,200}$/i;

/** The most of a notification's text sent on: the lock screen shows a few lines of it. */
const MAX_TEXT = 2000;

export function isDeviceToken(token: unknown): token is string {
	return typeof token === 'string' && DEVICE_TOKEN.test(token);
}

/**
 * Registers the iPhone a session is signed in on. A session has one iPhone, so a new token
 * replaces the one it had; a token another session had (someone else signed in on that iPhone)
 * moves to this one.
 */
export function registerPushDevice(device: {
	token: string;
	sandbox: boolean;
	userId: string;
	sessionId: string;
}): void {
	const token = device.token.toLowerCase();
	getDb().transaction((tx) => {
		tx.delete(pushDevice)
			.where(and(eq(pushDevice.sessionId, device.sessionId), ne(pushDevice.token, token)))
			.run();
		tx.insert(pushDevice)
			.values({ ...device, token })
			.onConflictDoUpdate({
				target: pushDevice.token,
				set: { userId: device.userId, sessionId: device.sessionId, sandbox: device.sandbox }
			})
			.run();
	});
}

/** Forgets the iPhone a session is signed in on: the app is leaving for another nolune. */
export function forgetSessionPushDevice(sessionId: string): void {
	getDb().delete(pushDevice).where(eq(pushDevice.sessionId, sessionId)).run();
}

/** Forgets iPhones Apple says are gone: the app was deleted, or the token was another build's. */
export function forgetPushDevices(tokens: string[]): void {
	if (tokens.length === 0) return;
	getDb()
		.delete(pushDevice)
		.where(
			inArray(
				pushDevice.token,
				tokens.map((token) => token.toLowerCase())
			)
		)
		.run();
}

/** The iPhones of a profile's members. */
export function pushDevicesForProfile(profileId: string): { token: string; sandbox: boolean }[] {
	return getDb()
		.select({ token: pushDevice.token, sandbox: pushDevice.sandbox })
		.from(pushDevice)
		.innerJoin(
			profileMember,
			and(eq(profileMember.userId, pushDevice.userId), eq(profileMember.profileId, profileId))
		)
		.all();
}

/**
 * Sends a new notification to the iPhones of its profile's members: its title, the profile's name
 * and its text, and the bell to open when it's tapped. Never throws; a problem is logged.
 */
export async function pushNotification(n: Notification): Promise<void> {
	const devices = pushDevicesForProfile(n.profileId);
	if (devices.length === 0 || !configExists()) return;
	const relay = readConfig().relay;
	const p = getDb().select().from(profile).where(eq(profile.id, n.profileId)).get();
	if (!relay || !p) return;
	const message: Push = {
		devices,
		title: n.title,
		subtitle: p.name,
		body: notificationText(n.body),
		thread: p.slug,
		path: `/p/${encodeURIComponent(p.slug)}?notification=${encodeURIComponent(n.id)}`
	};
	try {
		const url = new URL(`${GATEWAYS_PATH}/${encodeURIComponent(relay.name)}/${PUSH}`, relay.server);
		const res = await fetch(url, {
			method: 'POST',
			headers: { authorization: `Bearer ${relay.token}`, 'content-type': 'application/json' },
			body: JSON.stringify(message),
			signal: AbortSignal.timeout(30_000)
		});
		const body = (await res.json().catch(() => null)) as (PushResult & { error?: string }) | null;
		if (!res.ok || !body) {
			console.error(
				`[nolune] the relay didn't send a notification to iPhones: ${body?.error ?? res.status}`
			);
			return;
		}
		forgetPushDevices(body.gone);
	} catch (err) {
		console.error('[nolune] could not send a notification to iPhones:', (err as Error).message);
	}
}

const lexer = new Marked({ gfm: true });

/**
 * A notification's Markdown as text for the lock screen: pictures as their description, links as
 * their label, lists as lines with a bullet, and no markup.
 */
export function notificationText(markdown: string): string {
	const text = blocks(lexer.lexer(mediaAsText(markdown)))
		.replace(/\n{3,}/g, '\n\n')
		.trim();
	const characters = [...text];
	return characters.length > MAX_TEXT
		? characters.slice(0, MAX_TEXT).join('').trimEnd() + '…'
		: text;
}

function blocks(tokens: Token[]): string {
	return tokens
		.map(block)
		.filter((text) => text !== '')
		.join('\n');
}

function block(token: Token): string {
	switch (token.type) {
		case 'space':
		case 'hr':
			return '';
		case 'heading':
		case 'paragraph':
			return inline(token.tokens ?? []);
		case 'text':
			return token.tokens ? inline(token.tokens) : token.text;
		case 'code':
			return token.text;
		case 'blockquote':
			return blocks(token.tokens ?? []);
		case 'list': {
			const list = token as Tokens.List;
			const start = typeof list.start === 'number' ? list.start : 1;
			return list.items
				.map((item, i) => `${list.ordered ? `${start + i}.` : '•'} ${blocks(item.tokens).trim()}`)
				.join('\n');
		}
		case 'table': {
			const table = token as Tokens.Table;
			const row = (cells: Tokens.TableCell[]) =>
				cells.map((cell) => inline(cell.tokens)).join(' · ');
			return [row(table.header), ...table.rows.map(row)].join('\n');
		}
		case 'html':
			return token.text.replace(/<[^>]*>/g, '').trim();
		default:
			return 'tokens' in token && token.tokens ? inline(token.tokens) : token.raw;
	}
}

function inline(tokens: Token[]): string {
	return tokens
		.map((token): string => {
			switch (token.type) {
				case 'strong':
				case 'em':
				case 'del':
				case 'link':
					return inline(token.tokens ?? []);
				case 'image':
					return `[${token.text.trim() || 'picture'}]`;
				case 'codespan':
				case 'escape':
					return token.text;
				case 'br':
					return '\n';
				case 'text':
					return token.tokens ? inline(token.tokens) : token.text;
				case 'html':
					return token.text.replace(/<[^>]*>/g, '');
				default:
					return token.raw;
			}
		})
		.join('');
}
