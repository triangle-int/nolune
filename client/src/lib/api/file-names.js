/** Human-readable file names for links the companion shares in chat. */

/** @param {string} href @returns {string | null} */
function lastPathSegment(href) {
	try {
		const url = new URL(href);
		const segment = url.pathname.split('/').filter(Boolean).pop() ?? '';
		const name = decodeURIComponent(segment).trim();
		return name ? name : null;
	} catch {
		return null;
	}
}

/** @param {string} text */
function looksLikeUrl(text) {
	return /^[a-z][a-z0-9+.-]*:\/\//i.test(text);
}

/**
 * Pick the name shown in the viewer and used for downloads. A labelled link
 * keeps its label; an autolinked bare URL uses the last path segment instead
 * of the whole URL, so the capability query never becomes the file name.
 */
/** @param {string | null | undefined} text @param {string} href @returns {string} */
export function linkFileName(text, href) {
	const label = (text ?? '').trim();
	if (label && !looksLikeUrl(label)) return label;
	return lastPathSegment(href) ?? 'file';
}

/** @param {string} name */
function stripPath(name) {
	return name.split(/[\\/]/).filter(Boolean).pop() ?? '';
}

/** Filename from a Content-Disposition header, preferring the RFC 5987 form. */
/** @param {string | null | undefined} header @returns {string | null} */
export function filenameFromContentDisposition(header) {
	if (!header) return null;
	const extended = header.match(/filename\*\s*=\s*(?:[^']*)'[^']*'([^;]+)/i);
	if (extended) {
		try {
			const name = stripPath(decodeURIComponent(extended[1].trim()));
			if (name) return name;
		} catch {
			/* fall back to the plain filename */
		}
	}
	const plain = header.match(/filename\s*=\s*(?:"((?:[^"\\]|\\.)*)"|([^;]+))/i);
	if (!plain) return null;
	const raw = plain[1] !== undefined ? plain[1].replace(/\\(.)/g, '$1') : plain[2];
	const name = stripPath(raw.trim());
	return name ? name : null;
}
