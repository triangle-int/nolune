import { createHash, timingSafeEqual } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';

/*
 * The gateways the relay knows: a name each, with a hash of its token (never the token itself),
 * the network it was registered from, its traffic this month and whether it's blocked. Kept in
 * one JSON file, written whole: at once for registrations, releases and blocks, and by flush()
 * for what changes all the time (traffic, when each was last seen), which the relay calls every
 * minute. Without a file it lives in memory, for tests.
 */

export interface GatewayRecord {
	name: string;
	/** sha256 of the token, hex. */
	tokenHash: string;
	createdAt: string;
	/** When it last connected. */
	lastSeenAt?: string;
	/** Where it was registered from (network() of the client's address), for the limit on each. */
	registeredFrom?: string;
	/** What passed through the relay for it, both ways, in a month (UTC, `2026-09`). */
	traffic?: { month: string; bytes: number };
	/** Taken off the relay by its operator: requests get a page saying so, and it can't connect. */
	blocked?: { at: string; reason: string };
}

export function hashToken(token: string): string {
	return createHash('sha256').update(token).digest('hex');
}

/** This month, as traffic is counted: `2026-09`, in UTC. */
export function currentMonth(now = new Date()): string {
	return now.toISOString().slice(0, 7);
}

/**
 * Whom a client address stands for, for limits on each: an IPv4 address, or an IPv6 /64, which
 * one home usually has all of.
 */
export function network(address: string): string {
	const bare = address.replace(/^::ffff:(?=\d+\.)/, '').replace(/%.*$/, '');
	if (!bare.includes(':')) return bare;
	const [head, tail] = bare.split('::');
	const start = head ? head.split(':') : [];
	const end = tail ? tail.split(':') : [];
	const groups =
		tail === undefined
			? start
			: [...start, ...Array(8 - start.length - end.length).fill('0'), ...end];
	return `${groups
		.slice(0, 4)
		.map((group) => parseInt(group || '0', 16).toString(16))
		.join(':')}::/64`;
}

export class GatewayStore {
	readonly #file: string | undefined;
	readonly #gateways = new Map<string, GatewayRecord>();
	#dirty = false;

	constructor(file?: string) {
		this.#file = file;
		if (file && existsSync(file)) {
			const records = JSON.parse(readFileSync(file, 'utf8')) as GatewayRecord[];
			for (const record of records) this.#gateways.set(record.name, record);
		}
	}

	get size(): number {
		return this.#gateways.size;
	}

	has(name: string): boolean {
		return this.#gateways.has(name);
	}

	get(name: string): GatewayRecord | undefined {
		return this.#gateways.get(name);
	}

	list(): GatewayRecord[] {
		return [...this.#gateways.values()];
	}

	add(name: string, token: string, registeredFrom?: string): GatewayRecord {
		if (this.#gateways.has(name)) throw new Error(`the name ${name} is taken`);
		const record: GatewayRecord = {
			name,
			tokenHash: hashToken(token),
			createdAt: new Date().toISOString(),
			...(registeredFrom ? { registeredFrom } : {})
		};
		this.#gateways.set(name, record);
		this.#save();
		return record;
	}

	/** How many gateways were registered from a network and are still here. */
	countFrom(registeredFrom: string): number {
		let count = 0;
		for (const record of this.#gateways.values()) {
			if (record.registeredFrom === registeredFrom) count++;
		}
		return count;
	}

	/** The gateway, when the token is its own. */
	verify(name: string, token: string): GatewayRecord | null {
		const record = this.#gateways.get(name);
		if (!record) return null;
		const given = Buffer.from(hashToken(token), 'hex');
		const stored = Buffer.from(record.tokenHash, 'hex');
		return given.length === stored.length && timingSafeEqual(given, stored) ? record : null;
	}

	seen(name: string): void {
		const record = this.#gateways.get(name);
		if (!record) return;
		record.lastSeenAt = new Date().toISOString();
		this.#dirty = true;
	}

	addTraffic(name: string, bytes: number): void {
		const record = this.#gateways.get(name);
		if (!record) return;
		const month = currentMonth();
		if (record.traffic?.month !== month) record.traffic = { month, bytes: 0 };
		record.traffic.bytes += bytes;
		this.#dirty = true;
	}

	/** Bytes through the relay this month; a count from an earlier month is none. */
	trafficThisMonth(name: string): number {
		const traffic = this.#gateways.get(name)?.traffic;
		return traffic?.month === currentMonth() ? traffic.bytes : 0;
	}

	block(name: string, reason: string): boolean {
		const record = this.#gateways.get(name);
		if (!record) return false;
		record.blocked = { at: new Date().toISOString(), reason };
		this.#save();
		return true;
	}

	unblock(name: string): boolean {
		const record = this.#gateways.get(name);
		if (!record?.blocked) return false;
		delete record.blocked;
		this.#save();
		return true;
	}

	remove(name: string): boolean {
		const removed = this.#gateways.delete(name);
		if (removed) this.#save();
		return removed;
	}

	/** Writes what changed since the last write, if anything did. */
	flush(): void {
		if (this.#dirty) this.#save();
	}

	#save(): void {
		this.#dirty = false;
		if (!this.#file) return;
		mkdirSync(dirname(this.#file), { recursive: true });
		// Written next to it and renamed, so a crash mid-write leaves the last complete file.
		const temp = `${this.#file}.tmp`;
		writeFileSync(temp, JSON.stringify([...this.#gateways.values()], null, '\t') + '\n', {
			mode: 0o600
		});
		renameSync(temp, this.#file);
	}
}
