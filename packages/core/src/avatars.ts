/**
 * Assistant avatars: a small one-color mascot per profile, drawn on a 24×24 grid in
 * `currentColor`. Holes (`cut`, and the eyes on shapes marked `eyes`) are knocked out with a mask,
 * so the page behind shows through in both themes, and the eyes stay separate shapes that can
 * blink and look around.
 */

export const AVATARS = [
	'probe',
	'campfire',
	'lantern',
	'planet',
	'quantum',
	'comet',
	'moon',
	'satellite'
] as const;

export type Avatar = (typeof AVATARS)[number];

/** Each avatar's color in the light and dark themes, as the web's layout.css has them. */
export type AvatarColors = Record<Avatar, { light: string; dark: string }>;

/** The `--avatar-*` colors in a stylesheet: light (`:root`) first, then dark (`.dark`). */
export function parseAvatarColors(stylesheet: string): AvatarColors {
	return Object.fromEntries(
		AVATARS.map((avatar) => {
			const [light, dark] = [
				...stylesheet.matchAll(new RegExp(`--avatar-${avatar}:\\s*(#[0-9a-f]{3,8})`, 'gi'))
			].map((match) => match[1]);
			return [avatar, { light, dark }];
		})
	) as AvatarColors;
}

export function isAvatar(value: unknown): value is Avatar {
	return typeof value === 'string' && (AVATARS as readonly string[]).includes(value);
}

/**
 * The avatar a profile starts with, picked from its slug so profiles differ without anyone
 * choosing: a 32-bit FNV-1a hash of the slug's characters, mixed once more so slugs that differ
 * only at the end (family-2, family-3) still spread, whose bits from 16 up pick one. The migration
 * that added avatars computes the same in SQL (0008_avatars.sql): keep them in step.
 */
export function defaultAvatar(slug: string): Avatar {
	let hash = 2166136261;
	for (const char of slug) hash = Math.imul(hash ^ char.codePointAt(0)!, 16777619) >>> 0;
	hash = Math.imul(hash ^ (hash >>> 13), 16777619) >>> 0;
	return AVATARS[(hash >>> 16) % AVATARS.length];
}

/** One filled shape, drawn in order. */
export interface Shape {
	d: string;
	/** Names the shape for per-avatar motion ("flame", "tip", "dash", ...). */
	part?: string;
	/** Knocked out of this shape. */
	cut?: string;
	/** The eyes are knocked out of this shape. */
	eyes?: boolean;
}

/** An eye, centered on (cx, cy): an oval, or any shape in `d`. */
export interface Eye {
	cx: number;
	cy: number;
	rx?: number;
	ry?: number;
	d?: string;
}

export interface Glyph {
	shapes: Shape[];
	eyes: Eye[];
	/** The eyes are drawn (in a hole) instead of knocked out. */
	solidEyes?: boolean;
}

// --- shape helpers (path data, rounded to 2 decimals) ---

type Point = [number, number];

const n = (v: number) => String(Math.round(v * 100) / 100);
const rad = (deg: number) => (deg * Math.PI) / 180;

function ellipse(cx: number, cy: number, rx: number, ry: number, rot = 0): string {
	const [dx, dy] = [rx * Math.cos(rad(rot)), rx * Math.sin(rad(rot))];
	const a = `${n(rx)} ${n(ry)} ${n(rot)}`;
	return `M${n(cx - dx)} ${n(cy - dy)}A${a} 1 0 ${n(cx + dx)} ${n(cy + dy)}A${a} 1 0 ${n(cx - dx)} ${n(cy - dy)}Z`;
}

function circle(cx: number, cy: number, r: number): string {
	return ellipse(cx, cy, r, r);
}

/** A line with round ends, `r` thick on each side. */
function capsule(x1: number, y1: number, x2: number, y2: number, r: number): string {
	const len = Math.hypot(x2 - x1, y2 - y1);
	const [nx, ny] = [(-(y2 - y1) / len) * r, ((x2 - x1) / len) * r];
	return (
		`M${n(x1 + nx)} ${n(y1 + ny)}L${n(x2 + nx)} ${n(y2 + ny)}` +
		`A${n(r)} ${n(r)} 0 0 0 ${n(x2 - nx)} ${n(y2 - ny)}L${n(x1 - nx)} ${n(y1 - ny)}` +
		`A${n(r)} ${n(r)} 0 0 0 ${n(x1 + nx)} ${n(y1 + ny)}Z`
	);
}

