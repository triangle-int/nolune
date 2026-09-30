import { describe, expect, it } from 'vitest';
import { LOCALES, translations } from '$lib/i18n';
import { dayPart, greeting } from './greeting';

describe('dayPart', () => {
	it.each([
		[0, 'night'],
		[4, 'night'],
		[5, 'morning'],
		[11, 'morning'],
		[12, 'afternoon'],
		[17, 'afternoon'],
		[18, 'evening'],
		[22, 'evening'],
		[23, 'night']
	])('%i:00 is %s', (hour, part) => {
		expect(dayPart(hour)).toBe(part);
	});
});

describe('greeting', () => {
	const { m } = translations('en');
	const { greetings } = m.newChat;
	/** Every greeting a seed can pick at this hour. */
	const all = (hour: number, name: string) =>
		new Set(Array.from({ length: 100 }, (_, i) => greeting(greetings, name, hour, i / 100)));

	it('picks from the part of the day and from any time', () => {
		const morning = all(8, 'Anna');
		expect(morning).toContain('Good morning, Anna!');
		expect(morning).toContain('What can I help with, Anna?');
		expect(morning).not.toContain('Good evening, Anna!');
		expect(all(20, 'Anna')).toContain('Good evening, Anna!');
	});

	it('stays in the list at the ends of the seed', () => {
		expect(greeting(greetings, 'Anna', 8, 0)).toBe('Good morning, Anna!');
		expect(greeting(greetings, 'Anna', 8, 1)).toBe(greetings.anytime.at(-1)!('Anna'));
	});

	it.each(LOCALES)('reads well without a name in %s', (locale) => {
		const localized = translations(locale).m.newChat.greetings;
		for (const phrase of Object.values(localized).flat()) {
			const text = phrase('');
			expect(text.trim(), text).not.toBe('');
			expect(text, text).not.toMatch(/,\s*[.!?]|,\s*$|\s{2}/);
			expect(phrase('Anna'), text).toContain('Anna');
		}
	});
});
