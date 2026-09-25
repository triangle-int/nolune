import { browser } from '$app/environment';
import DOMPurify from 'dompurify';
import { Copy } from 'lucide';
import { Marked } from 'marked';
import { copyText } from './clipboard';

/** Lucide's copy icon as markup, for the buttons inside rendered HTML. */
const COPY_ICON = `<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${Copy.map(
	([tag, attrs]) =>
		`<${tag} ${Object.entries(attrs)
			.map(([name, value]) => `${name}="${value}"`)
			.join(' ')}/>`
).join('')}</svg>`;

function escapeHtml(text: string): string {
	return text
		.replaceAll('&', '&amp;')
		.replaceAll('<', '&lt;')
		.replaceAll('>', '&gt;')
		.replaceAll('"', '&quot;');
}

const marked = new Marked({
	gfm: true,
	renderer: {
		// Code blocks get a header with the language and a copy button, like ChatGPT.
		code({ text, lang }) {
			const language = escapeHtml((lang ?? '').split(/\s/)[0]);
			return `<div data-code-block class="my-3 overflow-hidden rounded-2xl border bg-muted/50">
<div class="flex h-9 items-center justify-between pr-2 pl-4 text-xs text-muted-foreground"><span>${language || 'text'}</span><button type="button" data-copy class="flex items-center gap-1.5 rounded-lg px-2 py-1 hover:bg-accent hover:text-foreground">${COPY_ICON}<span>Copy</span></button></div>
<pre class="overflow-x-auto px-4 pb-4"><code>${escapeHtml(text)}</code></pre></div>`;
		}
	}
});

let hooked = false;

/** Markdown to sanitized HTML. Only used in the browser; the server never renders messages. */
export function renderMarkdown(text: string): string {
	if (!browser) return `<p>${escapeHtml(text)}</p>`;
	if (!hooked) {
		DOMPurify.addHook('afterSanitizeAttributes', (node) => {
			if (node.tagName === 'A' && node.getAttribute('href')) {
				node.setAttribute('target', '_blank');
				node.setAttribute('rel', 'noopener noreferrer');
			}
		});
		hooked = true;
	}
	const html = marked.parse(text, { async: false });
	return DOMPurify.sanitize(html, { ADD_ATTR: ['target'] });
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