/** A polygon with each corner rounded to radius `r` (or the point's own third value). */
function roundPoly(corners: (Point | [number, number, number])[], r: number): string {
	// Every helper winds counter-clockwise, so shapes joined in one path add up instead of
	// cancelling where they overlap.
	const area = corners.reduce((sum, [x, y], i) => {
		const [x2, y2] = corners[(i + 1) % corners.length];
		return sum + x * y2 - x2 * y;
	}, 0);
	const points = area > 0 ? [...corners].reverse() : corners;
	const at = (i: number) => points[(i + points.length) % points.length];
	let d = '';
	points.forEach((p, i) => {
		const [a, b] = [at(i - 1), at(i + 1)];
		const u = [a[0] - p[0], a[1] - p[1]];
		const v = [b[0] - p[0], b[1] - p[1]];
		const [lu, lv] = [Math.hypot(u[0], u[1]), Math.hypot(v[0], v[1])];
		const angle = Math.acos((u[0] * v[0] + u[1] * v[1]) / (lu * lv));
		const radius = p[2] ?? r;
		const t = Math.min(radius / Math.tan(angle / 2), lu / 2, lv / 2);
		const turn = (p[0] - a[0]) * (b[1] - p[1]) - (p[1] - a[1]) * (b[0] - p[0]);
		const start = [p[0] + (u[0] / lu) * t, p[1] + (u[1] / lu) * t];
		const end = [p[0] + (v[0] / lv) * t, p[1] + (v[1] / lv) * t];
		const ar = t * Math.tan(angle / 2);
		d += `${i ? 'L' : 'M'}${n(start[0])} ${n(start[1])}A${n(ar)} ${n(ar)} 0 0 ${turn > 0 ? 1 : 0} ${n(end[0])} ${n(end[1])}`;
	});
	return d + 'Z';
}

/** A rounded rectangle centered on (cx, cy), turned `rot` degrees clockwise. */
function rect(cx: number, cy: number, w: number, h: number, r: number, rot = 0): string {
	const [c, s] = [Math.cos(rad(rot)), Math.sin(rad(rot))];
	const corner = (x: number, y: number): Point => [cx + x * c - y * s, cy + x * s + y * c];
	return roundPoly(
		[corner(-w / 2, -h / 2), corner(w / 2, -h / 2), corner(w / 2, h / 2), corner(-w / 2, h / 2)],
		r
	);
}

/**
 * A band `w` wide along a circle, from angle `a0` clockwise to `a1` (degrees, 0 = right), with
 * square or round ends.
 */
function arcBand(
	cx: number,
	cy: number,
	r: number,
	w: number,
	a0: number,
	a1: number,
	round = false
): string {
	const at = (radius: number, a: number) =>
		`${n(cx + radius * Math.cos(rad(a)))} ${n(cy + radius * Math.sin(rad(a)))}`;
	const [o, i] = [r + w / 2, r - w / 2];
	const large = a1 - a0 > 180 ? 1 : 0;
	const end = (a: number) =>
		round ? `A${n(w / 2)} ${n(w / 2)} 0 0 0 ${at(i, a)}` : `L${at(i, a)}`;
	const close = round ? `A${n(w / 2)} ${n(w / 2)} 0 0 0 ${at(o, a1)}` : '';
	return (
		`M${at(o, a1)}A${n(o)} ${n(o)} 0 ${large} 0 ${at(o, a0)}` +
		`${end(a0)}A${n(i)} ${n(i)} 0 ${large} 1 ${at(i, a1)}${close}Z`
	);
}

type Ellipse = [cx: number, cy: number, rx: number, ry: number, rot: number];

/** Which side of the line a → b a point is on: positive below it. */
function side(a: Point, b: Point, [x, y]: Point): number {
	return (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0]);
}

