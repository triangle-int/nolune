/**
 * Where a profile picture is served from, by the SHA-256 of what's stored (user.picture): a new
 * picture gets a new address, so browsers can keep each one for good.
 */
export function pictureUrl(sha256: string | null | undefined): string | null {
	return sha256 ? `/api/pictures/${sha256}` : null;
}
