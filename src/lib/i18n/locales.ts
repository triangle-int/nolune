/** The languages the interface comes in. English is the one every other is checked against. */
export const LOCALES = ['en', 'ru', 'de', 'es', 'fr'] as const;

export type Locale = (typeof LOCALES)[number];

export const DEFAULT_LOCALE: Locale = 'en';

/** Each language in its own words, for the picker in Settings. */
export const LANGUAGE_NAMES: Record<Locale, string> = {
	en: 'English',
	ru: 'Русский',
	de: 'Deutsch',
	es: 'Español',
	fr: 'Français'
};

export function isLocale(value: unknown): value is Locale {
	return typeof value === 'string' && (LOCALES as readonly string[]).includes(value);
}

/**
 * The language to use for an `Accept-Language` header: the one the browser prefers most among
 * those btw has, by the language alone (`ru-RU` is Russian). English when there's none.
 */
export function matchLocale(header: string | null | undefined): Locale {
	const wanted = (header ?? '')
		.split(',')
		.map((part, index) => {
			const [tag, ...params] = part.trim().split(';');
			const q = params.map((p) => p.trim()).find((p) => p.startsWith('q='));
			return { tag: tag.trim().toLowerCase(), q: q ? Number(q.slice(2)) : 1, index };
		})
		.filter((w) => w.tag && w.q > 0)
		.sort((a, b) => b.q - a.q || a.index - b.index);
	for (const { tag } of wanted) {
		const language = tag.split('-')[0];
		if (isLocale(language)) return language;
	}
	return DEFAULT_LOCALE;
}
