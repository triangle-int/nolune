import { randomInt } from 'node:crypto';

/*
 * Gateway names: the first label of a family's address, like cozy-otter-42.nolune.dev.
 */

/**
 * Names no gateway gets: the relay's own and the usual ones of a site's services, which an address
 * could otherwise pretend to be.
 */
const RESERVED = new Set([
	'about',
	'account',
	'accounts',
	'admin',
	'api',
	'app',
	'auth',
	'billing',
	'blog',
	'cdn',
	'dashboard',
	'dev',
	'docs',
	'download',
	'downloads',
	'ftp',
	'help',
	'id',
	'imap',
	'login',
	'mail',
	'news',
	'nolune',
	'ns1',
	'ns2',
	'oauth',
	'official',
	'pay',
	'relay',
	'root',
	'security',
	'signin',
	'signup',
	'smtp',
	'staging',
	'static',
	'status',
	'support',
	'team',
	'test',
	'verify',
	'www'
]);

export function isReservedName(name: string): boolean {
	return RESERVED.has(name);
}

const ADJECTIVES = [
	'amber',
	'brave',
	'bright',
	'calm',
	'cozy',
	'clever',
	'curious',
	'dreamy',
	'gentle',
	'golden',
	'happy',
	'jolly',
	'kind',
	'lively',
	'lucky',
	'merry',
	'misty',
	'quiet',
	'rosy',
	'sunny',
	'swift',
	'tidy',
	'velvet',
	'warm'
];

const NOUNS = [
	'badger',
	'comet',
	'cloud',
	'fox',
	'harbor',
	'heron',
	'lantern',
	'maple',
	'meadow',
	'moon',
	'orbit',
	'otter',
	'owl',
	'panda',
	'pebble',
	'planet',
	'robin',
	'river',
	'rocket',
	'star',
	'teapot',
	'tulip',
	'willow',
	'wren'
];

/** A friendly name nobody picked, like `cozy-otter-42`. It may be taken: check before using it. */
export function randomName(): string {
	const pick = (words: string[]) => words[randomInt(words.length)];
	return `${pick(ADJECTIVES)}-${pick(NOUNS)}-${randomInt(10, 100)}`;
}
