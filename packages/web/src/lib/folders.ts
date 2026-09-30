import { invalidate } from '$app/navigation';
import { errorMessage } from './http';

export interface FolderItem {
	id: string;
	name: string;
}

async function post(url: string, body: unknown): Promise<unknown> {
	const res = await fetch(url, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify(body)
	});
	const text = await res.text();
	if (!res.ok) {
		const message = errorMessage(text, res.headers.get('content-type'));
		throw new Error(message ?? `Request failed (${res.status})`);
	}
	try {
		return JSON.parse(text);
	} catch {
		return null;
	}
}

/** Creates a folder in the profile; the sidebar lists it once the layout reloads. */
export async function createFolder(slug: string, name: string): Promise<FolderItem> {
	const created = (await post(`/api/p/${encodeURIComponent(slug)}/folders`, {
		name
	})) as FolderItem;
	await invalidate('nolune:conversations');
	return created;
}

/** Moves a chat into a folder, or out of any with `null`. */
export async function moveChat(conversationId: string, folderId: string | null): Promise<void> {
	await post(`/api/c/${conversationId}/folder`, { folderId });
	await invalidate('nolune:conversations');
}
