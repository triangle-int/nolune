import { AVATARS, GLYPHS, eyePath, type Avatar } from './avatars.ts';

/**
 * The avatars for nolune for iOS (ios/Shared/Avatars.swift), which draws them with SwiftUI's
 * paths: those have no elliptical arcs, so each `A` becomes cubic curves, and every path is left
 * with absolute M, L, C and Z. `node scripts/ios-avatars.ts` writes the file; a test checks it's
 * current.
 */

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

const r2 = (v: number) => {
	const rounded = Math.round(v * 100) / 100;
	return String(Object.is(rounded, -0) ? 0 : rounded);
};

/** Cubic curves for an SVG arc, as the SVG spec converts one to center form (F.6.5). */
export function arcToCubics(
	x1: number,
	y1: number,
	rxIn: number,
	ryIn: number,
	rotation: number,
	large: boolean,
	sweep: boolean,
	x2: number,
	y2: number
): number[][] {
	if (x1 === x2 && y1 === y2) return [];
	let [rx, ry] = [Math.abs(rxIn), Math.abs(ryIn)];
	if (rx === 0 || ry === 0) return [[x1, y1, x2, y2, x2, y2]];
	const phi = (rotation * Math.PI) / 180;
	const [cos, sin] = [Math.cos(phi), Math.sin(phi)];
	const [dx, dy] = [(x1 - x2) / 2, (y1 - y2) / 2];
	const x1p = cos * dx + sin * dy;
	const y1p = -sin * dx + cos * dy;
	const lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
	if (lambda > 1) [rx, ry] = [rx * Math.sqrt(lambda), ry * Math.sqrt(lambda)];
	const num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
	const den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
	const coef = (large !== sweep ? 1 : -1) * Math.sqrt(Math.max(0, num / den));
	const cxp = (coef * rx * y1p) / ry;
	const cyp = (-coef * ry * x1p) / rx;
	const cx = cos * cxp - sin * cyp + (x1 + x2) / 2;
	const cy = sin * cxp + cos * cyp + (y1 + y2) / 2;
	const angle = (ux: number, uy: number, vx: number, vy: number) => {
		const dot = (ux * vx + uy * vy) / (Math.hypot(ux, uy) * Math.hypot(vx, vy));
		const a = Math.acos(Math.min(1, Math.max(-1, dot)));
		return ux * vy - uy * vx < 0 ? -a : a;
	};
	const theta = angle(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry);
	let delta = angle((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
	if (!sweep && delta > 0) delta -= 2 * Math.PI;
	if (sweep && delta < 0) delta += 2 * Math.PI;
	const segments = Math.max(1, Math.ceil(Math.abs(delta) / (Math.PI / 2) - 1e-9));
	const step = delta / segments;
	const t = (4 / 3) * Math.tan(step / 4);
	const point = (a: number) => [
		cx + rx * Math.cos(a) * cos - ry * Math.sin(a) * sin,
		cy + rx * Math.cos(a) * sin + ry * Math.sin(a) * cos
	];
	const tangent = (a: number) => [
		-rx * Math.sin(a) * cos - ry * Math.cos(a) * sin,
		-rx * Math.sin(a) * sin + ry * Math.cos(a) * cos
	];
	const curves: number[][] = [];
	for (let i = 0; i < segments; i++) {
		const [a1, a2] = [theta + i * step, theta + (i + 1) * step];
		const [p1, p2] = [point(a1), point(a2)];
		const [d1, d2] = [tangent(a1), tangent(a2)];
		curves.push([
			p1[0] + t * d1[0],
			p1[1] + t * d1[1],
			p2[0] - t * d2[0],
			p2[1] - t * d2[1],
			p2[0],
			p2[1]
		]);
	}
	// The last point is exactly where the arc ends, not where rounding put it.
	curves[curves.length - 1][4] = x2;
	curves[curves.length - 1][5] = y2;
	return curves;
}

/** The path with only absolute M, L, C and Z: its arcs as curves. */
export function withoutArcs(d: string): string {
	const tokens = d.match(/[A-Za-z]|-?\d*\.?\d+(?:e-?\d+)?/g) ?? [];
	let i = 0;
	const num = () => Number(tokens[i++]);
	let [x, y, startX, startY] = [0, 0, 0, 0];
	let command = '';
	const out: string[] = [];
	while (i < tokens.length) {
		if (/[A-Za-z]/.test(tokens[i])) command = tokens[i++];
		if (command !== command.toUpperCase()) throw new Error(`Relative path command ${command}`);
		switch (command) {
			case 'M':
				[x, y] = [num(), num()];
				[startX, startY] = [x, y];
				out.push(`M${r2(x)} ${r2(y)}`);
				command = 'L';
				break;
			case 'L':
				[x, y] = [num(), num()];
				out.push(`L${r2(x)} ${r2(y)}`);
				break;
			case 'C': {
				const c = [num(), num(), num(), num(), num(), num()];
				out.push(`C${c.map(r2).join(' ')}`);
				[x, y] = [c[4], c[5]];
				break;
			}
			case 'A': {
				const [rx, ry, rot, large, sweep, ex, ey] = [
					num(),
					num(),
					num(),
					num(),
					num(),
					num(),
					num()
				];
				for (const c of arcToCubics(x, y, rx, ry, rot, large === 1, sweep === 1, ex, ey)) {
					out.push(`C${c.map(r2).join(' ')}`);
				}
				[x, y] = [ex, ey];
				break;
			}
			case 'Z':
				out.push('Z');
				[x, y] = [startX, startY];
				break;
			default:
				throw new Error(`Path command ${command}`);
		}
	}
	return out.join('');
}

const swiftColor = (hex: string) => `Color(hex: 0x${hex.slice(1).toUpperCase()})`;

/** Avatars.swift, from the glyphs and their colors. */
export function iosAvatarSource(colors: AvatarColors): string {
	const glyphs = AVATARS.map((avatar) => {
		const g = GLYPHS[avatar];
		const eyes = g.eyes.map((e) => withoutArcs(eyePath(e))).join('');
		const shapes = g.shapes.map((s) => {
			const holes = (s.cut ? withoutArcs(s.cut) : '') + (s.eyes && !g.solidEyes ? eyes : '');
			return `\t\t\t\t.init(path: "${withoutArcs(s.d)}", holes: ${holes ? `"${holes}"` : 'nil'})`;
		});
		return [
			`\t\t"${avatar}": .init(`,
			`\t\t\tshapes: [`,
			shapes.join(',\n'),
			`\t\t\t],`,
			`\t\t\teyes: ${g.solidEyes ? `"${eyes}"` : 'nil'},`,
			`\t\t\tlight: ${swiftColor(colors[avatar].light)},`,
			`\t\t\tdark: ${swiftColor(colors[avatar].dark)}`,
			`\t\t)`
		].join('\n');
	});
	return [
		'// Generated by scripts/ios-avatars.ts from packages/core/src/avatars.ts and the avatar colors',
		"// in packages/web/src/routes/layout.css. Don't edit it: change those, and run the script.",
		'',
		'import SwiftUI',
		'',
		'extension AvatarGlyph {',
		'\tstatic let all: [String: AvatarGlyph] = [',
		glyphs.join(',\n'),
		'\t]',
		'}',
		''
	].join('\n');
}
