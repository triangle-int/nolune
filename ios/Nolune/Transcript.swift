import Foundation

// A chat as `GET /api/c/<id>/transcript` streams it (DESIGN.md, "The API for apps"): its
// transcript built in core (packages/core/src/transcript.ts, chat-view.ts), and the chat's other
// events as a page gets them (packages/core/src/runner.ts). Kinds the app doesn't know yet are
// kept as `unknown`, so a later nolune's don't break it. Times are milliseconds since 1970.

/// The model a chat runs on, and how hard it thinks.
struct ChatModel: Codable, Equatable {
	/// Nil once the model was removed in Models & keys: the chat keeps its name.
	let presetId: String?
	let presetName: String
	let provider: String
	let effort: String
	let contextWindow: Int?
}

/// How a chat's commands run: auto mode checks them first, unrestricted doesn't.
struct ChatCommands: Codable, Equatable {
	/// The chat's own choice; nil follows Models & keys (`fallback`).
	let mode: String?
	let fallback: String

	var current: String { mode ?? fallback }
}

/// Services and skills that changed since the chat's tools were loaded.
struct ToolChanges: Decodable, Equatable {
	struct Changes: Decodable, Equatable {
		let added: [String]
		let changed: [String]
		let removed: [String]
	}

	let skills: Changes?
	let services: Changes?
}

/// Work going on in the background: a command, or a subagent in a chat of its own.
struct BackgroundItem: Decodable, Equatable, Identifiable {
	/// `command` or `subagent`.
	let kind: String
	let id: String
	let summary: String?
	let command: String?
	let name: String?
	let conversationId: String?
	/// A subagent's: `pending`, `running` or `stopping`.
	let status: String?
	let startedAt: Double
}

/// Someone writing in the chat right now.
struct Typist: Decodable, Equatable, Identifiable {
	let id: String
	let name: String
}

/// A picture or file: one someone attached, or a copy of one a reply or a command showed.
struct ChatMedia: Decodable, Hashable {
	/// `ok`, or why there's no copy: `missing`, `unsupported`, `too_large`, `blocked`, `failed`.
	let status: String
	let id: String?
	let name: String
	let mime: String?
	let bytes: Int?
	let width: Int?
	let height: Int?
	/// A picture a browser shows (a HEIC comes as a JPEG copy).
	let viewable: Bool?
	/// What went wrong, for one without a copy.
	let error: String?
	/// An attachment's: why nolune got only the path where it's saved.
	let note: String?

	var isCopied: Bool { status == "ok" && id != nil }
}

struct Usage: Decodable, Equatable {
	let input: Int
	let cacheRead: Int
	let cacheWrite: Int
	let output: Int

	/// Everything the model read.
	var prompt: Int { input + cacheRead + cacheWrite }
}

/// A command's result, by its call.
struct CommandResult: Decodable, Equatable {
	enum Status: Equatable {
		case done, failed, stopped
		/// Auto mode didn't let it run; the output says why.
		case blocked
	}

	let output: String
	let isError: Bool
	/// What it showed with `nolune view`.
	let pictures: [ChatMedia]

	/// As `resultStatus` in core tells.
	var status: Status {
		if !isError { return .done }
		if output.hasPrefix("Blocked by auto mode") { return .blocked }
		return output.range(of: "Stopped by [^\\n]*$", options: .regularExpression) != nil ? .stopped : .failed
	}
}

/// A message that isn't nolune's: someone's, an automation's, a subagent's or a background command's.
struct ChatMessage: Decodable, Equatable {
	let id: Int
	/// `human`, `trigger`, `agent_message` or `task_result`.
	let kind: String
	let senderId: String?
	let senderName: String?
	let text: String?
	let attachments: [ChatMedia]?
	/// The automation's name, the subagent's, or the command's summary.
	let title: String?
	let output: String?
	let isError: Bool?
	let createdAt: Double
}

/// What the note-taker saved after reading the chat.
struct MemoryLook: Decodable, Equatable {
	struct Change: Decodable, Equatable, Identifiable {
		struct Undone: Decodable, Equatable {
			let at: Double
			let by: String?
		}

		let id: Int
		/// `add` or `replace`.
		let op: String
		/// Where it's kept, like `people/leo.md`.
		let note: String
		let fact: String
		let before: String?
		let undone: Undone?
	}

	let after: Int
	let createdAt: Double
	let changes: [Change]
}

/// A step of nolune's work, between two pieces of its text.
enum Step: Decodable, Equatable {
	case thinking(String)
	/// The conversation summarized, to go on from; empty while it's written.
	case compaction(summary: String, asked: Bool)
	case command(Command)
	case unknown

	struct Command: Decodable, Equatable {
		let id: String
		/// Nil while the model is still writing it.
		let command: String?
		let cwd: String?
		/// What it does, in plain words.
		let summary: String?
		/// The Lucide icon the model picked.
		let icon: String?
	}

