import Foundation

/// What a nolune says about itself at `/api/version` (DESIGN.md, "The API for apps").
struct Server: Decodable, Equatable {
	/// The API level the native screens read. A nolune below it gets today's web app.
	static let nativeAPI = 1

	let version: String
	let api: Int
	/// What it serves natively to whoever is signed in; nil when nobody is.
	let capabilities: [String]?

	var isNative: Bool { api >= Server.nativeAPI }
}

/**
 * The family's nolune's JSON, as whoever signed in on this iPhone (DESIGN.md, "The API for apps").
 * The session is better-auth's cookie, in the session's cookie storage (`HTTPCookieStorage.shared`
 * for the app's), which Cookies.swift copies to the web views', so pages open signed in too.
 */
struct Client {
	enum Problem: Error, Equatable {
		case wrongPassword
		/// better-auth allows five tries a minute (`rateLimit` in packages/web/src/lib/server/auth.ts).
		case tooManyTries
		/// It answered with an error.
		case refused(Int)
	}

	let origin: URL
	var session: URLSession = .shared

	/**
	 * What the nolune says it is. Nil from a nolune before the API for apps, which answers something
	 * else (its 401 or 404). Throws when it doesn't answer, or answers with a server's error, like
	 * the relay's page for a computer that's off.
	 */
	func version() async throws -> Server? {
		let (data, response) = try await get("api/version")
		if response.statusCode >= 500 { throw Problem.refused(response.statusCode) }
		guard response.statusCode == 200 else { return nil }
		return try? JSONDecoder().decode(Server.self, from: data)
	}

	func get(_ path: String) async throws -> (Data, HTTPURLResponse) {
		var request = self.request("GET", path)
		request.setValue("application/json", forHTTPHeaderField: "Accept")
		return try await send(request)
	}

	/**
	 * Signs in with better-auth (`POST /api/auth/sign-in/email`), and keeps the session's cookie.
	 * Sent without the cookies the app has, and so without an `Origin`: better-auth checks the
	 * origin of requests that carry cookies against the address the nolune was set up with, which
	 * the app may reach it by another of (a computer on this network). The sign-in page signs in on
	 * the server, which isn't checked.
	 */
	func signIn(email: String, password: String) async throws {
		var request = self.request("POST", "api/auth/sign-in/email")
		request.setValue("application/json", forHTTPHeaderField: "Content-Type")
		request.httpBody = try JSONEncoder().encode([
			"email": email.trimmingCharacters(in: .whitespacesAndNewlines).lowercased(),
			"password": password
		])
		request.httpShouldHandleCookies = false
		let (_, response) = try await send(request)
		switch response.statusCode {
		case 200..<300:
			let headers = response.allHeaderFields as? [String: String] ?? [:]
			let url = response.url ?? origin
			let cookies = HTTPCookie.cookies(withResponseHeaderFields: headers, for: url)
			session.configuration.httpCookieStorage?.setCookies(cookies, for: url, mainDocumentURL: url)
		case 400, 401:
			throw Problem.wrongPassword
		case 429:
			throw Problem.tooManyTries
		default:
			throw Problem.refused(response.statusCode)
		}
	}

	/**
	 * Signs out as the web app's menu does (`POST /logout`), after telling the nolune to stop
	 * sending this iPhone notifications (`DELETE /api/push`). What they answer doesn't matter: a
	 * session that's gone is signed out already.
	 */
	func signOut() async {
		_ = try? await send(request("DELETE", "api/push"))
		_ = try? await send(request("POST", "logout"))
	}

	private func request(_ method: String, _ path: String) -> URLRequest {
		var request = URLRequest(url: origin.appendingPathComponent(path), timeoutInterval: 15)
		request.httpMethod = method
		request.cachePolicy = .reloadIgnoringLocalCacheData
		return request
	}

	private func send(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
		let (data, response) = try await session.data(for: request)
		guard let response = response as? HTTPURLResponse else { throw URLError(.badServerResponse) }
		return (data, response)
	}
}
