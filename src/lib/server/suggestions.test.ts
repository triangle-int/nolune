import { describe, expect, it } from 'vitest';
import { translations } from '$lib/i18n';
import { withIcons } from './suggestions';

describe('withIcons', () => {
	const { m } = translations('ru');

	it('shows the general chips in the interface language', () => {
		const [chip] = withIcons(
			[{ id: 'reminder', icon: 'bell', label: 'Set a reminder', text: 'Remind me to ' }],
			m
		);
		expect(chip).toMatchObject(m.newChat.suggestions.reminder);
		expect(chip.icon.length).toBeGreaterThan(0);
	});

	it('keeps the ones made from memory as the model wrote them', () => {
		const made = { icon: 'dog', label: 'Walk Rex', text: 'Remind me to walk Rex at 18:00' };
		expect(withIcons([made], m)[0]).toMatchObject({ label: made.label, text: made.text });
	});
});