/** Where the line through a and b crosses the ellipse, in the order a → b. */
function crossings([cx, cy, rx, ry, rot]: Ellipse, a: Point, b: Point): [Point, Point] {
	const [c, s] = [Math.cos(rad(rot)), Math.sin(rad(rot))];
	const local = ([x, y]: Point) => [
		((x - cx) * c + (y - cy) * s) / rx,
		(-(x - cx) * s + (y - cy) * c) / ry
	];
	const [p, q] = [local(a), local(b)];
	const [dx, dy] = [q[0] - p[0], q[1] - p[1]];
	const [qa, qb, qc] = [dx * dx + dy * dy, 2 * (p[0] * dx + p[1] * dy), p[0] ** 2 + p[1] ** 2 - 1];
	const root = Math.sqrt(qb * qb - 4 * qa * qc);
	const at = (t: number): Point => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
	return [at((-qb - root) / (2 * qa)), at((-qb + root) / (2 * qa))];
}

/**
 * The part of a ring (inside `outer`, outside `inner`) on one side of the line a → b: below it
 * (`near`) or above it.
 */
function ringPart(outer: Ellipse, inner: Ellipse, a: Point, b: Point, near: boolean): string {
	const arc = (e: Ellipse, from: Point, to: Point, sweep: number) => {
		const large = side(a, b, [e[0], e[1]]) * (near ? 1 : -1) > 1e-6 ? 1 : 0;
		return `M${n(from[0])} ${n(from[1])}A${n(e[2])} ${n(e[3])} ${n(e[4])} ${large} ${sweep} ${n(to[0])} ${n(to[1])}`;
	};
	const [o1, o2] = near ? crossings(outer, a, b) : crossings(outer, b, a);
	const [i1, i2] = near ? crossings(inner, a, b) : crossings(inner, b, a);
	return arc(outer, o1, o2, 0) + arc(inner, i2, i1, 1).replace('M', 'L') + 'Z';
}

/** A circle with a bite taken out by another, the two horns rounded to radius `t`. */
function crescent([ox, oy, R]: number[], [ix, iy, r]: number[], t: number): string {
	// Each horn is rounded by a circle of radius t touching the outer circle inside and the inner
	// one outside: its center is R - t from the outer center and r + t from the inner one.
	const [dx, dy] = [ix - ox, iy - oy];
	const d = Math.hypot(dx, dy);
	const [a, b] = [R - t, r + t];
	const along = (a * a - b * b + d * d) / (2 * d);
	const h = Math.sqrt(a * a - along * along);
	const [mx, my] = [ox + (dx / d) * along, oy + (dy / d) * along];
	const tips = [-1, 1].map((s) => [mx + (s * -dy * h) / d, my + (s * dx * h) / d]);
	const [o1, o2] = tips.map(([x, y]) => [ox + ((x - ox) * R) / a, oy + ((y - oy) * R) / a]);
	const [i1, i2] = tips.map(([x, y]) => [ix + ((x - ix) * r) / b, iy + ((y - iy) * r) / b]);
	const p = (q: number[]) => `${n(q[0])} ${n(q[1])}`;
	return (
		`M${p(o1)}A${n(R)} ${n(R)} 0 1 0 ${p(o2)}A${n(t)} ${n(t)} 0 0 0 ${p(i2)}` +
		`A${n(r)} ${n(r)} 0 0 1 ${p(i1)}A${n(t)} ${n(t)} 0 0 0 ${p(o1)}Z`
	);
}

// --- the set, redrawn from the concept sheet ---

const PLANET = [11.9, 12.05, 7.7];
const RING_OUT: Ellipse = [12.1, 12.9, 11.6, 3.8, -13];
const RING_IN: Ellipse = [12.0, 12.4, 9.0, 2.8, -14];
const RING_GAP: Ellipse = [12.0, 12.4, 9.0 - 0.85, 2.8 - 0.85, -14];
const RING_ENDS: [Point, Point] = [
	[12.1 - 11.6 * Math.cos(rad(13)), 12.9 + 11.6 * Math.sin(rad(13))],
	[12.1 + 11.6 * Math.cos(rad(13)), 12.9 - 11.6 * Math.sin(rad(13))]
];

const LOGS = [
	[5.4, 17.6, 18.4, 21.65],
	[18.75, 17.45, 5.85, 21.8]
];
const logs = (grow: number) =>
	LOGS.map(([x1, y1, x2, y2]) => capsule(x1, y1, x2, y2, 1.38 + grow)).join('');

