import { describe, expect, it } from 'vitest';
import { parseCron } from '@nolune/core/schedule';
import { LOCALES, matchLocale, translations, type Locale, type Messages } from '.';

describe('matchLocale', () => {
	it.each([
		['ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7', 'ru'],
		['fr-CA', 'fr'],
		['ja,de;q=0.5,en;q=0.3', 'de'],
		['en-US,ru;q=0.9', 'en'],
		['es;q=0.5, de;q=0.8', 'de'],
		['es;q=0', 'en'],
		['*', 'en'],
		['', 'en'],
		[null, 'en']
	])('%s is %s', (header, locale) => {
		expect(matchLocale(header)).toBe(locale);
	});
});

/** Every string in a language's messages by its path, like `chat.deleteBody`. */
function strings(value: unknown, path = ''): Record<string, string> {
	if (typeof value === 'string') return { [path]: value };
	if (typeof value !== 'object' || value === null) return {};
	return Object.fromEntries(
		Object.entries(value).flatMap(([key, child]) =>
			Object.entries(strings(child, path ? `${path}.${key}` : key))
		)
	);
}

const slots = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

describe('messages', () => {
	const english = strings(translations('en').m);

	it.each(LOCALES.filter((l) => l !== 'en'))('%s fills every slot English has', (locale) => {
		const translated = strings(translations(locale).m);
		for (const [path, text] of Object.entries(english)) {
			// Lists of examples may have another length; everything else is there.
			if (/\.\d+$/.test(path)) continue;
			expect(translated[path], path).toBeDefined();
			expect(slots(translated[path]), path).toEqual(slots(text));
		}
	});

	it.each(LOCALES)('%s has no empty messages', (locale) => {
		for (const [path, text] of Object.entries(strings(translations(locale).m))) {
			expect(text.trim(), path).not.toBe('');
		}
	});

	it('counts in each language’s own plural forms', () => {
		const { m: ru } = translations('ru');
		expect([1, 3, 5, 21].map(ru.steps.ranCommands)).toEqual([
			'Выполнил 1 команду',
			'Выполнил 3 команды',
			'Выполнил 5 команд',
			'Выполнил 21 команду'
		]);
		expect(translations('en').m.steps.ranCommands(1)).toBe('Ran 1 command');
		expect(translations('de').m.memory.memories(2)).toBe('2 Erinnerungen');
	});
});

describe('schedules', () => {
	const describeIn = (m: Messages, cron: string) => {
		const schedule = parseCron(cron);
		if (!schedule) throw new Error(`can't read ${cron}`);
		return m.automations.describe(schedule);
	};

	const CRONS = [
		'30 7 * * 1-5',
		'*/10 9-17 * * *',
		'0 */2 * * *',
		'15 * * * *',
		'0 9 * * 6,0',
		'0 9 * * 1-4',
		'0 9,18 * * 1,3,5',
		'0 8 1 * *',
		'0 9 25 12 *',
		'0 9 * 6-8 *'
	];

	const EXPECTED: Record<Exclude<Locale, 'en'>, string[]> = {
		ru: [
			'По будням в 07:30',
			'Каждые 10 минут с 09:00 до 17:50',
			'Каждые 2 часа',
			'Каждый час в :15',
			'По выходным в 09:00',
			'С понедельника по четверг в 09:00',
			'По понедельникам, средам и пятницам в 09:00 и 18:00',
			'1 числа каждого месяца в 08:00',
			'Каждый год 25 декабря в 09:00',
			'Каждый день с июня по август в 09:00'
		],
		de: [
			'Montags bis freitags um 07:30 Uhr',
			'Alle 10 Minuten von 09:00 bis 17:50',
			'Alle 2 Stunden',
			'Stündlich um :15',
			'Am Wochenende um 09:00 Uhr',
			'Montag bis Donnerstag um 09:00 Uhr',
			'Montags, mittwochs und freitags um 09:00 und 18:00 Uhr',
			'Am 1. jedes Monats um 08:00 Uhr',
			'Jedes Jahr am 25. Dezember um 09:00 Uhr',
			'Täglich von Juni bis August um 09:00 Uhr'
		],
		es: [
			'De lunes a viernes a las 07:30',
			'Cada 10 minutos de 09:00 a 17:50',
			'Cada 2 horas',
			'Cada hora en el minuto 15',
			'Los fines de semana a las 09:00',
			'De lunes a jueves a las 09:00',
			'Los lunes, miércoles y viernes a las 09:00 y 18:00',
			'El día 1 de cada mes a las 08:00',
			'Cada año el 25 de diciembre a las 09:00',
			'Todos los días de junio a agosto a las 09:00'
		],
		fr: [
			'Du lundi au vendredi à 07:30',
			'Toutes les 10 minutes de 09:00 à 17:50',
			'Toutes les 2 heures',
			'Toutes les heures à la minute 15',
			'Le week-end à 09:00',
			'Du lundi au jeudi à 09:00',
			'Les lundis, mercredis et vendredis à 09:00 et 18:00',
			'Le 1er de chaque mois à 08:00',
			'Chaque année le 25 décembre à 09:00',
			'Tous les jours de juin à août à 09:00'
		]
	};

	it.each(Object.entries(EXPECTED))('says them in %s', (locale, expected) => {
		const { m } = translations(locale as Locale);
		expect(CRONS.map((cron) => describeIn(m, cron))).toEqual(expected);
	});

	it('keeps weekdays after the times, in each language', () => {
		const cron = '*/15 * * * 1-5';
		expect(describeIn(translations('en').m, cron)).toBe('Every 15 minutes on weekdays');
		expect(describeIn(translations('ru').m, cron)).toBe('Каждые 15 минут по будням');
		expect(describeIn(translations('de').m, cron)).toBe('Alle 15 Minuten montags bis freitags');
	});

	it('elides "de" before a vowel in French', () => {
		expect(describeIn(translations('fr').m, '0 9 * 4-6 *')).toBe(
			'Tous les jours d’avril à juin à 09:00'
		);
	});
});
