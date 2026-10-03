import Foundation

// A profile's pages as the gateway serves them to apps (#137, DESIGN.md "The API for apps"):
// memory, automations, skills, people & profile, a folder, and pictures. Times are milliseconds
// since 1970; what the pages say in words (schedules, calendar titles) comes in the person's
// language.

// MARK: - Memory

/// `GET /api/p/<slug>/memory`.
struct MemoryOverview: Decodable, Equatable {
	struct Fact: Decodable, Hashable {
		let text: String
		let learnedAt: Double?
	}

	/// A note: Markdown, its facts (its list items), and when it was last written, which a save
	/// sends back (`basedOn`) so it doesn't overwrite someone else's change.
	struct Note: Decodable, Identifiable, Hashable {
		let path: String
		let text: String
		let facts: [Fact]
		let updatedAt: Double

		var id: String { path }
		var updated: Date { Date(timeIntervalSince1970: updatedAt / 1000) }
	}

	/// A member's card, which goes with them into all their profiles.
	struct Card: Decodable, Identifiable, Hashable {
		let path: String
		let owner: String
		let ownerId: String
		let mine: Bool
		let file: Note?

		var id: String { path }
	}

	/// What the note-taker saved from a chat, which anyone can take back.
	struct Change: Decodable, Identifiable, Hashable {
		struct Undone: Decodable, Hashable {
			let at: Double
			let by: String?
		}

		struct Chat: Decodable, Hashable {
			let id: String
			let title: String
		}

		struct CardOwner: Decodable, Hashable {
			let ownerId: String
			let owner: String
		}

		let id: Int
		/// `add` or `replace`.
		let op: String
		let note: String
		let fact: String
		let before: String?
		let createdAt: Double
		let undone: Undone?
		let card: CardOwner?
		let conversation: Chat?

		var created: Date { Date(timeIntervalSince1970: createdAt / 1000) }
	}

	struct Core: Decodable, Equatable {
		let path: String
		let maxChars: Int
	}

	/// A member whose note is theirs.
	struct Member: Decodable, Hashable {
		let id: String
		let name: String
		let note: String
	}

	let files: [Note]
	let cards: [Card]
	let recent: [Change]
	let core: Core
	let members: [Member]
	let learnFromChats: Bool
}

/// A note's place and name, as the memory page tells them (packages/core/src/memory-categories.ts).
enum MemoryCategory: String, CaseIterable {
	case core, people, home, health, plans, routines, pets, places, projects, other

	/// The categories a note can move to, each its own note (`home.md`); people and projects have
	/// a note each, in their folder.
	static let notes: [MemoryCategory] = [.home, .health, .plans, .routines, .pets, .places, .projects, .other]

	/// Where a note belongs: `core.md`, `people/leo.md`, `home.md`; nil for one outside them.
	static func of(_ path: String) -> MemoryCategory? {
		if path == "core.md" { return .core }
		if path.hasPrefix("people/") { return .people }
		if path.hasPrefix("projects/") { return .projects }
		guard path.hasSuffix(".md"), !path.contains("/") else { return nil }
		return MemoryCategory(rawValue: String(path.dropLast(3)))
	}

	/// A note's title: its `# Title`, else its file's name.
	static func title(of note: MemoryOverview.Note) -> String {
		for line in note.text.split(separator: "\n", omittingEmptySubsequences: true).prefix(5) {
			if line.hasPrefix("# ") { return line.dropFirst(2).trimmingCharacters(in: .whitespaces) }
		}
		let name = note.path.split(separator: "/").last.map(String.init) ?? note.path
		return name.hasSuffix(".md") ? String(name.dropLast(3)) : name
	}

	/// A note's file name for a person or project, as the page makes it: `Anna Maria` is `anna-maria`.
	static func fileName(_ name: String) -> String {
		let folded = name.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: nil).lowercased()
		let words = folded.split { !($0.isLetter || $0.isNumber) }
		return words.joined(separator: "-")
	}
}

// MARK: - Automations

/// `GET /api/p/<slug>/automations`.
struct AutomationsOverview: Decodable, Equatable {
	struct Calendar: Decodable, Equatable {
		struct Day: Decodable, Identifiable, Equatable {
			struct Icon: Decodable, Hashable {
				let key: String
				let icon: String?
				let kind: String?
			}

