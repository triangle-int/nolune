/*
 * "a" or "an" before a choice, for template sentences like "Make a {{style}} sticker pack" whose
 * word is only known once someone picks or types it. By spelling, with the usual exceptions ("a
 * unicorn", "a '90s prom", "an 8-bit", "an hour"): right for labels, not a full rule of English.
 * No imports, so the Images page can use it too.
 */

export function indefiniteArticle(word: string): 'a' | 'an' {
	const w = word
		.trim()
		.replace(/^['"‘’“(]+/, '')
		.toLowerCase();
	if (/^(?:8|11(?!\d)|18(?!\d))/.test(w)) return 'an';
	if (/^(?:one|once|eu|uni|use|usu|uti|ura|ure|uri|uto|uku)/.test(w)) return 'a';
	if (/^(?:hour|honest|honou?r|heir)/.test(w)) return 'an';
	return /^[aeiou]/.test(w) ? 'an' : 'a';
}

/** "a " at the end of `text` as "an " when `next` wants it; any other text as it is. */
export function articleBefore(text: string, next: string): string {
	if (!next || !/(?:^|\s)[aA] $/.test(text) || indefiniteArticle(next) === 'a') return text;
	return text.replace(/([aA]) $/, '$1n ');
}
