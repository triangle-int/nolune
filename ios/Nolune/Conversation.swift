import Combine
import Foundation

/**
 * One chat, live (#134): its transcript from `/api/c/<id>/transcript`, kept as the events come
 * (ChatState in Transcript.swift), and what the person does in it. The stream comes back after it
 * drops (the network, or the computer asleep) and when the app returns to the front.
 */
@MainActor
final class Conversation: ObservableObject {
	let id: String
	let slug: String
	let client: Client
	@Published private(set) var state = ChatState()
	/// Goes up with each event, for the view to follow the newest message.
	@Published private(set) var revision = 0
	/// The stream dropped, and is being followed again.
	@Published private(set) var reconnecting = false
	/// What went wrong with the last thing the person did, under the composer.
	@Published var problem: String?

	private let stream = Cancelling()
	private var activations: AnyCancellable?
	private lazy var typing = TypingReporter { [client, id] typing in
		try? await client.typing(id, typing)
	}

	init(id: String, slug: String, client: Client) {
		self.id = id
		self.slug = slug
		self.client = client
	}

	func start() {
		guard activations == nil else { return }
		follow()
		activations = AppModel.shared.$activations
			.dropFirst()
			.receive(on: DispatchQueue.main)
			.sink { [weak self] _ in self?.follow() }
	}

	func stop() {
		stream.task = nil
		activations = nil
		typing.stop()
	}

	private func follow() {
		stream.task = Task { [weak self, client, id] in
			var wait: UInt64 = 1
			while !Task.isCancelled {
				do {
					for try await data in client.events("api/c/\(id)/transcript") {
						guard let self else { return }
						wait = 1
						self.reconnecting = false
						guard let event = try? JSONDecoder().decode(ChatEvent.self, from: data) else { continue }
						self.state.apply(event)
						self.revision += 1
						switch event {
						case .snapshot, .transcript, .status, .title:
							// Its Live Activity, if it has one, while the app sees it.
							LiveActivities.update(chat: id, state: self.state)
						default:
							break
						}
					}
				} catch {
					// Dropped; tried again below.
				}
				guard !Task.isCancelled, let self else { return }
				self.reconnecting = true
				try? await Task.sleep(nanoseconds: wait * 1_000_000_000)
				wait = min(wait * 2, 30)
			}
		}
	}

	// MARK: What the person does

	/// What they're typing, for the others to see that they are.
	func typed(_ text: String) {
		typing.typed(empty: text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
	}

	func send(_ text: String, uploads: [String]) async -> Bool {
		let sent = await act { try await self.client.send(self.id, text: text, uploads: uploads) }
		if sent {
			typing.sent()
			Haptics.success()
		}
		return sent
	}

	func stopWork() async {
		Haptics.tap()
		await act { try await self.client.stop(self.id) }
	}

	/// Has nolune answer the last message: Continue, or Try again after an error.
	func resume() async {
		await act { try await self.client.resume(self.id) }
	}

	func setPreset(_ preset: String) async {
		await act {
			let model = try await self.client.setPreset(self.id, to: preset)
			self.state.apply(.model(model))
		}
	}

	func setEffort(_ effort: String) async {
		await act {
			let model = try await self.client.setEffort(self.id, to: effort)
			self.state.apply(.model(model))
		}
	}

	func setCommands(_ mode: String) async {
		await act {
			let commands = try await self.client.setCommands(self.id, to: mode)
			self.state.apply(.commands(commands))
		}
	}

	func reloadTools() async {
		await act {
			try await self.client.reloadTools(self.id)
			self.state.apply(.tools(nil))
		}
	}

	func undo(_ change: Int) async {
		await act { try await self.client.undo(self.slug, memory: change) }
	}

	/// Does the work, saying what went wrong when it fails. True when it didn't.
	@discardableResult
	private func act(_ work: () async throws -> Void) async -> Bool {
		do {
			try await work()
			problem = nil
			return true
		} catch {
			problem = error.localizedDescription
			Haptics.failure()
			return false
		}
	}
}

/**
 * Tells the chat this person is typing, as the web app's TypingReporter does: at most every 3
 * seconds while they type, and that they stopped once the box is empty or after 5 seconds without
 * a key. Reports go one after another, so they arrive in order.
 */
@MainActor
final class TypingReporter {
	private let report: (Bool) async -> Void
	private var reported = false
	private var lastSent: Date?
	private var idle: Task<Void, Never>?
	private var queue: Task<Void, Never>?

	init(report: @escaping (Bool) async -> Void) {
		self.report = report
	}

	func typed(empty: Bool) {
		guard !empty else { return stop() }
		idle?.cancel()
		idle = Task { [weak self] in
			try? await Task.sleep(nanoseconds: 5_000_000_000)
			guard !Task.isCancelled else { return }
			self?.stop()
		}
		if !reported || lastSent.map({ Date().timeIntervalSince($0) >= 3 }) ?? true {
			reported = true
			lastSent = Date()
			send(true)
		}
	}

	func stop() {
		idle?.cancel()
		guard reported else { return }
		reported = false
		lastSent = nil
		send(false)
	}

	/// A message went: the nolune stops showing them typing itself.
	func sent() {
		idle?.cancel()
		reported = false
		lastSent = nil
	}

	private func send(_ typing: Bool) {
		let previous = queue
		let report = self.report
		queue = Task {
			await previous?.value
			await report(typing)
		}
	}
}

/// Cancels its task when given another, and when whatever kept it goes.
final class Cancelling {
	var task: Task<Void, Never>? {
		didSet { oldValue?.cancel() }
	}

	deinit {
		task?.cancel()
	}
}
