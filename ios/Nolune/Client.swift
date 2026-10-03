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

	func request(_ method: String, _ path: String, query: [URLQueryItem] = []) -> URLRequest {
		var url = origin.appendingPathComponent(path)
		if !query.isEmpty, var components = URLComponents(url: url, resolvingAgainstBaseURL: false) {
			components.queryItems = query
			url = components.url ?? url
		}
		var request = URLRequest(url: url, timeoutInterval: 15)
		request.httpMethod = method
		request.cachePolicy = .reloadIgnoringLocalCacheData
		return request
	}

	func send(_ request: URLRequest) async throws -> (Data, HTTPURLResponse) {
		let (data, response) = try await session.data(for: request)
		guard let response = response as? HTTPURLResponse else { throw URLError(.badServerResponse) }
		return (data, response)
	}
}

// MARK: - JSON

/// What the nolune said went wrong: the `{ message }` its JSON errors carry, or its status alone.
struct Refusal: LocalizedError, Equatable {
	let address: String
	let status: Int
	let message: String?

	var errorDescription: String? {
		message ?? String(localized: "\(address) answered with an error (\(status)).")
	}
}

extension Client {
	/// Calls a JSON endpoint and decodes its answer. Throws a Refusal for an error status.
	func call<Answer: Decodable>(
		_ method: String,
		_ path: String,
		query: [URLQueryItem] = [],
		body: (any Encodable)? = nil,
		as type: Answer.Type
	) async throws -> Answer {
		let data = try await call(method, path, query: query, body: body)
		return try JSONDecoder().decode(Answer.self, from: data)
	}

	/// Calls an endpoint, giving back what it answered. Throws a Refusal for an error status.
	@discardableResult
	func call(
		_ method: String,
		_ path: String,
		query: [URLQueryItem] = [],
		body: (any Encodable)? = nil
	) async throws -> Data {
		var request = self.request(method, path, query: query)
		request.setValue("application/json", forHTTPHeaderField: "Accept")
		if let body {
			request.setValue("application/json", forHTTPHeaderField: "Content-Type")
			request.httpBody = try JSONEncoder().encode(body)
		}
		let (data, response) = try await send(request)
		guard (200..<300).contains(response.statusCode) else {
			struct Message: Decodable { let message: String }
			let message = try? JSONDecoder().decode(Message.self, from: data).message
			throw Refusal(address: Address.display(origin), status: response.statusCode, message: message)
		}
		return data
	}

	/**
	 * Server-sent events (`text/event-stream`): each `data:` line, as it comes, until the stream
	 * ends or fails. The gateway's streams send a comment every 15 seconds, so one that says
	 * nothing for a minute is gone.
	 */
	func events(_ path: String) -> AsyncThrowingStream<Data, Error> {
		AsyncThrowingStream { continuation in
			let task = Task {
				do {
					var request = self.request("GET", path)
					request.timeoutInterval = 60
					request.setValue("text/event-stream", forHTTPHeaderField: "Accept")
					let (bytes, response) = try await session.bytes(for: request)
					let status = (response as? HTTPURLResponse)?.statusCode ?? 0
					guard status == 200 else {
						throw Refusal(address: Address.display(origin), status: status, message: nil)
					}
					for try await line in bytes.lines where line.hasPrefix("data:") {
						continuation.yield(Data(line.dropFirst(5).trimmingCharacters(in: .whitespaces).utf8))
					}
					continuation.finish()
				} catch {
					continuation.finish(throwing: error)
				}
			}
			continuation.onTermination = { _ in task.cancel() }
		}
	}
}

// MARK: - The family's nolune

extension Client {
	func me() async throws -> Me {
		try await call("GET", "api/me", as: Me.self)
	}

	/// Changes the signed-in person's name, giving back the name as kept.
	func renameMe(to name: String) async throws -> String {
		struct Name: Codable { let name: String }
		return try await call("PATCH", "api/me", body: Name(name: name), as: Name.self).name
	}

