/*
 * The welcome's sounds. The rule: a sound only where the screen moves by itself (the intro, the
 * tint washing in, memories arriving), and a soft click when someone picks something. The intro
 * is one sound for the whole scene. When the
 * welcome starts waiting for someone, whatever is still ringing fades out (`quiet`).
 *
 * Each cue is a file in src/lib/assets/sounds/welcome (`burst.mp3`), bundled with a hashed name
 * and fetched on first use; one without its file stays silent, so the welcome works before any
 * sounds exist.
 *
 * Browsers only allow sound after a click. Creating the profile is one, and the welcome is a
 * client-side navigation from there, so the intro can play; opened some other way it starts
 * silent, and the first click wakes it.
 */

/** The intro is one sound, the shimmer; every later moment has its own. */
export const CUES = ['shimmer', 'click', 'confirm', 'wash', 'sparkle', 'chord', 'gather'] as const;
export type Cue = (typeof CUES)[number];

/** A file's name without its folder and extension: the cue it is. */
const cueOf = (path: string) => path.slice(path.lastIndexOf('/') + 1).replace(/\.[^.]+$/, '');

/** The cues' files that exist, by name. */
const FILES = Object.fromEntries(
	Object.entries(
		import.meta.glob<string>('../assets/sounds/welcome/*.{mp3,m4a,wav}', {
			eager: true,
			query: '?url',
			import: 'default'
		})
	).map(([path, url]) => [cueOf(path), url])
) as Partial<Record<Cue, string>>;

/** Semitones of the major pentatonic, so a cascade of sparkles climbs without clashing. */
const PENTATONIC = [0, 2, 4, 7, 9, 12, 14, 16, 19, 21];

let ctx: AudioContext | null = null;
let out: GainNode | null = null;
let enabled = true;
const buffers = new Map<Cue, Promise<AudioBuffer | null>>();
/** What's playing, so it can be faded when the welcome starts waiting. */
const ringing = new Set<{ source: AudioBufferSourceNode; level: GainNode }>();

/** Sounds follow the Sounds setting; turning it off also quiets what's playing. */
export function setSoundsOn(on: boolean): void {
	enabled = on;
	if (!on) quiet(0.3);
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

/**
 * Plays a cue now. `semitones` pitches it (the sparkle cascade), `pan` places it left or right.
 * A cue whose file is still loading plays when it arrives: a short one only if it's still in
 * time, a long one (the shimmer, which swells into the burst) that much further in, to stay in
 * time with the screen.
 */
export function play(cue: Cue, { semitones = 0, pan = 0, gain = 1 } = {}): void {
	const a = audio();
	if (!a) return;
	const asked = a.ctx.currentTime;
	load(cue).then((buffer) => {
		const late = a.ctx.currentTime - asked;
		if (!buffer || !enabled || (late > 0.25 && buffer.duration < 2)) return;
		const source = a.ctx.createBufferSource();
		source.buffer = buffer;
		source.playbackRate.value = 2 ** (semitones / 12);
		const level = a.ctx.createGain();
		level.gain.value = gain;
		const panner = a.ctx.createStereoPanner();
		panner.pan.value = Math.max(-1, Math.min(1, pan));
		source.connect(level).connect(panner).connect(a.out);
		const playing = { source, level };
		ringing.add(playing);
		source.onended = () => ringing.delete(playing);
		source.start(a.ctx.currentTime, late > 0.25 ? late * source.playbackRate.value : 0);
	});
}

/** One memory landing, `i` of `n`: each climbs the scale a little, so the grid fills like a chord. */
export function sparkle(i: number, n: number): void {
	const step = Math.floor((i / Math.max(1, n - 1)) * (PENTATONIC.length - 1));
	play('sparkle', { semitones: PENTATONIC[step], pan: Math.sin(i * 1.7) * 0.6, gain: 0.7 });
}

/** The welcome waits for someone, or is done: whatever is still ringing fades out. */
export function quiet(fade = 1.5): void {
	if (!ctx) return;
	const now = ctx.currentTime;
	for (const { source, level } of ringing) {
		level.gain.cancelScheduledValues(now);
		level.gain.setValueAtTime(level.gain.value, now);
		level.gain.linearRampToValueAtTime(0, now + fade);
		source.stop(now + fade + 0.05);
	}
	ringing.clear();
}