	private enum Keys: String, CodingKey {
		case type, text, summary, asked
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: Keys.self)
		switch try container.decode(String.self, forKey: .type) {
		case "thinking":
			self = .thinking(try container.decode(String.self, forKey: .text))
		case "compaction":
			self = .compaction(
				summary: try container.decode(String.self, forKey: .summary),
				asked: (try? container.decode(Bool.self, forKey: .asked)) ?? false
			)
		case "command":
			self = .command(try Command(from: decoder))
		default:
			self = .unknown
		}
	}
}

/// A reply's text, or the steps of work between its texts.
enum Part: Decodable, Equatable, Identifiable {
	case text(key: String, text: String, media: [String: ChatMedia], pending: Bool)
	case activity(Activity)

	struct Activity: Decodable, Equatable {
		let key: String
		let steps: [Step]
		let startedAt: Double
		let endedAt: Double

		var commands: [Step.Command] {
			steps.compactMap { if case .command(let command) = $0 { return command } else { return nil } }
		}
	}

	var id: String {
		switch self {
		case .text(let key, _, _, _): return key
		case .activity(let activity): return activity.key
		}
	}

	private enum Keys: String, CodingKey {
		case type, key, text, media, pending
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: Keys.self)
		if try container.decode(String.self, forKey: .type) == "activity" {
			self = .activity(try Activity(from: decoder))
		} else {
			self = .text(
				key: try container.decode(String.self, forKey: .key),
				text: (try? container.decode(String.self, forKey: .text)) ?? "",
				media: (try? container.decode([String: ChatMedia].self, forKey: .media)) ?? [:],
				pending: (try? container.decode(Bool.self, forKey: .pending)) ?? false
			)
		}
	}
}

/// nolune's reply: its texts and work, and what it took.
struct Reply: Decodable, Equatable {
	let key: String
	let parts: [Part]
	let usage: Usage?
	let stopReasons: [String]
	/// The models that wrote it, in order.
	let models: [String]
	/// Still being written.
	let live: Bool

	/// Its words, for copying.
	var text: String {
		parts
			.compactMap { if case .text(_, let text, _, _) = $0 { return text.trimmingCharacters(in: .whitespacesAndNewlines) } else { return nil } }
			.filter { !$0.isEmpty }
			.joined(separator: "\n\n")
	}
}

/// One thing in a chat, in order.
enum Entry: Decodable, Equatable, Identifiable {
	case message(key: String, ChatMessage)
	case memory(key: String, MemoryLook)
	/// The conversation summarized after a reply: asked for, or the chat went quiet.
	case compaction(key: String, summary: String, asked: Bool, live: Bool)
	case reply(Reply)
	case unknown(key: String)

	var id: String {
		switch self {
		case .message(let key, _), .memory(let key, _), .compaction(let key, _, _, _), .unknown(let key): return key
		case .reply(let reply): return reply.key
		}
	}

	private enum Keys: String, CodingKey {
		case type, key, message, look, summary, asked, live
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: Keys.self)
		let key = try container.decode(String.self, forKey: .key)
		switch try container.decode(String.self, forKey: .type) {
		case "human", "trigger", "agent_message", "task_result":
			self = .message(key: key, try container.decode(ChatMessage.self, forKey: .message))
		case "memory":
			self = .memory(key: key, try container.decode(MemoryLook.self, forKey: .look))
		case "compaction":
			self = .compaction(
				key: key,
				summary: try container.decode(String.self, forKey: .summary),
				asked: (try? container.decode(Bool.self, forKey: .asked)) ?? false,
				live: (try? container.decode(Bool.self, forKey: .live)) ?? false
			)
		case "reply":
			self = .reply(try Reply(from: decoder))
		default:
			self = .unknown(key: key)
		}
	}
}

/// The output of the command running now, as it comes.
struct ToolOutput: Decodable, Equatable {
	let id: String
	var text: String
}

/// What `/transcript` sends: the chat first, then what changes.
enum ChatEvent: Decodable {
	struct Snapshot: Decodable {
		let title: String
		let model: ChatModel?
		let commands: ChatCommands
		let toolChanges: ToolChanges?
		let running: Bool
		let error: String?
		let queued: [ChatMessage]
		let toolOutput: ToolOutput?
		let background: [BackgroundItem]
		let typing: [Typist]
		let entries: [Entry]
		let results: [String: CommandResult]
	}

	case snapshot(Snapshot)
	/// Every entry's key in order, the entries new or changed, and results not sent before.
	case transcript(order: [String], entries: [Entry], results: [String: CommandResult])
	case status(running: Bool, error: String?)
	case queued([ChatMessage])
	case toolOutput(id: String, chunk: String)
	case title(String)
	case model(ChatModel)
	case commands(ChatCommands)
	case tools(ToolChanges?)
	case background([BackgroundItem])
	case typing([Typist])
	case other

	private enum Keys: String, CodingKey {
		case type, snapshot, order, entries, results, running, error, queued, id, chunk, title, model, commands
		case changes, background, typing
	}

