/*
 * The welcome's sounds: a song, and a sound only where the screen moves by itself (a check
 * passing, the tint washing in) or where an avatar is picked or poked, which yaps in its own
 * voice; other clicks are silent.
 *
 * The song plays the intro, then stays on under the questions, much quieter (`duck`), for as long
 * as they take. When the memories arrive it jumps to its last phrase at full level, and ends by
 * itself over the new chat. It streams through a media element, so the whole song never has to be
 * decoded at once.
 *
 * Each sound is a file in src/lib/assets/sounds/welcome (`wash.mp3`, the song `music.mp3`),
 * bundled with a hashed name and fetched on first use; one without its file stays silent, so the
 * welcome works before any sounds exist.
 *
 * Browsers only allow sound after a click. Creating the profile is one, and the welcome is a
 * client-side navigation from there, so the intro can play; opened some other way it starts
 * silent, and the first click wakes it.
 */

import { AVATARS, type Avatar } from '@btw/core/avatars';

/** The short sounds, played from memory. */
export const CUES = ['confirm', 'wash', 'yap'] as const;
export type Cue = (typeof CUES)[number];
/** A short sound, or the song. */
type Sound = Cue | 'music';

/** How loud the song stays under the questions. */
export const UNDER = 0.15;

/** Where the song's last phrase starts, in seconds: the welcome ends to it. */
export const LAST_PHRASE = 179.8;
/**
 * Moments in the last phrase, in seconds from its start. Its bars fall about every 2.6 seconds:
 * the second, the turn (the fifth, where the phrase heads home), and when the chat starts opening,
 * to be in place on the last note.
 */
export const PHRASE = { second: 2.65, turn: 10.45, open: 15.8 };

/** A file's name without its folder and extension: the sound it is. */
const nameOf = (path: string) => path.slice(path.lastIndexOf('/') + 1).replace(/\.[^.]+$/, '');

/** The sounds' files that exist, by name. */
const FILES = Object.fromEntries(
	Object.entries(
		import.meta.glob<string>('../assets/sounds/welcome/*.{mp3,m4a,wav}', {
			eager: true,
			query: '?url',
			import: 'default'
		})
	).map(([path, url]) => [nameOf(path), url])
) as Partial<Record<Sound, string>>;

let ctx: AudioContext | null = null;
let out: GainNode | null = null;
let enabled = true;
const buffers = new Map<Cue, Promise<AudioBuffer | null>>();
/** What's playing, so it can be faded when the welcome starts waiting. */
const ringing = new Set<{ cue: Cue; source: AudioBufferSourceNode; level: GainNode }>();
/** How often each cue was quieted (all of them under `undefined`), so one on its way stays quiet. */
const hushes = new Map<Cue | undefined, number>();
const hushed = (cue: Cue) => (hushes.get(undefined) ?? 0) + (hushes.get(cue) ?? 0);

/** The song, once there's audio, and the level it's meant to be at. */
let song: { el: HTMLAudioElement; level: GainNode; at: number } | null = null;
/** Bumped by whatever the song is told next, so what it was told before (a start, a stop) lapses. */
let songTurn = 0;
/** Paused by the Sounds setting, to go on where it was when it's turned back on. */
let songMuted = false;

const wait = (ms: number) => new Promise((done) => setTimeout(done, ms));

/** Sounds follow the Sounds setting: off quiets everything, back on picks the song up again. */
export function setSoundsOn(on: boolean): void {
	if (on === enabled) return;
	enabled = on;
	if (!on) {
		const playing = song && !song.el.paused;
		quiet(0.3);
		songMuted = !!playing;
	} else if (songMuted && song && audio()) {
		songMuted = false;
		const s = song;
		const turn = ++songTurn;
		s.el.play().then(
			() => turn === songTurn && ramp(s.level, s.at, 0.6),
			() => {}
		);
	}
}

/** The audio graph, made on first use; null when sounds are off or the browser has no audio. */
function audio(): { ctx: AudioContext; out: GainNode } | null {
	if (!enabled || typeof window === 'undefined') return null;
	if (!ctx) {
		try {
			ctx = new AudioContext();
			out = ctx.createGain();
			out.gain.value = 0.5;
			out.connect(ctx.destination);
			for (const cue of CUES) load(cue);
			if (FILES.music) {
				const el = new Audio(FILES.music);
				el.preload = 'auto';
				const level = ctx.createGain();
				level.gain.value = 0;
				ctx.createMediaElementSource(el).connect(level).connect(out);
				song = { el, level, at: 0 };
			}
		} catch {
			return null;
		}
	}
	if (ctx.state === 'suspended') ctx.resume().catch(() => {});
	return out ? { ctx, out } : null;
}

function load(cue: Cue): Promise<AudioBuffer | null> {
	let found = buffers.get(cue);
	if (!found) {
		const url = FILES[cue];
		found = !url
			? Promise.resolve(null)
			: fetch(url)
					.then((res) => (res.ok ? res.arrayBuffer() : null))
					.then((data) => (data && ctx ? ctx.decodeAudioData(data) : null))
					.catch(() => null);
		buffers.set(cue, found);
	}
	return found;
}

/**
 * Starts loading the sounds, so they're ready when their moments come. Also call it from a click,
 * so a page that started silent can play from then on.
 */
export function wake(): void {
	audio();
}