			struct Entry: Decodable, Hashable {
				let triggerId: String?
				let name: String
				let icon: String?
				let kind: String?
				/// How it went, for a run that already happened.
				let status: String?
				let conversationId: String?
				let time: String
			}

			let key: String
			let day: Int
			let inMonth: Bool
			let isToday: Bool
			let isPast: Bool
			let title: String
			let relative: String?
			let icons: [Icon]
			let more: Int
			let entries: [Entry]

			var id: String { key }
		}

		/// One that runs too often to show on each day.
		struct Frequent: Decodable, Identifiable, Hashable {
			let id: String
			let name: String
			let icon: String?
			let kind: String
			let schedule: String
		}

		let title: String
		let weekdays: [String]
		let cells: [Day]
		let frequent: [Frequent]
		/// The months before and after (`2026-09`), and this one when another is shown.
		let prev: String?
		let next: String
		let current: String?
		let selected: String
	}

	struct Automation: Decodable, Identifiable, Hashable {
		struct Run: Decodable, Identifiable, Hashable {
			let id: String
			/// `cron`, `once`, `webhook`, `wake` or `manual`.
			let source: String
			/// `pending`, `running`, `ok`, `notified`, `silent`, `stopped` or `failed`.
			let status: String
			let at: String
			let conversationId: String?
			let output: String?
		}

		let id: String
		let name: String
		let summary: String?
		let icon: String?
		/// `cron`, `once` or `webhook`.
		let kind: String
		let schedule: String
		let cron: String?
		/// `on`, `paused` or `done`.
		let state: String
		let next: String?
		/// `agent` or `script`.
		let action: String
		let text: String
		let model: String
		let webhookUrl: String?
		let runs: [Run]
	}

	let timeZone: String
	let calendar: Calendar
	let triggers: [Automation]
}

// MARK: - Skills

/// A skill nolune has in the profile: `GET /api/p/<slug>/skills`.
struct Skill: Decodable, Identifiable, Hashable {
	let name: String
	let description: String
	/// `profile`, `global` or `builtin`.
	let scope: String
	let enabled: Bool
	/// About how much of each request it takes.
	let tokens: Int

	var id: String { name }
}

struct Skills: Decodable {
	let skills: [Skill]
}

// MARK: - People & profile

/// `GET /api/p/<slug>/settings`.
struct ProfileSettings: Decodable, Equatable {
	struct Member: Decodable, Identifiable, Hashable {
		let id: String
		let name: String
		/// Their note in memory, `people/anna.md`, once there is one.
		let note: String?
		let exists: Bool
		/// Other people notes that could be theirs.
		let candidates: [PersonNote]
	}

	let name: String
	let avatar: String
	let members: [Member]
	let people: [PersonNote]
	/// Who else has an account here, to add.
	let others: [String]
	let soul: String
	let maxSoul: Int
}

/// A note about someone in memory, to tell whose it is.
struct PersonNote: Decodable, Identifiable, Hashable {
	let path: String
	let title: String?
	let who: String?
	let aliases: [String]
	let facts: [String]

	var id: String { path }
}

/// What adding someone answered: added, or which note is theirs, first.
struct MemberAdded: Decodable {
	struct Choose: Decodable {
		let who: String
		let candidates: [PersonNote]
	}

	let message: String?
	let choose: Choose?
}

// MARK: - Folders

/// A folder as its page shows it: `GET /api/p/<slug>/folders/<id>`.
struct FolderDetail: Decodable, Equatable {
	struct File: Decodable, Identifiable, Hashable {
		let id: String
		let name: String
		let mime: String
		let bytes: Int
		let viewable: Bool
	}

	let id: String
	let name: String
	let instructions: String
	let files: [File]
	let maxFiles: Int
	let maxInstructions: Int
}

// MARK: - Pictures

/// `GET /api/p/<slug>/images`: the profile's picture templates, and whether nolune can make them.
struct ImagesOverview: Decodable, Equatable {
	let templates: [PictureTemplate]
	let ready: Bool
	/// Why it can't, when it can't.
	let problem: String?
}

struct PictureTemplate: Decodable, Identifiable, Hashable {
	enum Setting: Decodable, Hashable {
		struct Option: Decodable, Hashable {
			let value: String
			let label: String
		}

		/// One of its options, or the person's own words when `custom`.
		case select(id: String, label: String, default: String, options: [Option], custom: Bool)
		/// Up to `max` emoji.
		case emoji(id: String, label: String, default: String, max: Int)
		case text(id: String, label: String, default: String, placeholder: String?, required: Bool)