// Comet tails run up and to the right at this slope.
const TAIL = Math.tan(rad(24.6));

export const GLYPHS: Record<Avatar, Glyph> = {
	probe: {
		shapes: [
			{
				d:
					circle(12.05, 14, 7.6) +
					circle(6.3, 20.05, 1.6) +
					circle(11.85, 21.65, 1.6) +
					circle(17.75, 20.05, 1.6),
				cut: circle(12.5, 13.78, 4.15)
			},
			{ d: capsule(13.4, 7.3, 15.55, 2.95, 0.5) },
			{ d: circle(15.95, 2.25, 1.45), part: 'tip' }
		],
		solidEyes: true,
		eyes: [
			{ cx: 11.85, cy: 14.3, rx: 1.08, ry: 1.68 },
			{ cx: 14.7, cy: 13.97, rx: 1.0, ry: 1.63 }
		]
	},
	campfire: {
		shapes: [
			{
				part: 'flame',
				eyes: true,
				cut: logs(0.9),
				d:
					'M12.35 0.75C14.3 1.7 15.65 3.3 15.65 5.1C15.65 6.2 15.3 6.9 15.45 7.6' +
					'C15.55 8.2 16.2 8.5 16.5 7.9C16.8 7.4 17.05 6.9 17.3 6.55C17.5 6.3 17.8 6.35 17.95 6.6' +
					'C18.95 8.1 19.5 9.7 19.5 11.7C19.5 15.6 16.3 18.8 12.35 18.8C8.4 18.8 5.2 15.6 5.2 12' +
					'C5.2 8.8 7.3 6.6 9.5 4.95C10.9 3.9 11.95 3.3 11.95 1.9C11.95 1.3 11.85 0.9 12.35 0.75Z'
			},
			{ d: logs(0), part: 'logs' }
		],
		eyes: [
			{ cx: 10.09, cy: 12.63, rx: 1.25, ry: 1.72 },
			{ cx: 14.58, cy: 12.62, rx: 1.25, ry: 1.68 }
		]
	},
	lantern: {
		shapes: [
			{
				eyes: true,
				d:
					ellipse(11.3, 14.33, 7.77, 7.4) +
					roundPoly(
						[
							[5.6, 12.6, 0],
							[0.55, 9.6, 1.1],
							[1.45, 14.45, 1.2],
							[0.55, 19.1, 1.1],
							[5.6, 16.4, 0]
						],
						1
					)
			},
			{ d: arcBand(17.72, 6.93, 3.92, 1.3, 168, 335) },
			{ d: circle(21.27, 7.6, 2.05), part: 'lure' }
		],
		eyes: [
			{ cx: 11.99, cy: 14.34, rx: 1.2, ry: 1.71 },
			{ cx: 15.82, cy: 13.79, rx: 1.16, ry: 1.67 }
		]
	},
	planet: {
		shapes: [
			{
				d: ringPart(RING_OUT, RING_IN, ...RING_ENDS, false),
				cut: circle(PLANET[0], PLANET[1], PLANET[2] + 0.5),
				part: 'ring'
			},
			{
				d: circle(PLANET[0], PLANET[1], PLANET[2]),
				eyes: true,
				cut: ringPart(RING_IN, RING_GAP, ...RING_ENDS, true)
			},
			{ d: ringPart(RING_OUT, RING_IN, ...RING_ENDS, true), part: 'ring' }
		],
		eyes: [
			{ cx: 10.7, cy: 11.48, rx: 1.15, ry: 1.53 },
			{ cx: 14.64, cy: 10.56, rx: 1.1, ry: 1.49 }
		]
	},
	quantum: {
		shapes: [
			{
				eyes: true,
				d: roundPoly(
					[
						[12.15, -0.3, 0.7],
						[12.15, 5.45, 0.2],
						[13.8, 5.45, 0.5],
						[13.8, 17.35, 0.6],
						[12.23, 17.35, 0.2],
						[12.23, 24.3, 0.57],
						[3.2, 14.49, 0.8],
						[3.2, 8.36, 0.8]
					],
					0.8
				)
			},
			{ d: capsule(13.9, 3.66, 15.8, 3.66, 0.74), part: 'dash' },
			{ d: capsule(16.14, 6.29, 18.18, 6.29, 0.78), part: 'dash' },
			{
				d: capsule(15.6, 9.33, 16.98, 9.33, 0.78) + capsule(19.56, 9.33, 20.02, 9.33, 0.78),
				part: 'dash'
			},
			{ d: capsule(15.68, 12.83, 18.18, 12.83, 0.78), part: 'dash' },
			{
				d: capsule(15.55, 15.46, 15.92, 15.46, 0.74) + capsule(18.4, 15.46, 20.16, 15.46, 0.74),
				part: 'dash'
			},
			{ d: capsule(14.63, 18.04, 17.02, 18.04, 0.74), part: 'dash' },
			{ d: capsule(13.89, 20.62, 15.09, 20.62, 0.74), part: 'dash' }
		],
		eyes: [
			{ cx: 6.99, cy: 11.95, rx: 1.2, ry: 1.66 },
			{ cx: 10.76, cy: 11.56, rx: 1.2, ry: 1.61 }
		]
	},
	comet: {
		shapes: [
			{ d: circle(7.55, 14.2, 6.7), eyes: true },
			// Each tail starts inside the head, clear of the eyes, so it can stream on its own.
			{ d: capsule(8, 6.34 + 6 * TAIL, 18.73, 4.17, 1.1), part: 'tail' },
			{ d: capsule(11, 10.54 + 3 * TAIL, 22.25, 6.77, 1.0), part: 'tail' },
			{ d: capsule(12, 13.97 + 3 * TAIL, 20.2, 11.59, 1.0), part: 'tail' }
		],
		eyes: [
			{ cx: 4.31, cy: 15.15, rx: 1.14, ry: 1.55 },
			{ cx: 8.82, cy: 14.5, rx: 1.19, ry: 1.6 }
		]
	},
	moon: {
		shapes: [{ eyes: true, d: crescent([12.99, 12.18, 11.05], [20.59, 9.21, 8.84], 0.35) }],
		eyes: [
			{
				cx: 8,
				cy: 13.15,
				d: arcBand(8, 12.35, 1.62, 1.15, 0, 180, true)
			}
		]
	},
	satellite: {
		shapes: [
			{
				eyes: true,
				d:
					circle(12.05, 14.15, 5.2) +
					capsule(12.1, 6, 12.1, 9.5, 0.4) +
					capsule(5, 13.9, 19.2, 13.9, 0.62)
			},
			{ d: circle(12.1, 5.9, 1.18), part: 'tip' },
			{ d: rect(3.55, 13.75, 4.8, 7.2, 0.6, 9), part: 'panel' },
			{ d: rect(20.65, 13.75, 4.8, 7.2, 0.6, -9), part: 'panel' }
		],
		eyes: [
			{ cx: 10.4, cy: 14.36, rx: 1.12, ry: 1.5 },
			{ cx: 13.8, cy: 14.34, rx: 1.12, ry: 1.5 }
		]
	}
};

export function eyePath(e: Eye): string {
	return e.d ?? ellipse(e.cx, e.cy, e.rx ?? 1.2, e.ry ?? 1.65);
}

/** A standalone SVG of the glyph, for favicons and previews. */
export function avatarSvg(avatar: Avatar, fill = 'currentColor', id = 'a'): string {
	const g = GLYPHS[avatar];
	const eyes = g.eyes.map((e) => `<path d="${eyePath(e)}"/>`).join('');
	const shapes = g.shapes
		.map((s, i) => {
			const holes = (s.cut ? `<path d="${s.cut}"/>` : '') + (s.eyes && !g.solidEyes ? eyes : '');
			if (!holes) return `<path d="${s.d}"/>`;
			const m = `${id}${i}`;
			return (
				`<mask id="${m}"><rect width="24" height="24" fill="#fff"/><g fill="#000">${holes}</g></mask>` +
				`<path d="${s.d}" mask="url(#${m})"/>`
			);
		})
		.join('');
	return (
		`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="${fill}">` +
		shapes +
		(g.solidEyes ? eyes : '') +
		`</svg>`
	);
}
