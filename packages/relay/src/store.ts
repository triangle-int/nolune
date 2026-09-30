import { createHash, timingSafeEqual } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';

/*
 * The gateways the relay knows: a name each, with a hash of its token (never the token itself).
 * Kept in one JSON file, written whole on each change, which suits the few changes a relay sees
 * (registrations and releases; a connection only updates lastSeenAt in memory until the next
 * write). Without a file it lives in memory, for tests.
 */

export interface GatewayRecord {
	name: string;
	/** sha256 of the token, hex. */
	tokenHash: string;
	createdAt: string;
	/** When it last connected. */
	lastSeenAt?: string;
}

export function hashToken(token: string): string {
	return createHash('sha256').update(token).digest('hex');
}

export class GatewayStore {
	readonly #file: string | undefined;
	readonly #gateways = new Map<string, GatewayRecord>();

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

	add(name: string, token: string): GatewayRecord {
		if (this.#gateways.has(name)) throw new Error(`the name ${name} is taken`);
		const record = { name, tokenHash: hashToken(token), createdAt: new Date().toISOString() };
		this.#gateways.set(name, record);
		this.#save();
		return record;
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
		if (record) record.lastSeenAt = new Date().toISOString();
	}

	remove(name: string): void {
		if (this.#gateways.delete(name)) this.#save();
	}

	#save(): void {
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
