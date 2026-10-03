import Foundation

// What the family's nolune answers in JSON (DESIGN.md, "The API for apps"). Fields an app doesn't
// know are left out; times are milliseconds since 1970, as the gateway sends them.

/// Someone signed in: `GET /api/me`.
struct Me: Decodable, Equatable {
	let id: String
	let name: String
	let email: String
	let isAdmin: Bool
	let picture: String?
}

/// A profile the person is in: `GET /api/profiles`.
struct Profile: Decodable, Identifiable, Hashable {
	struct Member: Decodable, Hashable {
		let id: String
		let name: String
		let picture: String?
	}

	let slug: String
	let name: String
	let avatar: String
	let members: [Member]

	var id: String { slug }
}

/// A chat as lists show it: `GET /api/p/<slug>/chats`.
struct ChatSummary: Decodable, Identifiable, Hashable {
	let id: String
	/// Empty until it has one: the app says "New chat".
	var title: String
	let presetName: String
	var folderId: String?
	let updatedAt: Double
	let running: Bool

	var updated: Date { Date(timeIntervalSince1970: updatedAt / 1000) }
}

struct ChatPage: Decodable {
	let chats: [ChatSummary]
	/// What to pass as `after` for the next page; nil on the last.
	let next: String?
}

/// A folder of chats: `GET /api/p/<slug>/folders`.
struct Folder: Decodable, Identifiable, Hashable {
	let id: String
	var name: String
}

struct Folders: Decodable {
	let folders: [Folder]
}

struct Profiles: Decodable {
	let profiles: [Profile]
}

/// The bell: `GET /api/notifications`, newest first, and when the person last opened it.
struct Bell: Decodable, Equatable {
	struct Item: Decodable, Identifiable, Hashable {
		struct Source: Decodable, Hashable {
			let slug: String
			let name: String
			let avatar: String
		}

		let id: String
		let title: String
		let body: String
		/// `info` or `error`.
		let level: String
		let createdAt: Double
		let profile: Source
		/// The chat it was continued in, if anyone has.
		let conversationId: String?

		var created: Date { Date(timeIntervalSince1970: createdAt / 1000) }
	}

	var items: [Item]
	var seenAt: Double

	static let empty = Bell(items: [], seenAt: 0)

	var unseen: Int { items.filter { $0.createdAt > seenAt }.count }
}

/// Where a continued notification lives: `POST /api/notifications/<id>/continue`.
struct ChatPlace: Decodable {
	let slug: String
	let conversationId: String
}
