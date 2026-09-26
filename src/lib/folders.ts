import { invalidate } from '$app/navigation';

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
	let data: { message?: string } | null = null;
	try {
		data = JSON.parse(text);
	} catch {
		// plain text
	}
	if (!res.ok) throw new Error(data?.message ?? (text || `Request failed (${res.status})`));
	return data;
}

/** Creates a folder in the profile; the sidebar lists it once the layout reloads. */
export async function createFolder(slug: string, name: string): Promise<FolderItem> {
	const created = (await post(`/api/p/${encodeURIComponent(slug)}/folders`, {
		name
	})) as FolderItem;
	await invalidate('btw:conversations');
	return created;
}

/** Moves a chat into a folder, or out of any with `null`. */
export async function moveChat(conversationId: string, folderId: string | null): Promise<void> {
	await post(`/api/c/${conversationId}/folder`, { folderId });
	await invalidate('btw:conversations');
}
