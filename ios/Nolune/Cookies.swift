import Foundation
import WebKit

/**
 * The family's nolune's cookies in both of the app's stores: URLSession's (`HTTPCookieStorage`),
 * which native screens' requests carry (Client.swift), and the web views' (`WKWebsiteDataStore`).
 * Whoever signs in natively opens the web app's pages signed in, and whoever signed in on a page
 * (an invite's, or the web app before the native screens) is signed in natively too.
 */
@MainActor
enum Cookies {
	private static var web: WKHTTPCookieStore { WKWebsiteDataStore.default().httpCookieStore }

	/// The app's cookies for the nolune, into the web views'.
	static func toWeb(_ origin: URL, from storage: HTTPCookieStorage = .shared) async {
		for cookie in storage.cookies(for: origin) ?? [] {
			await withCheckedContinuation { done in web.setCookie(cookie) { done.resume() } }
		}
	}

	/**
	 * The web views' cookies for the nolune, into the app's. True when a session's came with
	 * them. `unlessSignedIn` leaves the app's own session be, like one that signed in meanwhile.
	 */
	@discardableResult
	static func fromWeb(
		_ origin: URL,
		unlessSignedIn: Bool = false,
		to storage: HTTPCookieStorage = .shared
	) async -> Bool {
		let cookies = await webCookies().filter { goes($0, to: origin) }
		if unlessSignedIn, (storage.cookies(for: origin) ?? []).contains(where: isSession) { return false }
		storage.setCookies(cookies, for: origin, mainDocumentURL: origin)
		return cookies.contains(where: isSession)
	}

	/// Signed out: the session's cookies go from both. The rest stay, like the web app's settings.
	static func signOut(_ origin: URL, in storage: HTTPCookieStorage = .shared) async {
		for cookie in storage.cookies(for: origin) ?? [] where isSession(cookie) {
			storage.deleteCookie(cookie)
		}
		for cookie in await webCookies() where isSession(cookie) && goes(cookie, to: origin) {
			await withCheckedContinuation { done in web.delete(cookie) { done.resume() } }
		}
	}

	/// A cookie the nolune's pages set, as the web views have it.
	static func webCookie(_ name: String, for origin: URL) async -> HTTPCookie? {
		await webCookies().first { $0.name == name && goes($0, to: origin) }
	}

	/// A cookie for the nolune's pages, in both stores.
	static func set(_ cookie: HTTPCookie, in storage: HTTPCookieStorage = .shared) async {
		storage.setCookie(cookie)
		await withCheckedContinuation { done in web.setCookie(cookie) { done.resume() } }
	}

	private static func webCookies() async -> [HTTPCookie] {
		await withCheckedContinuation { done in web.getAllCookies { done.resume(returning: $0) } }
	}

	/// better-auth's (`better-auth.session_token` and the like; `__Secure-` first over https).
	nonisolated static func isSession(_ cookie: HTTPCookie) -> Bool {
		cookie.name.hasPrefix("better-auth.") || cookie.name.hasPrefix("__Secure-better-auth.")
	}

	/// Whether a browser sends the cookie to the address: its host's, or a domain it's in.
	nonisolated static func goes(_ cookie: HTTPCookie, to origin: URL) -> Bool {
		guard let host = origin.host?.lowercased() else { return false }
		if cookie.isSecure, origin.scheme?.lowercased() != "https" { return false }
		let domain = cookie.domain.lowercased()
		let bare = domain.hasPrefix(".") ? String(domain.dropFirst()) : domain
		return host == bare || host.hasSuffix(".\(bare)")
	}
}
