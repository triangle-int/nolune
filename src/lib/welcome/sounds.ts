/*
 * The welcome's sounds. The rule: a sound only where the screen moves by itself (the intro, the
 * tint washing in, memories arriving), and a soft click when someone picks something. While a
 * step waits for them, it stays quiet.
 *
 * Music plays under those moments and fades out whenever the welcome waits for someone; the next
 * moment picks it up at its own cut (MUSIC). Effects play on top.
 *
 * Each cue is a file in src/lib/assets/sounds/welcome (`burst.mp3`, `music.mp3`), bundled with a
 * hashed name and fetched on first use; one without its file stays silent, so the welcome works
 * before any sounds exist.
 *
 * Browsers only allow sound after a click. Creating the profile is one, and the welcome is a
 * client-side navigation from there, so the intro can play; opened some other way it starts
 * silent, and the first click wakes it.
 */

export const CUES = [
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

/**
 * Where each moment that plays by itself starts in the music, in seconds, cut to the song's
 * sections: the intro rises out of silence with it, the tint washes in on the lift at 23.7 s, and
 * memories take flight on the drop at 42 s, which comes 0.9 s after the arrival opens, as they
 * start to fly. For "Wistful Melodic Arc" (music.mp3, its first 56 s); another song needs its own.
 */
export const MUSIC = { intro: 0, hello: 23.45, arrival: 41.1 } as const;
export type MusicCut = keyof typeof MUSIC;
/** Under the effects, and well under anyone's own music. */
const MUSIC_LEVEL = 0.5;

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
) as Partial<Record<Cue | 'music', string>>;

/** Semitones of the major pentatonic, so a cascade of sparkles climbs without clashing. */
const PENTATONIC = [0, 2, 4, 7, 9, 12, 14, 16, 19, 21];

let ctx: AudioContext | null = null;
let out: GainNode | null = null;
let enabled = true;
const buffers = new Map<Cue | 'music', Promise<AudioBuffer | null>>();
let music: { source: AudioBufferSourceNode; gain: GainNode } | null = null;
/** Bumped by every play and fade, so music that finishes loading late doesn't start after all. */
let musicTurn = 0;

/** Sounds follow the Sounds setting; turning it off also fades the music out. */
export function setSoundsOn(on: boolean): void {
	enabled = on;
	if (!on) fadeOutMusic(0.3);
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

function load(cue: Cue | 'music'): Promise<AudioBuffer | null> {
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
 * Starts loading everything, so the music is ready when its moments come. Also call it from a
 * click, so a page that started silent can play from then on.
 */
export function wake(): void {
	audio();
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

/**
 * Plays the music from a moment's cut, fading in over `fadeIn` seconds. Starting late (the file
 * was still loading) starts that much further in, so it stays in time with the screen.
 */
export function playMusic(cut: MusicCut, fadeIn = 0.4): void {
	const a = audio();
	if (!a) return;
	fadeOutMusic(0.3);
	const turn = ++musicTurn;
	const asked = a.ctx.currentTime;
	load('music').then((buffer) => {
		if (!buffer || !enabled || turn !== musicTurn) return;
		const now = a.ctx.currentTime;
		const source = a.ctx.createBufferSource();
		source.buffer = buffer;
		const gain = a.ctx.createGain();
		gain.gain.setValueAtTime(0, now);
		gain.gain.linearRampToValueAtTime(MUSIC_LEVEL, now + fadeIn);
		source.connect(gain).connect(a.ctx.destination);
		source.start(now, MUSIC[cut] + Math.min(now - asked, 2));
		music = { source, gain };
	});
}

/** The welcome waits for someone, or is done: the music fades out. Once; later calls do nothing. */
export function fadeOutMusic(fade = 1.5): void {
	musicTurn++;
	if (!music || !ctx) return;
	const { source, gain } = music;
	const now = ctx.currentTime;
	gain.gain.cancelScheduledValues(now);
	gain.gain.setValueAtTime(gain.gain.value, now);
	gain.gain.linearRampToValueAtTime(0, now + fade);
	source.stop(now + fade + 0.05);
	music = null;
}
