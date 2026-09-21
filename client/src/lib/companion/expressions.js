// @ts-check
/**
 * Little Moon's expressions (#86): a pure table from a companion state to
 * the face it shows and the motion it makes, with the reduced-motion
 * fallback (the same face, held still). The table is shared by the client
 * (`MoonExpression.svelte`, the static SVGs under `client/static/skins/moon`),
 * the client overlay route, the desktop overlay (`desktop/src/lib/
 * components/Moon.svelte` draws `eyesMarkup` inline) and the `/design-system`
 * gallery, so every copy of the moon makes the same face for the same state.
 *
 * Every state has a face of its own except that work reads the same wherever
 * it happens (the status text names the computer). Approval, permission,
 * degraded and failure states never borrow the completed or idle face, and a
 * companion that is offline, blocked or stopped with an error does not move
 * on its own, so motion can never suggest progress the runtime has not made.
 */

/** @typedef {import("./state.js").CompanionKind} CompanionKind */
/** @typedef {"idle" | "listening" | "thinking" | "recalling" | "working" | "waiting" | "blocked" | "failed" | "offline" | "completed"} Expression */
/**
 * Continuous or one-shot motion of the whole moon; `none` holds still.
 * breathe: slow scale, at rest. float: a gentle bob, thinking. drift: bob
 * with a slight tilt, recalling. nod: a quick, small nod, at work. hold: a
 * soft pulse, waiting on the user. settle: one bounce, then still.
 * @typedef {"none" | "breathe" | "float" | "drift" | "nod" | "hold" | "settle"} Motion
 */
/** @typedef {{ kind: CompanionKind; expression: Expression; motion: Motion; file: string }} ExpressionRow */

/** The approved lavender crescent (client/static/skins/moon/character.svg). */
export const MOON_BODY = "M155 24C161 23 164 29 160 34C127 74 130 129 157 168C183 207 228 224 277 210C284 208 289 214 285 221C262 266 217 292 170 290C93 287 36 230 36 157C36 91 85 34 155 24Z";

/** State → face, motion and the still SVG under `static/skins/moon`, in the reducer's priority order. */
export const EXPRESSIONS = Object.freeze(
	/** @type {readonly ExpressionRow[]} */ ([
		{ kind: "offline", expression: "offline", motion: "none", file: "offline.svg" },
		{ kind: "blocked", expression: "blocked", motion: "none", file: "blocked.svg" },
		{ kind: "waiting", expression: "waiting", motion: "hold", file: "waiting.svg" },
		{ kind: "failed", expression: "failed", motion: "none", file: "failed.svg" },
		{ kind: "working_remote", expression: "working", motion: "nod", file: "working.svg" },
		{ kind: "working", expression: "working", motion: "nod", file: "working.svg" },
		{ kind: "recalling", expression: "recalling", motion: "drift", file: "recalling.svg" },
		{ kind: "thinking", expression: "thinking", motion: "float", file: "thinking.svg" },
		{ kind: "listening", expression: "listening", motion: "breathe", file: "listening.svg" },
		{ kind: "completed", expression: "completed", motion: "settle", file: "completed.svg" },
		{ kind: "idle", expression: "idle", motion: "breathe", file: "character.svg" },
	]).map((row) => Object.freeze(row)),
);

/**
 * The face and motion for a state. Under `prefers-reduced-motion` the face
 * stays and the motion is `none`; the status text carries the meaning.
 * @param {string} kind
 * @param {{ reducedMotion?: boolean }} [options]
 * @returns {Readonly<{ kind: CompanionKind; expression: Expression; motion: Motion; asset: string }>}
 */
export function companionExpression(kind, { reducedMotion = false } = {}) {
	const row = EXPRESSIONS.find((r) => r.kind === kind) ?? /** @type {ExpressionRow} */ (EXPRESSIONS[EXPRESSIONS.length - 1]);
	return Object.freeze({
		kind: row.kind,
		expression: row.expression,
		motion: reducedMotion ? "none" : row.motion,
		asset: expressionAsset(row.expression),
	});
}

/**
 * The static SVG for a face. `idle` is the base character; the others sit
 * beside it under the expression's name.
 * @param {string} expression
 */
export function expressionAsset(expression) {
	const row = EXPRESSIONS.find((r) => r.expression === expression) ?? /** @type {ExpressionRow} */ (EXPRESSIONS[EXPRESSIONS.length - 1]);
	return `/skins/moon/${row.file}`;
}

/**
 * The eyes of each face on the 320×320 moon: the only thing that changes
 * between expressions. Open eyes are two ellipses; closed or happy eyes are
 * two short strokes. `fill` is the caller's plum, so the static files carry
 * the literal token value and the desktop draws with its CSS variable.
 * @param {string} expression
 * @param {string} fill
 */
export function eyesMarkup(expression, fill) {
	/** @param {number} cy @param {number} rx @param {number} ry @param {number} [dx] */
	const eyes = (cy, rx, ry, dx = 0) => `<ellipse cx="${81 + dx}" cy="${cy}" rx="${rx}" ry="${ry}" fill="${fill}"/><ellipse cx="${110 + dx}" cy="${cy}" rx="${rx}" ry="${ry}" fill="${fill}"/>`;
	/** @param {string} left @param {string} right */
	const strokes = (left, right) => `<path d="${left}" stroke="${fill}" stroke-width="4" stroke-linecap="round" fill="none"/><path d="${right}" stroke="${fill}" stroke-width="4" stroke-linecap="round" fill="none"/>`;
	switch (expression) {
		case "thinking":
			return eyes(157, 7, 7);
		case "listening":
			return eyes(160, 7, 11);
		case "recalling":
			return eyes(154, 7, 7, -4);
		case "working":
			return eyes(166, 6, 5);
		case "waiting":
			return eyes(156, 6, 9, 4);
		case "blocked":
			return `<rect x="74" y="162" width="14" height="4" rx="2" fill="${fill}"/><rect x="103" y="162" width="14" height="4" rx="2" fill="${fill}"/>`;
		case "failed":
			return eyes(172, 5, 6);
		case "offline":
			return strokes("M74 164c3 5 11 5 14 0", "M103 164c3 5 11 5 14 0");
		case "completed":
			return strokes("M74 167c3-6 11-6 14 0", "M103 167c3-6 11-6 14 0");
		default:
			return eyes(164, 6, 9);
	}
}
