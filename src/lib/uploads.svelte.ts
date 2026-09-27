import { SvelteMap } from 'svelte/reactivity';
import type { Messages } from './i18n';

/** The server's limits (MAX_ATTACHMENTS and MAX_MEDIA_BYTES in @btw/core), checked early. */
export const MAX_FILES = 10;
export const MAX_FILE_BYTES = 100 * 1024 * 1024;

export interface PendingFile {
	key: string;
	name: string;
	size: number;
	/** A local preview, for pictures the browser can show. */
	preview: string | null;
	/** 0 to 1 while uploading. */
	progress: number;
	status: 'uploading' | 'ready' | 'failed';
	/** The upload's id once it's on the server; the message is sent with it. */
	id?: string;
	error?: string;
}

const BROWSER_PICTURES = new Set([
	'image/png',
	'image/jpeg',
	'image/gif',
	'image/webp',
	'image/avif'
]);

/** The message SvelteKit's `error()` sends, or the raw text. */
function errorMessage(xhr: XMLHttpRequest, m: Messages): string {
	try {
		const body = JSON.parse(xhr.responseText) as { message?: unknown };
		if (typeof body.message === 'string') return body.message;
	} catch {
		// not JSON
	}
	return xhr.responseText.trim() || m.errors.uploadFailed(xhr.status || null);
}

/**
 * Files attached in a composer. Each one uploads as soon as it's added, so sending is quick;
 * the message then only carries their ids.
 */
export class Attachments {
	files = $state<PendingFile[]>([]);
	readonly uploading = $derived(this.files.some((f) => f.status === 'uploading'));
	/** Ids of the files ready to send, in order. */
	readonly ids = $derived(this.files.flatMap((f) => (f.status === 'ready' && f.id ? [f.id] : [])));

	#slug: () => string;
	#m: Messages;
	#requests = new SvelteMap<string, XMLHttpRequest>();

	/** `slug`: the profile the files are for. `m`: the words for what goes wrong. */
	constructor(slug: () => string, m: Messages) {
		this.#slug = slug;
		this.#m = m;
	}

	add(list: Iterable<File>) {
		for (const file of list) {
			const key = crypto.randomUUID();
			const preview = BROWSER_PICTURES.has(file.type) ? URL.createObjectURL(file) : null;
			const pending: PendingFile = {
				key,
				name: file.name || 'file',
				size: file.size,
				preview,
				progress: 0,
				status: 'uploading'
			};
			if (this.files.length >= MAX_FILES) {
				pending.status = 'failed';
				pending.error = this.#m.errors.tooManyFiles(MAX_FILES);
			} else if (file.size > MAX_FILE_BYTES) {
				pending.status = 'failed';
				pending.error = this.#m.errors.tooLarge(MAX_FILE_BYTES / (1024 * 1024));
			}
			this.files.push(pending);
			if (pending.status === 'uploading') this.#upload(key, file);
		}
	}

	#find(key: string): PendingFile | undefined {
		return this.files.find((f) => f.key === key);
	}

	#upload(key: string, file: File) {
		const xhr = new XMLHttpRequest();
		this.#requests.set(key, xhr);
		xhr.open('POST', `/api/p/${encodeURIComponent(this.#slug())}/uploads`);
		xhr.setRequestHeader('x-file-name', encodeURIComponent(file.name || 'file'));
		xhr.upload.onprogress = (event) => {
			const item = this.#find(key);
			if (item && event.lengthComputable) item.progress = event.loaded / event.total;
		};
		xhr.onloadend = () => {
			this.#requests.delete(key);
			const item = this.#find(key);
			if (!item) return;
			if (xhr.status === 200) {
				item.id = (JSON.parse(xhr.responseText) as { id: string }).id;
				item.status = 'ready';
				item.progress = 1;
			} else if (xhr.status !== 0 || item.status === 'uploading') {
				item.status = 'failed';
				item.error = errorMessage(xhr, this.#m);
			}
		};
		xhr.send(file);
	}

	/** Takes a file out of the composer, and off the server if it got there. */
	remove(key: string) {
		const item = this.#find(key);
		if (!item) return;
		this.#requests.get(key)?.abort();
		this.#requests.delete(key);
		if (item.id) {
			fetch(`/api/p/${encodeURIComponent(this.#slug())}/uploads/${item.id}`, {
				method: 'DELETE'
			}).catch(() => {});
		}
		if (item.preview) URL.revokeObjectURL(item.preview);
		this.files = this.files.filter((f) => f.key !== key);
	}

	/** After these uploads were added somewhere (a folder): the server has taken them. */
	forget(ids: string[]) {
		const taken = (f: PendingFile) => !!f.id && ids.includes(f.id);
		for (const item of this.files)
			if (taken(item) && item.preview) URL.revokeObjectURL(item.preview);
		this.files = this.files.filter((f) => !taken(f));
	}

	/** After sending: the server has taken the files, so they're only forgotten here. */
	clear() {
		for (const item of this.files) if (item.preview) URL.revokeObjectURL(item.preview);
		this.files = [];
	}
}