	init(from decoder: Decoder) throws {
		let container = try decoder.container(keyedBy: Keys.self)
		switch try container.decode(String.self, forKey: .type) {
		case "snapshot":
			self = .snapshot(try container.decode(Snapshot.self, forKey: .snapshot))
		case "transcript":
			self = .transcript(
				order: try container.decode([String].self, forKey: .order),
				entries: try container.decode([Entry].self, forKey: .entries),
				results: try container.decode([String: CommandResult].self, forKey: .results)
			)
		case "status":
			self = .status(
				running: try container.decode(Bool.self, forKey: .running),
				error: try container.decodeIfPresent(String.self, forKey: .error)
			)
		case "queued":
			self = .queued(try container.decode([ChatMessage].self, forKey: .queued))
		case "tool_output":
			self = .toolOutput(id: try container.decode(String.self, forKey: .id), chunk: try container.decode(String.self, forKey: .chunk))
		case "title":
			self = .title(try container.decode(String.self, forKey: .title))
		case "model":
			self = .model(try container.decode(ChatModel.self, forKey: .model))
		case "commands":
			self = .commands(try container.decode(ChatCommands.self, forKey: .commands))
		case "tools":
			self = .tools(try container.decodeIfPresent(ToolChanges.self, forKey: .changes))
		case "background":
			self = .background(try container.decode([BackgroundItem].self, forKey: .background))
		case "typing":
			self = .typing(try container.decode([Typist].self, forKey: .typing))
		default:
			self = .other
		}
	}
}

/// A chat as the app shows it, kept from its events as a page keeps its ChatState.
struct ChatState: Equatable {
	var loaded = false
	var title = ""
	var model: ChatModel?
	var commands = ChatCommands(mode: nil, fallback: "auto")
	var toolChanges: ToolChanges?
	var running = false
	var error: String?
	/// Messages sent while nolune works, read after its current step.
	var queued: [ChatMessage] = []
	var toolOutput: ToolOutput?
	var background: [BackgroundItem] = []
	/// Everyone typing, this person too.
	var typing: [Typist] = []
	var entries: [Entry] = []
	var results: [String: CommandResult] = [:]

	mutating func apply(_ event: ChatEvent) {
		switch event {
		case .snapshot(let snapshot):
			loaded = true
			title = snapshot.title
			model = snapshot.model
			commands = snapshot.commands
			toolChanges = snapshot.toolChanges
			running = snapshot.running
			error = snapshot.error
			queued = snapshot.queued
			toolOutput = snapshot.toolOutput
			background = snapshot.background
			typing = snapshot.typing
			entries = snapshot.entries
			results = snapshot.results
		case .transcript(let order, let changed, let results):
			var byKey: [String: Entry] = [:]
			for entry in entries + changed { byKey[entry.id] = entry }
			entries = order.compactMap { byKey[$0] }
			self.results.merge(results) { _, new in new }
		case .status(let running, let error):
			self.running = running
			self.error = error
		case .queued(let queued):
			self.queued = queued
		case .toolOutput(let id, let chunk):
			if toolOutput?.id == id {
				toolOutput?.text += chunk
			} else {
				toolOutput = ToolOutput(id: id, text: chunk)
			}
		case .title(let title):
			self.title = title
		case .model(let model):
			self.model = model
		case .commands(let commands):
			self.commands = commands
		case .tools(let changes):
			toolChanges = changes
		case .background(let background):
			self.background = background
		case .typing(let typing):
			self.typing = typing
		case .other:
			break
		}
	}

	/**
	 * The last message has no reply, and none is coming: the chat offers Continue. Notes saved and
	 * summaries don't count.
	 */
	var unanswered: Bool {
		guard !running, queued.isEmpty else { return false }
		let last = entries.last { entry in
			switch entry {
			case .memory, .compaction, .unknown: return false
			case .message, .reply: return true
			}
		}
		guard let last else { return false }
		if case .reply = last { return false }
		return true
	}

	/// The newest reply still being written, and the step it's on.
	var liveReply: Reply? {
		guard case .reply(let reply)? = entries.last, reply.live else { return nil }
		return reply
	}

	/// The reply to the last message, unless the last message has none yet.
	var lastReply: Reply? {
		for entry in entries.reversed() {
			switch entry {
			case .reply(let reply): return reply
			case .message: return nil
			case .memory, .compaction, .unknown: continue
			}
		}
		return nil
	}

	/**
	 * The command nolune runs now, in the model's words, as a Live Activity shows it; nil between
	 * commands, and once it's done. As the gateway has it (`activityState` in core's
	 * live-activities.ts).
	 */
	var runningCommand: String? {
		guard running, case .activity(let activity)? = lastReply?.parts.last,
			case .command(let command)? = activity.steps.last, results[command.id] == nil
		else { return nil }
		return command.summary
	}

	/// The model's context used by the last call, for the technical details.
	var lastUsage: Usage? {
		for entry in entries.reversed() {
			if case .reply(let reply) = entry, let usage = reply.usage { return usage }
		}
		return nil
	}
}
