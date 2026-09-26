import { browser } from '$app/environment';
import type { DisplayMedia } from '@btw/core';
import { isMediaHref } from '@btw/core/media-refs';
import DOMPurify from 'dompurify';
import { Copy, Download, File, FileX, Image, ImageOff, type IconNode } from 'lucide';
import { Marked, type Token, type Tokens } from 'marked';
import { copyText } from './clipboard';
import { formatBytes } from './format';

/** A Lucide icon as markup, for the buttons and cards inside rendered HTML. */
function iconSvg(node: IconNode, size: number, className?: string): string {
	return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"${className ? ` class="${className}"` : ''}>${node
		.map(
			([tag, attrs]) =>
				`<${tag} ${Object.entries(attrs)
					.map(([name, value]) => `${name}="${value}"`)
					.join(' ')}/>`
		)
		.join('')}</svg>`;
}

const COPY_ICON = iconSvg(Copy, 14);
const FILE_ICON = iconSvg(File, 20, 'shrink-0 text-muted-foreground');
const DOWNLOAD_ICON = iconSvg(Download, 16, 'shrink-0 text-muted-foreground');
const IMAGE_ICON = iconSvg(Image, 20, 'shrink-0');
const IMAGE_OFF_ICON = iconSvg(ImageOff, 16, 'shrink-0 text-muted-foreground');
const FILE_X_ICON = iconSvg(FileX, 16, 'shrink-0 text-muted-foreground');

function escapeHtml(text: string): string {
	return text
		.replaceAll('&', '&amp;')
		.replaceAll('<', '&lt;')
		.replaceAll('>', '&gt;')
		.replaceAll('"', '&quot;');
}

/** Where a reply's pictures and files come from. */
export interface MediaContext {
	conversationId: string;
	/** The copies made when the reply was saved, keyed by link target. */
	media?: Record<string, DisplayMedia>;
	/** The reply is still being written, so nothing has been copied yet. */
	pending?: boolean;
}

type CopiedMedia = Extract<DisplayMedia, { status: 'ok' }>;

/** Set only while renderMarkdown parses, which is synchronous. */
let context: MediaContext | undefined;

/** Pictures are at most this tall in the chat; the viewer shows them full size. */
const MAX_PICTURE_HEIGHT = 448;

function mediaUrl(id: string, download = false): string {
	const conversation = encodeURIComponent(context?.conversationId ?? '');
	return `/api/c/${conversation}/media/${encodeURIComponent(id)}${download ? '?download' : ''}`;
}

function picture(media: CopiedMedia, alt: string): string {
	// The size holds the picture's space before it loads, so the chat doesn't jump.
	let size = '';
	if (media.width && media.height) {
		const scale = Math.min(1, MAX_PICTURE_HEIGHT / media.height);
		size = ` width="${Math.round(media.width * scale)}" height="${Math.round(media.height * scale)}"`;
	}
	return `<button type="button" data-media-view data-name="${escapeHtml(media.name)}" data-download="${mediaUrl(media.id, true)}" aria-label="${escapeHtml(`Open ${alt || media.name}`)}" class="my-1 mr-1.5 inline-block max-w-full cursor-zoom-in overflow-hidden rounded-xl align-top"><img src="${mediaUrl(media.id)}" alt="${escapeHtml(alt)}"${size} loading="lazy" decoding="async" class="block h-auto max-h-[28rem] max-w-full bg-muted"></button>`;
}

/**
 * File cards go on a line of their own, under the text that introduces them ("It's here:"),
 * rather than in the middle of the sentence. Pictures stay inline so they can sit side by side.
 */
const OWN_LINE = 'my-2 flex w-fit';
const INLINE = 'my-1 mr-1.5 inline-flex align-middle';

/** `label` is HTML. */
function fileCard(media: CopiedMedia, label: string): string {
	const name = escapeHtml(media.name);
	const details =
		label === name ? formatBytes(media.bytes) : `${name} · ${formatBytes(media.bytes)}`;
	return `<a href="${mediaUrl(media.id, true)}" download="${name}" data-media-file class="${OWN_LINE} max-w-full items-center gap-3 rounded-xl border px-3 py-2 leading-snug hover:bg-muted">${FILE_ICON}<span class="min-w-0"><span class="block truncate font-medium">${label}</span><span class="block truncate text-xs text-muted-foreground">${details}</span></span>${DOWNLOAD_ICON}</a>`;
}

/** Something the reply links to that couldn't be copied. `label` is HTML. */
function problem(icon: string, label: string, reason: string, layout: string): string {
	return `<span data-media-problem class="${layout} max-w-full items-center gap-2 rounded-xl border border-dashed px-3 py-2 text-sm leading-snug">${icon}<span class="min-w-0"><span class="font-medium">${label}</span> <span class="text-muted-foreground">· ${escapeHtml(reason)}</span></span></span>`;
}

function pendingPicture(alt: string): string {
	return `<span data-media-pending class="my-1 mr-1.5 inline-flex h-40 w-60 max-w-full animate-pulse flex-col items-center justify-center gap-2 rounded-xl bg-muted p-3 text-center align-top text-sm text-muted-foreground">${IMAGE_ICON}<span class="line-clamp-2">${escapeHtml(alt)}</span></span>`;
}

function pendingFile(label: string): string {
	return `<span data-media-pending class="${OWN_LINE} max-w-full animate-pulse items-center gap-3 rounded-xl border px-3 py-2 leading-snug">${FILE_ICON}<span class="min-w-0 truncate font-medium">${label}</span></span>`;
}

/**
 * Pictures written into a sentence ("Here it is: ![photo](…)") go on a line of their own, under
 * the text before them and above the text after them, like file cards do. Pictures with only
 * spaces between them stay side by side. Changes `tokens`, a list of inline tokens, in place.
 */
export function pictureLines(tokens: Token[]): void {
	const blank = (token: Token | undefined) => token?.type === 'text' && !token.raw.trim();
	const needsBreak = (token: Token | undefined) => token !== undefined && token.type !== 'br';
	const breaks: number[] = [];
	for (let start = 0; start < tokens.length; start++) {
		if (tokens[start].type !== 'image') continue;
		let end = start;
		for (let next = start + 1; next < tokens.length; next++) {
			if (tokens[next].type === 'image') end = next;
			else if (!blank(tokens[next])) break;
		}
		let before = start - 1;
		while (blank(tokens[before])) before--;
		let after = end + 1;
		while (blank(tokens[after])) after++;
		if (needsBreak(tokens[before])) breaks.push(start);
		if (needsBreak(tokens[after])) breaks.push(end + 1);
		start = end;
	}
	for (const at of breaks.reverse()) tokens.splice(at, 0, { type: 'br', raw: '' });
}

const marked = new Marked({
	gfm: true,
	walkTokens(token) {
		const inline = (token as Tokens.Generic).tokens;
		if (inline) pictureLines(inline);
	},
	renderer: {
		// Code blocks get a header with the language and a copy button, like ChatGPT.
		code({ text, lang }) {
			const language = escapeHtml((lang ?? '').split(/\s/)[0]);
			return `<div data-code-block class="my-3 overflow-hidden rounded-2xl border bg-muted/50">
