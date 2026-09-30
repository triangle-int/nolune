import { getContext, setContext } from 'svelte';
import { en, type Messages } from './messages/en';
import { de } from './messages/de';
import { es } from './messages/es';
import { fr } from './messages/fr';
import { ru } from './messages/ru';
import type { Locale } from './locales';

/*
 * The interface's words in each language. Only the interface: what people write, nolune's replies,
 * chat titles and everything that reaches the model stay as they are, whatever the language.
 *
 * Messages are plain strings, functions for the ones with values in them ("Worked for 12s"),
 * and, for sentences with markup in them (a link, a name in bold), strings with `{slots}` that
 * the Rich component fills. Every language has the same keys: TypeScript checks the shape against
 * English, and i18n.test.ts that the slots match.
 */

export * from './locales';
export type { Messages };

const MESSAGES: Record<Locale, Messages> = { en, ru, de, es, fr };

/** The interface's language and its messages. */
export interface I18n {
	locale: Locale;
	m: Messages;
	/** The locale for `Intl` and `toLocale…String`: British English, so dates read "28 Sept". */
	intl: string;
}

const cache = new Map<Locale, I18n>();

export function translations(locale: Locale): I18n {
	let found = cache.get(locale);
	if (!found) {
		found = { locale, m: MESSAGES[locale], intl: locale === 'en' ? 'en-GB' : locale };
		cache.set(locale, found);
	}
	return found;
}

const KEY = Symbol('i18n');

/** Set once by the root layout; changing the language reloads the page. */
export function setI18n(locale: Locale): I18n {
	return setContext(KEY, translations(locale));
}

export function getI18n(): I18n {
	return getContext<I18n>(KEY);
}
