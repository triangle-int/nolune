import Foundation

/**
 * The family's nolune as the share extension reaches it: as the person signed in to the app, with
 * the cookies it left (Shared.Session), and only what sharing needs (DESIGN.md, "The API for apps").
 */
struct ShareClient {
	/// A chat to send to, as lists show it.
	struct Chat: Decodable, Identifiable, Hashable {
		let id: String
		/// Empty until it has one.
		let title: String
	}

	struct Problem: LocalizedError {
		let message: String
		var errorDescription: String? { message }
	}

	let session: Shared.Session
	private var urls: URLSession { Self.urls }
	private static let urls: URLSession = {
		let configuration = URLSessionConfiguration.ephemeral
		configuration.httpShouldSetCookies = false
		configuration.httpCookieAcceptPolicy = .never
		return URLSession(configuration: configuration)
	}()

	init(session: Shared.Session) {
		self.session = session
	}

	/// The profile's latest chats.
	func chats(_ slug: String) async throws -> [Chat] {
		struct Page: Decodable { let chats: [Chat] }
		let request = self.request("GET", "api/p/\(slug)/chats", query: [URLQueryItem(name: "limit", value: "20")])
		return try JSONDecoder().decode(Page.self, from: try await send(request)).chats
	}

	/// Uploads a file for a message, giving back its id.
	func upload(_ slug: String, file: URL, named name: String) async throws -> String {
		struct Upload: Decodable { let id: String }
		var request = self.request("POST", "api/p/\(slug)/uploads")
		request.timeoutInterval = 600
		request.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
		request.setValue(Self.uriComponent(name), forHTTPHeaderField: "x-file-name")
		let (data, response) = try await urls.upload(for: request, fromFile: file)
		return try JSONDecoder().decode(Upload.self, from: try check(data, response)).id
	}

	/// Starts a chat with the message, giving back its id.
	func startChat(_ slug: String, text: String, uploads: [String]) async throws -> String {
		struct Body: Encodable {
			let text: String
			let uploads: [String]
		}
		var request = self.request("POST", "api/p/\(slug)/chats")
		request.setValue("application/json", forHTTPHeaderField: "Content-Type")
		request.httpBody = try JSONEncoder().encode(Body(text: text, uploads: uploads))
		return try JSONDecoder().decode(Chat.self, from: try await send(request)).id
	}

	/// Sends the message in a chat.
	func send(_ chat: String, text: String, uploads: [String]) async throws {
		struct Body: Encodable {
			let text: String
			let uploads: [String]
		}
		var request = self.request("POST", "api/c/\(chat)/messages")
		request.setValue("application/json", forHTTPHeaderField: "Content-Type")
		request.httpBody = try JSONEncoder().encode(Body(text: text, uploads: uploads))
		_ = try await send(request)
	}

	private func request(_ method: String, _ path: String, query: [URLQueryItem] = []) -> URLRequest {
		var url = session.origin.appendingPathComponent(path)
		if !query.isEmpty, var components = URLComponents(url: url, resolvingAgainstBaseURL: false) {
			components.queryItems = query
			url = components.url ?? url
		}
		var request = URLRequest(url: url, timeoutInterval: 30)
		request.httpMethod = method
		request.cachePolicy = .reloadIgnoringLocalCacheData
		request.setValue("application/json", forHTTPHeaderField: "Accept")
		request.setValue(session.cookies, forHTTPHeaderField: "Cookie")
		return request
	}

	private func send(_ request: URLRequest) async throws -> Data {
		let (data, response) = try await urls.data(for: request)
		return try check(data, response)
	}

	/// What it answered, or what went wrong: its `{ message }`, or that the person isn't signed in.
	private func check(_ data: Data, _ response: URLResponse) throws -> Data {
		let status = (response as? HTTPURLResponse)?.statusCode ?? 0
		if (200..<300).contains(status) { return data }
		if status == 401 {
			throw Problem(message: String(localized: "Open nolune and sign in again."))
		}
		struct Message: Decodable { let message: String }
		let message = try? JSONDecoder().decode(Message.self, from: data).message
		throw Problem(message: message ?? String(localized: "nolune answered with an error (\(status))."))
	}

	/// A file's name for the `x-file-name` header, as `encodeURIComponent` has it.
	static func uriComponent(_ text: String) -> String {
		let unreserved = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.!~*'()")
		return text.addingPercentEncoding(withAllowedCharacters: unreserved) ?? text
	}
}