		var id: String {
			switch self {
			case .select(let id, _, _, _, _), .emoji(let id, _, _, _), .text(let id, _, _, _, _): return id
			}
		}

		var label: String {
			switch self {
			case .select(_, let label, _, _, _), .emoji(_, let label, _, _), .text(_, let label, _, _, _): return label
			}
		}

		var initial: String {
			switch self {
			case .select(_, _, let value, _, _), .emoji(_, _, let value, _), .text(_, _, let value, _, _): return value
			}
		}

		private enum Keys: String, CodingKey {
			case type, id, label, `default`, options, custom, max, placeholder, required
		}

		init(from decoder: Decoder) throws {
			let c = try decoder.container(keyedBy: Keys.self)
			let id = try c.decode(String.self, forKey: .id)
			let label = try c.decode(String.self, forKey: .label)
			let initial = (try? c.decode(String.self, forKey: .default)) ?? ""
			switch try c.decode(String.self, forKey: .type) {
			case "select":
				self = .select(
					id: id,
					label: label,
					default: initial,
					options: try c.decode([Option].self, forKey: .options),
					custom: (try? c.decode(Bool.self, forKey: .custom)) ?? false
				)
			case "emoji":
				self = .emoji(id: id, label: label, default: initial, max: (try? c.decode(Int.self, forKey: .max)) ?? 4)
			default:
				self = .text(
					id: id,
					label: label,
					default: initial,
					placeholder: try? c.decode(String.self, forKey: .placeholder),
					required: (try? c.decode(Bool.self, forKey: .required)) ?? false
				)
			}
		}
	}

	let id: String
	let name: String
	let title: String
	let description: String
	/// What it asks for, in words with the settings in them: `Make a {{theme}} invitation for {{name}}`.
	let sentence: String?
	let category: String
	let icon: String?
	/// Its tile's color, `#f6b7c1`, without a cover.
	let color: String?
	/// When its cover changed, for its address; nil without one.
	let cover: Double?
	/// `required`, `optional` or `none`.
	let image: String
	let imageLabel: String?
	/// `photo`, or `drawing` for one drawn on the spot.
	let imageSource: String
	let maxImages: Int
	/// `square`, `portrait`, `landscape` or `auto`.
	let size: String
	let settings: [Setting]

	/**
	 * Its sentence with the settings filled in, as the web shows it while they're picked:
	 * `{{name}}` is the value (or the setting's name while empty), `{{#age}}…{{/age}}` only when
	 * there's an age, and `{{image}}` the picture.
	 */
	func sentence(with values: [String: String], picture: String) -> String {
		guard var text = sentence else { return title }
		// Sections first: kept, without their markers, when their setting has a value.
		while let open = text.range(of: #"\{\{#([a-zA-Z0-9_-]+)\}\}"#, options: .regularExpression) {
			let key = String(text[open].dropFirst(3).dropLast(2))
			guard let close = text.range(of: "{{/\(key)}}", range: open.upperBound..<text.endIndex) else {
				text.removeSubrange(open)
				continue
			}
			let filled = key == "image" ? !picture.isEmpty : !(values[key] ?? "").isEmpty
			let inside = String(text[open.upperBound..<close.lowerBound])
			text.replaceSubrange(open.lowerBound..<close.upperBound, with: filled ? inside : "")
		}
		while let slot = text.range(of: #"\{\{([a-zA-Z0-9_-]+)\}\}"#, options: .regularExpression) {
			let key = String(text[slot].dropFirst(2).dropLast(2))
			if key == "image" {
				text.replaceSubrange(slot, with: picture)
			} else {
				let setting = settings.first { $0.id == key }
				let value = values[key] ?? ""
				text.replaceSubrange(slot, with: value.isEmpty ? (setting?.label.lowercased() ?? key) : label(of: value, for: setting))
			}
		}
		return text
	}

	/// An option's words, for its value.
	private func label(of value: String, for setting: Setting?) -> String {
		if case .select(_, _, _, let options, _)? = setting {
			return options.first { $0.value == value }?.label ?? value
		}
		return value
	}
}

// MARK: - Calls

extension Client {
	// Memory

	func memory(_ slug: String) async throws -> MemoryOverview {
		try await call("GET", "api/p/\(slug)/memory", as: MemoryOverview.self)
	}