	func profiles() async throws -> [Profile] {
		try await call("GET", "api/profiles", as: Profiles.self).profiles
	}

	/// A page of the profile's chats, most recently active first; `after` is the last page's `next`.
	func chats(_ slug: String, after: String? = nil, limit: Int = 50) async throws -> ChatPage {
		var query = [URLQueryItem(name: "limit", value: String(limit))]
		if let after { query.append(URLQueryItem(name: "after", value: after)) }
		return try await call("GET", "api/p/\(slug)/chats", query: query, as: ChatPage.self)
	}

	/// What a new chat starts with; left out, the new chat page's defaults.
	struct NewChat: Encodable {
		var preset: String?
		var effort: String?
		var folder: String?
		var text: String?
		var uploads: [String]?
	}

	func startChat(_ slug: String, _ chat: NewChat = NewChat()) async throws -> ChatSummary {
		try await call("POST", "api/p/\(slug)/chats", body: chat, as: ChatSummary.self)
	}

	/// Renames a chat, giving back the title as the nolune kept it.
	func renameChat(_ id: String, to title: String) async throws -> String {
		struct Title: Codable { let title: String }
		return try await call("PATCH", "api/c/\(id)", body: Title(title: title), as: Title.self).title
	}

	func deleteChat(_ id: String) async throws {
		try await call("DELETE", "api/c/\(id)")
	}

	/// Moves a chat into a folder, or out of any with nil.
	func moveChat(_ id: String, to folder: String?) async throws {
		struct Move: Encodable {
			let folderId: String?
			enum CodingKeys: CodingKey { case folderId }
			// The null is sent, for out of any folder.
			func encode(to encoder: Encoder) throws {
				var container = encoder.container(keyedBy: CodingKeys.self)
				try container.encode(folderId, forKey: .folderId)
			}
		}
		try await call("POST", "api/c/\(id)/folder", body: Move(folderId: folder))
	}

	func folders(_ slug: String) async throws -> [Folder] {
		try await call("GET", "api/p/\(slug)/folders", as: Folders.self).folders
	}

	func createFolder(_ slug: String, named name: String) async throws -> Folder {
		struct Name: Encodable { let name: String }
		return try await call("POST", "api/p/\(slug)/folders", body: Name(name: name), as: Folder.self)
	}

	func renameFolder(_ slug: String, _ id: String, to name: String) async throws -> Folder {
		struct Name: Encodable { let name: String }
		return try await call("PATCH", "api/p/\(slug)/folders/\(id)", body: Name(name: name), as: Folder.self)
	}

	/// Deletes a folder for everyone in the profile; its chats stay, out of any folder.
	func deleteFolder(_ slug: String, _ id: String) async throws {
		try await call("DELETE", "api/p/\(slug)/folders/\(id)")
	}

	// MARK: The bell

	func bell() async throws -> Bell {
		try await call("GET", "api/notifications", as: Bell.self)
	}

	/// Opening the bell marks everything in it as read.
	func markBellSeen() async throws {
		try await call("POST", "api/notifications/seen")
	}

	func dismiss(_ notification: String) async throws {
		try await call("POST", "api/notifications/\(notification)/dismiss")
	}

	func clearBell() async throws {
		try await call("POST", "api/notifications/clear")
	}

	/// Turns a notification into a chat, or finds the one it was continued in.
	func continueInChat(_ notification: String) async throws -> ChatPlace {
		try await call("POST", "api/notifications/\(notification)/continue", as: ChatPlace.self)
	}

	// MARK: Notifications on this iPhone

	/// Registers the token Apple gave the app with this session (Push.swift).
	func registerPush(_ token: String, sandbox: Bool) async throws {
		struct Device: Encodable {
			let token: String
			let sandbox: Bool
		}
		try await call("POST", "api/push", body: Device(token: token, sandbox: sandbox))
	}

	/// Tells the nolune to stop sending this iPhone notifications.
	func forgetPush() async {
		_ = try? await call("DELETE", "api/push")
	}
}
