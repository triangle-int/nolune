import { describe, expect, it } from 'vitest';
import { isLocalHref, mediaAsText, mediaRefs } from './media-refs.ts';

describe('isLocalHref', () => {
	it.each(['file:///a.png', '/Users/anna/a.png', '~/a.png', 'photos/a.png'])(
		'%s is local',
		(href) => {
			expect(isLocalHref(href)).toBe(true);
		}
	);

	it.each([
		'',
		'https://example.com',
		'mailto:anna@example.com',
		'#top',
		'?q=1',
		'//cdn.example.com'
	])('"%s" is not local', (href) => {
		expect(isLocalHref(href)).toBe(false);
	});
});

describe('mediaRefs', () => {
	it('finds local files and remote pictures once each, outside code', () => {
		const markdown = [
			'![a](/tmp/a.png) [site](https://example.com) ![web](https://example.com/p.jpg)',
			'[form](~/form.pdf) `![c](/c.png)` [again](/tmp/a.png) [top](#top)',
			'',
			'```',
			'![b](/b.png)',
			'```'
		].join('\n');
		expect(mediaRefs(markdown)).toEqual(['/tmp/a.png', 'https://example.com/p.jpg', '~/form.pdf']);
	});
});

describe('mediaAsText', () => {
	it('replaces pictures with their description and file links with their label', () => {
		expect(
			mediaAsText(
				'Look ![beach](/p/b.jpg) and ![](/p/c.jpg), fill in [the form](~/f.pdf) from [the site](https://example.com)'
			)
		).toBe('Look [beach] and [picture], fill in the form from [the site](https://example.com)');
	});
});