	/// Writes a note, `basedOn` its `updatedAt` when it was opened (0 for a new one). A Refusal
	/// with status 409 when it changed since.
	func writeNote(_ slug: String, path: String, text: String, basedOn: Double) async throws {
		struct Body: Encodable {
			let path: String
			let text: String
			let basedOn: Double
		}
		try await call("PUT", "api/p/\(slug)/memory/notes", body: Body(path: path, text: text, basedOn: basedOn))
	}

	func forgetNote(_ slug: String, path: String) async throws {
		struct Body: Encodable { let path: String }
		try await call("DELETE", "api/p/\(slug)/memory/notes", body: Body(path: path))
	}

	/// Moves a note, or merges it into one that's there already. Says which it did.
	func moveNote(_ slug: String, from: String, to: String) async throws -> String {
		struct Body: Encodable {
			let from: String
			let to: String
		}
		struct Moved: Decodable { let message: String }
		return try await call("POST", "api/p/\(slug)/memory/moves", body: Body(from: from, to: to), as: Moved.self).message
	}

	/// Takes back what the note-taker saved. Why it couldn't, when it couldn't: `undone` (it was
	/// already), `changed` (the note changed since) or `owner` (it's someone else's card).
	func undoChange(_ slug: String, _ id: Int) async throws -> String? {
		var request = self.request("POST", "api/p/\(slug)/memory/\(id)/undo")
		request.setValue("application/json", forHTTPHeaderField: "Accept")
		let (data, response) = try await send(request)
		if response.statusCode == 409 {
			struct Reason: Decodable { let reason: String }
			return (try? JSONDecoder().decode(Reason.self, from: data))?.reason ?? "changed"
		}
		guard (200..<300).contains(response.statusCode) else {
			throw Refusal(address: Address.display(origin), status: response.statusCode, message: nil)
		}
		return nil
	}

	func learnFromChats(_ slug: String, _ on: Bool) async throws {
		struct Body: Encodable { let on: Bool }
		try await call("PUT", "api/p/\(slug)/memory/learning", body: Body(on: on))
	}

	// Automations

	func automations(_ slug: String, month: String? = nil) async throws -> AutomationsOverview {
		let query = month.map { [URLQueryItem(name: "month", value: $0)] } ?? []
		return try await call("GET", "api/p/\(slug)/automations", query: query, as: AutomationsOverview.self)
	}

	/// Changes an automation's description and instructions.
	func editAutomation(_ slug: String, _ id: String, summary: String, text: String) async throws {
		struct Body: Encodable {
			let summary: String
			let text: String
		}
		try await call("PATCH", "api/p/\(slug)/automations/\(id)", body: Body(summary: summary, text: text))
	}

	func setAutomation(_ slug: String, _ id: String, enabled: Bool) async throws {
		struct Body: Encodable { let enabled: Bool }
		try await call("PATCH", "api/p/\(slug)/automations/\(id)", body: Body(enabled: enabled))
	}

	/// Runs an automation now, giving back the chat its agent works in, when it started right away.
	@discardableResult
	func runAutomation(_ slug: String, _ id: String) async throws -> String? {
		struct Run: Decodable { let conversationId: String? }
		return try await call("POST", "api/p/\(slug)/automations/\(id)/run", as: Run.self).conversationId
	}

	func deleteAutomation(_ slug: String, _ id: String) async throws {
		try await call("DELETE", "api/p/\(slug)/automations/\(id)")
	}

	// Skills

	func skills(_ slug: String) async throws -> [Skill] {
		try await call("GET", "api/p/\(slug)/skills", as: Skills.self).skills
	}

	func setSkills(_ slug: String, _ names: [String], enabled: Bool) async throws -> [Skill] {
		struct Body: Encodable {
			let names: [String]
			let enabled: Bool
		}
		return try await call("PATCH", "api/p/\(slug)/skills", body: Body(names: names, enabled: enabled), as: Skills.self).skills
	}

	// People & profile

	func profileSettings(_ slug: String) async throws -> ProfileSettings {
		try await call("GET", "api/p/\(slug)/settings", as: ProfileSettings.self)
	}

	func renameProfile(_ slug: String, to name: String) async throws {
		struct Body: Encodable { let name: String }
		try await call("PATCH", "api/p/\(slug)/settings", body: Body(name: name))
	}

	func setAvatar(_ slug: String, _ avatar: String) async throws {
		struct Body: Encodable { let avatar: String }
		try await call("PATCH", "api/p/\(slug)/settings", body: Body(avatar: avatar))
	}

