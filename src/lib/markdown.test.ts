import { Lexer, Parser, type Tokens } from 'marked';
import { describe, expect, it } from 'vitest';
import { pictureLines } from './markdown';

/** One paragraph through pictureLines and marked's own renderer. */
function render(markdown: string): string {
	const paragraph = Lexer.lex(markdown)[0] as Tokens.Paragraph;
	pictureLines(paragraph.tokens);
	return Parser.parseInline(paragraph.tokens);
}

describe('pictureLines', () => {
	it('puts a picture under the text before it', () => {
		expect(render('Here it is: ![cat](a.png)')).toBe('Here it is: <br><img src="a.png" alt="cat">');
	});

	it('puts the text after pictures under them', () => {
		expect(render('![cat](a.png)\nA cat.')).toBe('<img src="a.png" alt="cat"><br>\nA cat.');
	});

	it('keeps pictures next to each other side by side', () => {
		expect(render('Two:\n![a](a.png)\n![b](b.png)')).toBe(
			'Two:\n<br><img src="a.png" alt="a">\n<img src="b.png" alt="b">'
		);
	});

	it('leaves pictures on their own alone', () => {
		expect(render('![a](a.png) ![b](b.png)')).toBe(
			'<img src="a.png" alt="a"> <img src="b.png" alt="b">'
		);
	});

	it("doesn't add a break where the text already has one", () => {
		expect(render('Here:  \n![cat](a.png)')).toBe('Here:<br><img src="a.png" alt="cat">');
	});
});
