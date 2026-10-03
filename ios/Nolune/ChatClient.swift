import Foundation

// A chat's calls (DESIGN.md, "The API for apps"), for the native chat and composer (#134, #136).

/// What a new chat starts with, and what a chat can switch to: `GET /api/p/<slug>/new-chat`.
struct ChatOptions: Decodable, Equatable {
	struct Preset: Decodable, Identifiable, Hashable {
		let id: String
		let name: String
		let provider: String
	}

	/// The person's chips for a new chat; `suggestionsStale`: new ones are worth asking for.
	let suggestions: [Suggestion]
	let suggestionsStale: Bool
	let presets: [Preset]
	let defaultPresetId: String
	/// `low` to `max`.
	let efforts: [String]
	/// How commands run unless a chat says otherwise: `auto` or `unrestricted`.
	let commandMode: String
}

/// A new chat's chip: its icon, what it says, and the text it puts in the composer.
struct Suggestion: Decodable, Hashable {
	let icon: IconNode
	let label: String
	let text: String
}

/// A file uploaded for a message, before it's sent.
struct Upload: Decodable, Equatable {
	let id: String
	let name: String
	let mime: String
	let bytes: Int
}

extension Client {
	func chatOptions(_ slug: String) async throws -> ChatOptions {
		try await call("GET", "api/p/\(slug)/new-chat", as: ChatOptions.self)
	}

	/// New chips, made from the profile's memory when it changed. Waits for the model, a minute at most.
	func suggestions(_ slug: String) async throws -> [Suggestion] {
		var request = self.request("GET", "api/p/\(slug)/suggestions")
		request.timeoutInterval = 90
		request.setValue("application/json", forHTTPHeaderField: "Accept")
		let (data, response) = try await send(request)
		guard response.statusCode == 200 else { throw Refusal(address: Address.display(origin), status: response.statusCode, message: nil) }
		return try JSONDecoder().decode([Suggestion].self, from: data)
	}

	/// Sends a message, with files uploaded for it. nolune reads it after its current step.
	func send(_ chat: String, text: String, uploads: [String]) async throws {
		struct Message: Encodable {
			let text: String
			let uploads: [String]
		}
		try await call("POST", "api/c/\(chat)/messages", body: Message(text: text, uploads: uploads))
	}

	/// Stops nolune's work in the chat, in the background too.
	func stop(_ chat: String) async throws {
		try await call("POST", "api/c/\(chat)/stop")
	}

	/// Has nolune answer the last message, after it stopped or went wrong.
	func resume(_ chat: String) async throws {
		try await call("POST", "api/c/\(chat)/continue")
	}

	func setPreset(_ chat: String, to preset: String) async throws -> ChatModel {
		struct Body: Encodable { let presetId: String }
		return try await call("POST", "api/c/\(chat)/preset", body: Body(presetId: preset), as: ChatModel.self)
	}

	func setEffort(_ chat: String, to effort: String) async throws -> ChatModel {
		struct Body: Encodable { let effort: String }
		return try await call("POST", "api/c/\(chat)/effort", body: Body(effort: effort), as: ChatModel.self)
	}

	func setCommands(_ chat: String, to mode: String) async throws -> ChatCommands {
		struct Body: Encodable { let mode: String }
		return try await call("POST", "api/c/\(chat)/commands", body: Body(mode: mode), as: ChatCommands.self)
	}

	/// Loads the services and skills that changed since the chat's tools were.
	func reloadTools(_ chat: String) async throws {
		try await call("POST", "api/c/\(chat)/tools")
	}

	/// Says whether this person is typing in the chat; the nolune forgets it after 8 seconds.
	func typing(_ chat: String, _ typing: Bool) async throws {
		struct Body: Encodable { let typing: Bool }
		try await call("POST", "api/c/\(chat)/typing", body: Body(typing: typing))
	}

	/// Takes back a note the note-taker saved.
	func undo(_ slug: String, memory change: Int) async throws {
		try await call("POST", "api/p/\(slug)/memory/\(change)/undo")
	}

	/// A file for a message, uploaded as it is, its name in a header (`x-file-name`).
	func upload(_ slug: String, _ data: Data, named name: String) async throws -> Upload {
		var request = self.request("POST", "api/p/\(slug)/uploads")
		request.timeoutInterval = 600
		request.setValue("application/json", forHTTPHeaderField: "Accept")
		request.setValue("application/octet-stream", forHTTPHeaderField: "Content-Type")
		request.setValue(Self.uriComponent(name), forHTTPHeaderField: "x-file-name")
		request.httpBody = data
		let (answer, response) = try await send(request)
		guard (200..<300).contains(response.statusCode) else {
			struct Message: Decodable { let message: String }
			let message = try? JSONDecoder().decode(Message.self, from: answer).message
			throw Refusal(address: Address.display(origin), status: response.statusCode, message: message)
		}
		return try JSONDecoder().decode(Upload.self, from: answer)
	}

	/// A file uploaded and then taken off the message.
	func deleteUpload(_ slug: String, _ id: String) async {
		_ = try? await call("DELETE", "api/p/\(slug)/uploads/\(id)")
	}

	/// Where a chat's picture or file is: shown as it is, or as the original to save (`download`).
	func media(_ chat: String, _ id: String, download: Bool = false) -> URL {
		let url = origin.appendingPathComponent("api/c/\(chat)/media/\(id)")
		guard download, var components = URLComponents(url: url, resolvingAgainstBaseURL: false) else { return url }
		components.queryItems = [URLQueryItem(name: "download", value: nil)]
		return components.url ?? url
	}

	/// As JavaScript's `encodeURIComponent`, which the gateway decodes.
	static func uriComponent(_ text: String) -> String {
		let unreserved = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_.!~*'()"))
		return text.addingPercentEncoding(withAllowedCharacters: unreserved) ?? text
	}
}