	func writeSoul(_ slug: String, _ soul: String) async throws {
		struct Body: Encodable { let soul: String }
		try await call("PATCH", "api/p/\(slug)/settings", body: Body(soul: soul))
	}

	/// Adds someone, with their note when it's known (a path, or `new`).
	func addMember(_ slug: String, _ who: String, note: String? = nil) async throws -> MemberAdded {
		struct Body: Encodable {
			let who: String
			let note: String?
		}
		return try await call("POST", "api/p/\(slug)/members", body: Body(who: who, note: note), as: MemberAdded.self)
	}

	func linkNote(_ slug: String, member: String, note: String) async throws {
		struct Body: Encodable { let note: String }
		try await call("PATCH", "api/p/\(slug)/members/\(member)", body: Body(note: note))
	}

	func removeMember(_ slug: String, _ member: String) async throws {
		try await call("DELETE", "api/p/\(slug)/members/\(member)")
	}

	func deleteProfile(_ slug: String) async throws {
		try await call("DELETE", "api/p/\(slug)")
	}

	// Folders

	func folder(_ slug: String, _ id: String) async throws -> FolderDetail {
		try await call("GET", "api/p/\(slug)/folders/\(id)", as: FolderDetail.self)
	}

	func setInstructions(_ slug: String, _ folder: String, _ instructions: String) async throws {
		struct Body: Encodable { let instructions: String }
		try await call("PATCH", "api/p/\(slug)/folders/\(folder)", body: Body(instructions: instructions))
	}

	/// Adds files uploaded for the folder (`upload`), answering with all of its files.
	func addFolderFiles(_ slug: String, _ folder: String, uploads: [String]) async throws -> [FolderDetail.File] {
		struct Body: Encodable { let uploads: [String] }
		struct Files: Decodable { let files: [FolderDetail.File] }
		return try await call("POST", "api/p/\(slug)/folders/\(folder)/files", body: Body(uploads: uploads), as: Files.self).files
	}

	func removeFolderFile(_ slug: String, _ folder: String, _ file: String) async throws {
		try await call("DELETE", "api/p/\(slug)/folders/\(folder)/files/\(file)")
	}

	/// A folder's file, shown as it is.
	func folderFile(_ slug: String, _ folder: String, _ file: String) -> URL {
		origin.appendingPathComponent("api/p/\(slug)/folders/\(folder)/files/\(file)")
	}

	// Pictures

	func images(_ slug: String) async throws -> ImagesOverview {
		try await call("GET", "api/p/\(slug)/images", as: ImagesOverview.self)
	}

	/// Starts a chat that makes a picture: from a template and its settings, or a description.
	func makePicture(
		_ slug: String,
		template: String?,
		settings: [String: String] = [:],
		uploads: [String] = [],
		shape: String? = nil,
		extra: String? = nil,
		text: String? = nil
	) async throws -> ChatSummary {
		struct Body: Encodable {
			let template: String?
			let settings: [String: String]
			let uploads: [String]
			let shape: String?
			let extra: String?
			let text: String?
		}
		let body = Body(template: template, settings: settings, uploads: uploads, shape: shape, extra: extra, text: text)
		return try await call("POST", "api/p/\(slug)/images", body: body, as: ChatSummary.self)
	}

	/// A template's cover picture.
	func cover(_ slug: String, _ template: PictureTemplate) -> URL? {
		guard let cover = template.cover else { return nil }
		var components = URLComponents(url: origin.appendingPathComponent("api/p/\(slug)/templates/\(template.id)/cover"), resolvingAgainstBaseURL: false)
		components?.queryItems = [URLQueryItem(name: "v", value: String(Int(cover)))]
		return components?.url
	}

	// The account

	/// A new picture of the signed-in person: a small square, as Settings crops it.
	func setPicture(_ jpeg: Data) async throws {
		var request = self.request("PUT", "api/me/picture")
		request.setValue("image/jpeg", forHTTPHeaderField: "Content-Type")
		request.setValue("application/json", forHTTPHeaderField: "Accept")
		request.httpBody = jpeg
		let (data, response) = try await send(request)
		guard (200..<300).contains(response.statusCode) else {
			struct Message: Decodable { let message: String }
			let message = try? JSONDecoder().decode(Message.self, from: data).message
			throw Refusal(address: Address.display(origin), status: response.statusCode, message: message)
		}
	}
}