<div class="flex h-9 items-center justify-between pr-2 pl-4 text-xs text-muted-foreground"><span>${language || 'text'}</span><button type="button" data-copy class="flex items-center gap-1.5 rounded-lg px-2 py-1 hover:bg-accent hover:text-foreground">${COPY_ICON}<span>Copy</span></button></div>
<pre class="overflow-x-auto px-4 pb-4"><code>${escapeHtml(text)}</code></pre></div>`;
		},
		// Pictures show btw's copy of the file (see @btw/core media.ts), never the link itself.
		image({ href, text, tokens }) {
			const alt = tokens ? this.parser.parseInline(tokens, this.parser.textRenderer) : text;
			const media = context?.media?.[href];
			if (media?.status === 'ok') {
				return media.viewable
					? picture(media, alt)
					: fileCard(media, escapeHtml(alt || media.name));
			}
			if (media) {
				return problem(IMAGE_OFF_ICON, escapeHtml(alt || media.name), media.error, INLINE);
			}
			if (context?.pending && isMediaHref(href, 'image')) return pendingPicture(alt);
			return problem(IMAGE_OFF_ICON, escapeHtml(alt || 'Picture'), 'Not available', INLINE);
		},
		// Links to files on the computer become downloads of the copy; web links stay links.
		link(token) {
			if (!context || !isMediaHref(token.href, 'link')) return false;
			const label = this.parser.parseInline(token.tokens);
			const media = context.media?.[token.href];
			if (media?.status === 'ok') return fileCard(media, label);
			if (media) return problem(FILE_X_ICON, label, media.error, OWN_LINE);
			// Written before files were copied: the path means nothing to the browser.
			return context.pending ? pendingFile(label) : label;
		}
	}
});

const SANITIZE = {
	ADD_ATTR: ['target'],
	// Nothing in a reply may load from another site without a click, since the URL could carry
	// what the reply knows (prompt injection): no stylesheets or inline styles, which can load
	// url(), no audio or video, no alternative image sources, no SVG images.
	FORBID_TAGS: ['style', 'video', 'audio', 'source', 'track', 'picture', 'image', 'feImage'],
	FORBID_ATTR: ['style', 'srcset', 'poster', 'background']
};

let hooked = false;

/**
 * Markdown to sanitized HTML. Only used in the browser; the server never renders messages.
 * With `media`, pictures and file links show the copies made for the reply.
 */
export function renderMarkdown(text: string, media?: MediaContext): string {
	if (!browser) return `<p>${escapeHtml(text)}</p>`;
	if (!hooked) {
		DOMPurify.addHook('afterSanitizeAttributes', (node) => {
			if (node.tagName === 'A' && node.getAttribute('href') && !node.hasAttribute('download')) {
				node.setAttribute('target', '_blank');
				node.setAttribute('rel', 'noopener noreferrer');
			}
			// Pictures only ever come from btw's own copies (see SANITIZE).
			if (node.tagName === 'IMG' && !node.getAttribute('src')?.startsWith('/api/c/')) {
				node.removeAttribute('src');
			}
		});
		hooked = true;
	}
	context = media;
	try {
		return DOMPurify.sanitize(marked.parse(text, { async: false }), SANITIZE);
	} finally {
		context = undefined;
	}
}

/** Handles the copy buttons inside rendered code blocks. */
export function codeCopyButtons(node: HTMLElement) {
	const onClick = async (event: MouseEvent) => {
		const button = (event.target as Element).closest<HTMLButtonElement>('[data-copy]');
		if (!button) return;
		const code = button.closest('[data-code-block]')?.querySelector('code')?.textContent ?? '';
		await copyText(code);
		const label = button.querySelector('span');
		if (!label) return;
		label.textContent = 'Copied';
		setTimeout(() => (label.textContent = 'Copy'), 2000);
	};
	node.addEventListener('click', onClick);
	return () => node.removeEventListener('click', onClick);
}
