import test from 'node:test';
import assert from 'node:assert/strict';
import {
	SETTINGS_SECTIONS,
	SETTINGS,
	defaultSection,
	ownerLabel,
	sectionForPath,
	sectionHref,
	settingsIn,
} from '../src/lib/settings/sections.js';

test('settings has five sections in a fixed order (#98)', () => {
	assert.deepEqual(
		SETTINGS_SECTIONS.map((s) => s.id),
		['companion', 'connections', 'capabilities', 'data', 'advanced'],
	);
	for (const section of SETTINGS_SECTIONS) {
		assert.ok(section.label && section.description, `${section.id} needs a label and description`);
	}
	assert.equal(defaultSection(), 'companion');
	assert.equal(sectionHref('companion', 'data'), '/companion/settings/data');
});

test('every retained setting has exactly one section and one owner', () => {
	const keys = SETTINGS.map((s) => s.key);
	assert.equal(new Set(keys).size, keys.length, 'setting keys are unique');
	const sectionIds = new Set(SETTINGS_SECTIONS.map((s) => s.id));
	for (const setting of SETTINGS) {
		assert.ok(sectionIds.has(setting.section), `${setting.key} points at unknown section ${setting.section}`);
		assert.ok(['server', 'companion'].includes(setting.scope), `${setting.key} needs a scope`);
		assert.ok(setting.label, `${setting.key} needs a label`);
	}
	for (const section of SETTINGS_SECTIONS) {
		assert.ok(settingsIn(section.id).length > 0, `${section.id} owns at least one setting`);
	}
});

test('raw server and protocol fields are owned by Advanced only', () => {
	const raw = SETTINGS.filter((s) => s.raw);
	assert.ok(raw.length >= 5, 'server, updates, voice id, email, github are raw');
	for (const setting of raw) {
		assert.equal(setting.section, 'advanced', `${setting.key} is raw and must live under Advanced`);
	}
	const byKey = Object.fromEntries(SETTINGS.map((s) => [s.key, s]));
	assert.equal(byKey['server.port'].scope, 'server');
	assert.equal(byKey['server.port'].section, 'advanced');
	assert.equal(byKey['initiative'].scope, 'companion');
	assert.equal(byKey['initiative'].section, 'companion');
	assert.equal(byKey['timezone'].section, 'companion');
	assert.equal(byKey['models'].section, 'connections');
	assert.equal(byKey['models'].scope, 'server');
	assert.equal(byKey['provider'], undefined, 'the global provider switch is gone (#156)');
	assert.equal(byKey['model-mode'], undefined, 'model routing is gone (#156)');
	assert.equal(byKey['computers'].section, 'connections');
	assert.equal(byKey['paired-devices'].section, 'connections');
	assert.equal(byKey['codex'].section, 'connections', 'the Codex login lives beside the API keys (#27)');
	assert.equal(byKey['codex'].scope, 'server');
	assert.equal(byKey['skills'].section, 'capabilities');
	assert.equal(byKey['extensions'].section, 'capabilities');
	assert.equal(byKey['export'].section, 'data');
	assert.equal(byKey['export'].scope, 'companion');
	assert.equal(byKey['email'].section, 'advanced');
	assert.equal(byKey['github'].section, 'advanced');
	assert.equal(byKey['voice-id'].section, 'advanced');
});

test('the current section is read from the route and owners have plain names', () => {
	assert.equal(sectionForPath('/companion/settings/data'), 'data');
	assert.equal(sectionForPath('/companion/settings/advanced/'), 'advanced');
	assert.equal(sectionForPath('/companion/settings'), 'companion');
	assert.equal(sectionForPath('/companion/settings/nope'), 'companion');
	assert.equal(ownerLabel('server'), 'This server');
	assert.equal(ownerLabel('companion'), 'Your companion');
});
