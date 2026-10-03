/*
 * nolune for iOS (ios/ in the repository) shows this web app in a window of its own, and listens
 * for a few messages from it (`nolune`, in its WebView.swift). In a browser there's no app.
 */

interface App {
	postMessage(message: { type: 'connect' }): void;
}

function app(): App | null {
	if (typeof window === 'undefined') return null;
	const webkit = (window as { webkit?: { messageHandlers?: Record<string, App | undefined> } })
		.webkit;
	return webkit?.messageHandlers?.nolune ?? null;
}

/** Whether the page is open in nolune for iOS. Only in the browser: the server can't tell. */
export function inIosApp(): boolean {
	return app() !== null;
}

/**
 * The app's own screens show some pages inside them, under the app's navigation: their web views
 * say so in the user agent, and the pages leave out their header and sidebar, the app's own.
 */
export const EMBEDDED_AGENT = 'nolune-embedded';

export function isEmbedded(userAgent: string | null): boolean {
	return !!userAgent?.includes(EMBEDDED_AGENT);
}

/** Back to the app's first screen, to connect to another family's nolune. */
export function connectElsewhere(): void {
	app()?.postMessage({ type: 'connect' });
}
