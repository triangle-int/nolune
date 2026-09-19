import test from 'node:test';
import assert from 'node:assert/strict';
import { PRIMARY_TABS, activeTab, tabHref } from '../src/lib/companion/navigation.js';

test('primary navigation is exactly the five companion destinations (#98)', () => {
	assert.deepEqual(
		PRIMARY_TABS.map((t) => t.id),
		['chat', 'activity', 'memory', 'computers', 'settings'],
	);
	for (const tab of PRIMARY_TABS) {
		assert.ok(tab.label && tab.label[0] === tab.label[0].toUpperCase(), `${tab.id} needs a display label`);
		assert.ok(tab.description, `${tab.id} needs a one-line description`);
	}
	for (const retired of ['drops', 'skills', 'agents', 'thoughts', 'stats']) {
		assert.ok(!PRIMARY_TABS.some((t) => t.id === retired), `${retired} must not be a primary tab`);
	}
});

test('tab links live under the companion slug', () => {
	assert.equal(tabHref('companion', 'chat'), '/companion/chat');
	assert.equal(tabHref('companion', 'settings'), '/companion/settings');
});

test('the active tab follows nested routes and folds drops into activity', () => {
	assert.equal(activeTab('/companion', 'companion'), 'chat');
	assert.equal(activeTab('/companion/chat/thread-1', 'companion'), 'chat');
	assert.equal(activeTab('/companion/settings/advanced', 'companion'), 'settings');
	assert.equal(activeTab('/companion/computers', 'companion'), 'computers');
	assert.equal(activeTab('/companion/drops', 'companion'), 'activity', 'drops are an activity outcome, not a destination');
	assert.equal(activeTab('/companion/memory', 'companion'), 'memory');
	assert.equal(activeTab('/companion/settingsx', 'companion'), 'chat', 'prefix matches do not activate a tab');
});
