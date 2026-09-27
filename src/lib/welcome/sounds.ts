/*
 * The welcome's sounds. The rule: a sound only where the screen moves by itself (the intro, the
 * tint washing in, memories arriving), and a soft click when someone picks something. While a
 * step waits for them, it stays quiet.
 *
 * Each cue is a file in src/lib/assets/sounds/welcome (`burst.mp3`), bundled with a hashed name and
 * fetched on first use; a cue without its file stays silent, so the welcome works before any
 * sounds exist. `pad` loops under the intro and the steps.
 *
 * Browsers only allow sound after a click. Creating the profile is one, and the welcome is a
 * client-side navigation from there, so the intro can play; opened some other way it starts
 * silent, and the first click wakes it.
 */

export const CUES = [
	'pad',
	'shimmer',
	'trace',
	'tick',
	'burst',
	'swell',
	'click',
	'confirm',
	'wash',
	'sparkle',
	'chord',
	'gather'
] as const;
export type Cue = (typeof CUES)[number];

/** The cues' files that exist, by name. */
const FILES = Object.fromEntries(
	Object.entries(
		import.meta.glob<string>('../assets/sounds/welcome/*.{mp3,m4a,wav}', {
			eager: true,
			query: '?url',
			import: 'default'
		})
	).map(([path, url]) => [
		path
			.split('/')
			.pop()!
			.replace(/\.[^.]+$/, ''),
		url
	])
) as Partial<Record<Cue, string>>;

/** Semitones of the major pentatonic, so a cascade of sparkles climbs without clashing. */
const PENTATONIC = [0, 2, 4, 7, 9, 12, 14, 16, 19, 21];

let ctx: AudioContext | null = null;
let out: GainNode | null = null;
let enabled = true;
const buffers = new Map<Cue, Promise<AudioBuffer | null>>();
let pad: { source: AudioBufferSourceNode; gain: GainNode } | null = null;
let padWanted = false;

/** Sounds follow the Sounds setting; turning it off also stops the pad. */
export function setSoundsOn(on: boolean): void {
	enabled = on;
	if (!on) stopPad(0.3);
}

/** The audio graph, made on first use; null when sounds are off or the browser has no audio. */
function audio(): { ctx: AudioContext; out: GainNode } | null {
	if (!enabled || typeof window === 'undefined') return null;
	if (!ctx) {
		try {
			ctx = new AudioContext();
			out = ctx.createGain();
			out.gain.value = 0.25;
			out.connect(ctx.destination);
			for (const cue of CUES) load(cue);
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

/** Call from a click, so a page that started silent can play from then on. */
export function wake(): void {
	if (audio() && padWanted) startPad();
}

/**
 * Plays a cue now. `semitones` pitches it (the sparkle cascade), `pan` places it left or right.
 * A cue whose file is still loading plays when it arrives, unless that's too late to matter.
 */
export function play(cue: Cue, { semitones = 0, pan = 0, gain = 1 } = {}): void {
	const a = audio();
	if (!a) return;
	const asked = a.ctx.currentTime;
	load(cue).then((buffer) => {
		if (!buffer || !enabled || a.ctx.currentTime - asked > 0.25) return;
		const source = a.ctx.createBufferSource();
		source.buffer = buffer;
		source.playbackRate.value = 2 ** (semitones / 12);
		const level = a.ctx.createGain();
		level.gain.value = gain;
		const panner = a.ctx.createStereoPanner();
		panner.pan.value = Math.max(-1, Math.min(1, pan));
		source.connect(level).connect(panner).connect(a.out);
		source.start();
	});
}

/** One memory landing, `i` of `n`: each climbs the scale a little, so the grid fills like a chord. */
export function sparkle(i: number, n: number): void {
	const step = Math.floor((i / Math.max(1, n - 1)) * (PENTATONIC.length - 1));
	play('sparkle', { semitones: PENTATONIC[step], pan: Math.sin(i * 1.7) * 0.6, gain: 0.7 });
}

/** The quiet bed under the intro and the steps, looped until stopPad. */
export function startPad(): void {
	padWanted = true;
	const a = audio();
	if (!a || pad) return;
	load('pad').then((buffer) => {
		if (!buffer || pad || !padWanted || !enabled) return;
		const source = a.ctx.createBufferSource();
		source.buffer = buffer;
		source.loop = true;
		const gain = a.ctx.createGain();
		const now = a.ctx.currentTime;
		gain.gain.setValueAtTime(0, now);
		gain.gain.linearRampToValueAtTime(0.6, now + 2.5);
		source.connect(gain).connect(a.out);
		source.start();
		pad = { source, gain };
	});
}

export function stopPad(fade = 2): void {
	padWanted = false;
	if (!pad || !ctx) return;
	const { source, gain } = pad;
	const now = ctx.currentTime;
	gain.gain.cancelScheduledValues(now);
	gain.gain.setValueAtTime(gain.gain.value, now);
	gain.gain.linearRampToValueAtTime(0, now + fade);
	source.stop(now + fade + 0.05);
	pad = null;
}
