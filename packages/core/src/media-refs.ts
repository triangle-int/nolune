import { Marked, type Token } from 'marked';

/**
 * Which links in a reply are pictures or files to copy. Shared by the gateway, which copies
 * them, and the browser, which shows the copies, so both must read the Markdown the same way.
 * No Node imports: this file is bundled into the web app.
 */

const lexer = new Marked({ gfm: true });

const SCHEME = /^[a-z][a-z0-9+.-]*:/i;

export function isRemoteHref(href: string): boolean {
	return /^https?:\/\//i.test(href);
}

/** A path on this computer: absolute, `~/…`, relative to the profile folder, or a file: URL. */
export function isLocalHref(href: string): boolean {
	if (/^file:/i.test(href)) return true;
	if (!href.trim() || SCHEME.test(href)) return false;
	return !href.startsWith('#') && !href.startsWith('?') && !href.startsWith('//');
}

/**
 * Whether a Markdown link target gets copied: pictures from this computer or the web, and files
 * from this computer. Links to web pages stay ordinary links.
 */
export function isMediaHref(href: string, kind: 'image' | 'link'): boolean {
	return isLocalHref(href) || (kind === 'image' && isRemoteHref(href));
}

/** The link targets to copy, in order, each once. Code spans and code blocks are skipped. */
export function mediaRefs(markdown: string): string[] {
	const found = new Set<string>();
	lexer.walkTokens(lexer.lexer(markdown), (token: Token) => {
		if ((token.type === 'image' || token.type === 'link') && isMediaHref(token.href, token.type)) {
			found.add(token.href);
		}
	});
	return [...found];
}

/**
 * The text with pictures replaced by their description and file links by their label, for
 * places that show a reply as plain text, like the notification menu.
 */
export function mediaAsText(markdown: string): string {
	const replacements: [string, string][] = [];
	lexer.walkTokens(lexer.lexer(markdown), (token: Token) => {
		if (token.type === 'image' && isMediaHref(token.href, 'image')) {
			replacements.push([token.raw, `[${token.text.trim() || 'picture'}]`]);
		} else if (token.type === 'link' && isMediaHref(token.href, 'link')) {
			replacements.push([token.raw, token.text]);
		}
	});
	let text = markdown;
	for (const [raw, replacement] of replacements) text = text.replace(raw, replacement);
	return text;
}
