import { describe, expect, it } from 'vitest';
import { slugBase } from './slugs.ts';

describe('slugBase', () => {
	it('joins the words of a name with dashes, without accents', () => {
		expect(slugBase('  Zoë & Max! ')).toBe('zoe-max');
		expect(slugBase('Trip to Japan 2027')).toBe('trip-to-japan-2027');
	});

	it('spells Cyrillic in Latin', () => {
		expect(slugBase('Семья')).toBe('semya');
		expect(slugBase('Книжный клуб')).toBe('knizhnyy-klub');
		expect(slugBase('Ёжик и Щука')).toBe('yozhik-i-shchuka');
		expect(slugBase('Мама, папа и Юля')).toBe('mama-papa-i-yulya');
		expect(slugBase('Сім’я Ґонти')).toBe('sim-ya-gonti');
		expect(slugBase('Дача 2026')).toBe('dacha-2026');
	});

	it('reads й and ё the same when they come as a letter and a mark', () => {
		expect(slugBase('Андрей'.normalize('NFD'))).toBe('andrey');
		expect(slugBase('Алёна'.normalize('NFD'))).toBe('alyona');
	});

	it('keeps at most 40 characters, without a dash at the end', () => {
		const slug = slugBase('Очень длинное название для профиля нашей большой семьи');
		expect(slug.length).toBeLessThanOrEqual(40);
		expect(slug).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
	});

	it('is empty when no letter or digit is left', () => {
		expect(slugBase('🏡')).toBe('');
		expect(slugBase(' — ')).toBe('');
	});
});
