import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { COMPANION_KINDS } from '../src/lib/companion/state.js';
import { EXPRESSIONS, MOON_BODY, companionExpression, eyesMarkup, expressionAsset } from '../src/lib/companion/expressions.js';

const kinds = COMPANION_KINDS.map((k) => k.kind);
const moonDir = new URL('../static/skins/moon/', import.meta.url);

test('every companion state maps to one expression, one motion and one SVG that exists', () => {
	assert.deepEqual(EXPRESSIONS.map((e) => e.kind), kinds, 'the table lists every state once, in priority order');
	for (const kind of kinds) {
		const look = companionExpression(kind);
		assert.equal(look.kind, kind);
		assert.match(look.expression, /^[a-z]+$/);
		assert.match(look.motion, /^[a-z]+$/);
		assert.equal(look.asset, expressionAsset(look.expression));
		assert.match(look.asset, /^\/skins\/moon\/[a-z]+\.svg$/, `${kind} is drawn from a Little Moon SVG under client/static/skins/moon`);
		assert.ok(existsSync(new URL(look.asset.replace('/skins/moon/', ''), moonDir)), `${look.asset} exists`);
	}
	assert.equal(expressionAsset('idle'), '/skins/moon/character.svg', 'idle is the base character');
	assert.equal(expressionAsset('thinking'), '/skins/moon/thinking.svg', 'the existing thinking expression is kept');
	assert.ok(Object.isFrozen(EXPRESSIONS) && Object.isFrozen(companionExpression('idle')));
});

test('reduced motion keeps the expression and stops the motion', () => {
	for (const kind of kinds) {
		const moving = companionExpression(kind, { reducedMotion: false });
		const still = companionExpression(kind, { reducedMotion: true });
		assert.equal(still.motion, 'none', `${kind} is still under prefers-reduced-motion`);
		assert.equal(still.expression, moving.expression, `${kind} keeps its expression`);
		assert.equal(still.asset, moving.asset);
	}
	assert.equal(companionExpression('idle').motion, companionExpression('idle', {}).motion, 'motion is allowed unless reduced');
});

test('approval, permission, degraded and failure states never look like success or rest', () => {
	const success = new Set([companionExpression('completed').expression, companionExpression('idle').expression]);
	for (const kind of ['offline', 'blocked', 'waiting', 'failed']) {
		const look = companionExpression(kind);
		assert.ok(!success.has(look.expression), `${kind} (${look.expression}) is not drawn as completed or idle`);
	}
	assert.notEqual(companionExpression('blocked').expression, companionExpression('waiting').expression, 'a blocker and an open request are told apart');
	assert.notEqual(companionExpression('failed').expression, companionExpression('blocked').expression);
	assert.notEqual(companionExpression('offline').expression, companionExpression('failed').expression);
	// A companion that stopped, is blocked or has no connection does not move on its own.
	for (const kind of ['offline', 'blocked', 'failed']) {
		assert.equal(companionExpression(kind).motion, 'none', `${kind} holds still even when motion is allowed`);
	}
	// Work in progress is told apart from rest without relying on motion alone.
	assert.notEqual(companionExpression('working').expression, companionExpression('idle').expression);
	assert.equal(companionExpression('working_remote').expression, companionExpression('working').expression, 'where the work happens is in the status text, not the face');
	assert.equal(companionExpression('unknown').expression, companionExpression('idle').expression, 'an unknown kind falls back to idle');
});

test('the static SVGs draw the same face the desktop overlay draws inline', () => {
	const expressions = [...new Set(EXPRESSIONS.map((e) => e.expression))];
	assert.ok(expressions.length >= 8, `one face per state of note (${expressions.length})`);
	for (const expression of expressions) {
		const file = readFileSync(new URL(expressionAsset(expression).replace('/skins/moon/', ''), moonDir), 'utf8');
		assert.ok(file.includes(`d="${MOON_BODY}"`), `${expression}.svg uses the approved crescent path`);
		assert.ok(file.includes('fill="#B7A9E7"'), `${expression}.svg is lavender`);
		const eyes = eyesMarkup(expression, '#201D29');
		assert.ok(eyes.length > 0);
		assert.ok(file.includes(eyes), `${expression}.svg draws the eyes the shared table describes`);
		assert.ok(!file.includes('<animate'), `${expression}.svg is a still; motion belongs to the page`);
	}
	assert.equal(eyesMarkup('idle', 'var(--primary-foreground)').includes('var(--primary-foreground)'), true, 'the fill is the caller\'s so the desktop can use its token');
	assert.notEqual(eyesMarkup('idle', '#201D29'), eyesMarkup('thinking', '#201D29'));
});
