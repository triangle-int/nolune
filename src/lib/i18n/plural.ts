export type PluralForms = Partial<Record<Intl.LDMLPluralRule, string>> & { other: string };

/**
 * The form of a phrase for a count, by the language's own rules: English has one and other,
 * Russian one, few, many and other. A missing form falls back to `other`.
 */
export function plural(locale: string): (n: number, forms: PluralForms) => string {
	const rules = new Intl.PluralRules(locale);
	return (n, forms) => forms[rules.select(n)] ?? forms.other;
}

/** "a, b and c" in the language's own words. */
export function listOf(locale: string): (items: string[]) => string {
	const format = new Intl.ListFormat(locale, { type: 'conjunction' });
	return (items) => format.format(items);
}
