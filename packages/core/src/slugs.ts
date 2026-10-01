/** Cyrillic letters in Latin, as Russian, Ukrainian and Belarusian names are spelled in addresses. */
const LATIN: Record<string, string> = {
	а: 'a',
	б: 'b',
	в: 'v',
	г: 'g',
	д: 'd',
	е: 'e',
	ё: 'yo',
	ж: 'zh',
	з: 'z',
	и: 'i',
	й: 'y',
	к: 'k',
	л: 'l',
	м: 'm',
	н: 'n',
	о: 'o',
	п: 'p',
	р: 'r',
	с: 's',
	т: 't',
	у: 'u',
	ф: 'f',
	х: 'kh',
	ц: 'ts',
	ч: 'ch',
	ш: 'sh',
	щ: 'shch',
	ъ: '',
	ы: 'y',
	ь: '',
	э: 'e',
	ю: 'yu',
	я: 'ya',
	є: 'ye',
	і: 'i',
	ї: 'yi',
	ґ: 'g',
	ў: 'u'
};

/**
 * A name as the start of a slug: lower case, Cyrillic spelled in Latin, accents dropped, its words
 * joined by `-` (`Zoë & Max!` → `zoe-max`, `Книжный клуб` → `knizhnyy-klub`), at most 40
 * characters. Empty when no letter or digit is left, like for `🏡`.
 */
export function slugBase(name: string): string {
	return name
		.normalize('NFC')
		.toLowerCase()
		.replace(/[Ѐ-ӿ]/g, (letter) => LATIN[letter] ?? letter)
		.normalize('NFKD')
		.replace(/\p{M}/gu, '')
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 40)
		.replace(/-+$/, '');
}