/** Once the browser lets the page make sound: now, or after the click that wakes it. */
function running(c: AudioContext): Promise<void> {
	if (c.state === 'running') return Promise.resolve();
	return new Promise((ready) => {
		const check = () => {
			if (c.state !== 'running') return;
			c.removeEventListener('statechange', check);
			ready();
		};
		c.addEventListener('statechange', check);
	});
}

/** Moves a level to `to` over `fade` seconds, from wherever it is now. */
function ramp(level: GainNode, to: number, fade: number): void {
	const now = level.context.currentTime;
	level.gain.cancelScheduledValues(now);
	level.gain.setValueAtTime(level.gain.value, now);
	level.gain.linearRampToValueAtTime(to, now + Math.max(0.01, fade));
}

/**
 * Plays a short cue now, `rate` times as fast (and that much higher). One that can't start yet
 * (still loading, or the page not allowed sound until a click) plays when it can, if it's still
 * in time.
 */
export function play(cue: Cue, { rate = 1, gain = 1 } = {}): void {
	const a = audio();
	if (!a) return;
	const asked = performance.now();
	const hush = hushed(cue);
	Promise.all([load(cue), running(a.ctx)]).then(([buffer]) => {
		if (!buffer || !enabled || hushed(cue) !== hush) return;
		if (performance.now() - asked > 250) return;
		const source = a.ctx.createBufferSource();
		source.buffer = buffer;
		source.playbackRate.value = rate;
		const level = a.ctx.createGain();
		level.gain.value = gain;
		source.connect(level).connect(a.out);
		const playing = { cue, source, level };
		ringing.add(playing);
		source.onended = () => ringing.delete(playing);
		source.start();
	});
}

/**
 * Plays the song from `from` seconds in, at `level`: over again if it's already on, dipping out
 * first. `loop` keeps it going round for as long as the questions take; without it, it ends where
 * the song does.
 *
 * Resolves once it's playing, for the screen to start in time with it, or after a second and a
 * half if it can't yet (still loading, or no sound allowed until a click), so the screen doesn't
 * wait on it; it then joins as soon as it can, that much further in.
 */
export function music(from = 0, { level = 1, loop = true } = {}): Promise<void> {
	const a = audio();
	if (!a || !song) return Promise.resolve();
	const s = song;
	const turn = ++songTurn;
	s.at = level;
	s.el.loop = loop;
	songMuted = false;
	/** When the screen started: now, unless it gives up waiting first. */
	let since: number | null = null;
	const playing = (async () => {
		if (!s.el.paused) {
			ramp(s.level, 0, 0.2);
			await wait(200);
		}
		await running(a.ctx);
		if (!enabled || turn !== songTurn) return;
		const late = since === null ? 0 : (performance.now() - since) / 1000;
		s.level.gain.cancelScheduledValues(a.ctx.currentTime);
		s.level.gain.setValueAtTime(0, a.ctx.currentTime);
		s.el.currentTime = from + late;
		// Once it's there, so the screen starts with the music rather than a gap.
		if (s.el.readyState > 0 && s.el.seeking) {
			await Promise.race([
				new Promise((done) => s.el.addEventListener('seeked', done, { once: true })),
				wait(1000)
			]);
		}
		await s.el.play().catch(() => {});
		if (turn === songTurn) ramp(s.level, s.at, 0.25);
	})();
	return Promise.race([playing, wait(1500).then(() => void (since = performance.now()))]);
}

/** Each avatar's voice, in semitones from the yap's own (sped up): a scale from lowest to highest. */
const VOICES = [-5, -3, -1, 0, 2, 4, 5, 7];

/**
 * An avatar yaps in its own voice, a little different each time, so clicking through them sounds
 * like a roll call rather than one sound over and over.
 */
export function yap(avatar: Avatar): void {
	const semitones = VOICES[AVATARS.indexOf(avatar) % VOICES.length] + (Math.random() - 0.5);
	play('yap', { rate: 1.35 * 2 ** (semitones / 12), gain: 0.8 });
}

/** The song goes to its last phrase at full level, and ends by itself over whatever comes next. */
export function lastPhrase(): Promise<void> {
	return music(LAST_PHRASE, { loop: false });
}

/** The song goes on at `level` (`UNDER`, while the welcome waits), over `fade` seconds. */
export function duck(level: number, fade: number): void {
	if (!song) return;
	song.at = level;
	if (!song.el.paused) ramp(song.level, level, fade);
}

/**
 * Whatever is still ringing fades out, or only `sound`; one still on its way stays quiet. The
 * song stops once it's faded.
 */
export function quiet(fade = 1.5, sound?: Sound): void {
	if (sound !== 'music') hushes.set(sound, (hushes.get(sound) ?? 0) + 1);
	if (!ctx) return;
	const now = ctx.currentTime;
	for (const playing of ringing) {
		if (sound && playing.cue !== sound) continue;
		const { source, level } = playing;
		ramp(level, 0, fade);
		source.stop(now + fade + 0.05);
		ringing.delete(playing);
	}
	if (song && (!sound || sound === 'music')) {
		const { el, level } = song;
		const turn = ++songTurn;
		if (el.paused) return;
		ramp(level, 0, fade);
		// Unless it's been started again meanwhile.
		setTimeout(() => turn === songTurn && el.pause(), fade * 1000 + 50);
	}
}
