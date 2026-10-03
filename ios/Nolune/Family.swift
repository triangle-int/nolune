import Combine
import Foundation
import UIKit

/**
 * The family's nolune as the native screens show it: who is signed in, their profiles, the open
 * profile's chats and folders, the chats nolune is working in, and the bell. It keeps them
 * current from the gateway's event streams, as the web sidebar does (`/api/events` for profiles
 * and the bell, the profile's `/running` for its chats), and does what the screens ask.
 */
@MainActor
final class Family: ObservableObject {
	let client: Client
	@Published private(set) var me: Me?
	@Published private(set) var profiles: [Profile] = []
	/// The open profile, kept for the next launch.
	@Published private(set) var slug: String?
	/// The open profile's chats, most recently active first, as many pages as were loaded.
	@Published private(set) var chats: [ChatSummary] = []
	/// Where the next page of chats starts; nil when they're all here.
	@Published private(set) var more: String?
	@Published private(set) var folders: [Folder] = []
	/// The chats nolune is working in right now, in the open profile.
	@Published private(set) var running: Set<String> = []
	@Published private(set) var bell = Bell.empty
	/// Loaded once, at least.
	@Published private(set) var loaded = false
	/// What went wrong with something the person did, to show them.
	@Published var problem: String?

	private let defaults: UserDefaults
	private var streams: [Task<Void, Never>] = []
	private var runningStream: Task<Void, Never>?
	private var subscriptions: Set<AnyCancellable> = []
	private var registered: String?

	init(client: Client, defaults: UserDefaults = .standard) {
		self.client = client
		self.defaults = defaults
	}

	var profile: Profile? { profiles.first { $0.slug == slug } }

	private var profileKey: String { "profile \(client.origin.absoluteString)" }

	// MARK: Loading

	/// Loads everything, then keeps it current until `stop`.
	func start() async {
		await reload()
		listen()
		// Notifications on this iPhone, for whoever signed in natively (Push.swift).
		Push.start()
		AppModel.shared.$deviceToken
			.compactMap { $0 }
			.removeDuplicates()
			.sink { [weak self] token in self?.register(token) }
			.store(in: &subscriptions)
		AppModel.shared.$activations
			.dropFirst()
			.sink { [weak self] _ in self?.becameActive() }
			.store(in: &subscriptions)
	}

	func stop() {
		streams.forEach { $0.cancel() }
		streams = []
		runningStream?.cancel()
		runningStream = nil
		subscriptions = []
	}

	func reload() async {
		do {
			let client = self.client
			async let me = client.me()
			async let profiles = client.profiles()
			async let bell = client.bell()
			(self.me, self.profiles, self.bell) = try await (me, profiles, bell)
			let saved = slug ?? defaults.string(forKey: profileKey)
			let pick = self.profiles.first { $0.slug == saved } ?? self.profiles.first
			if pick?.slug != slug {
				slug = pick?.slug
				listenToRunning()
			}
			await reloadProfile()
			loaded = true
		} catch {
			problem = error.localizedDescription
		}
	}

	/// The open profile's first page of chats, and its folders.
	func reloadProfile() async {
		guard let slug else {
			(chats, more, folders) = ([], nil, [])
			return
		}
		do {
			let client = self.client
			async let page = client.chats(slug)
			async let folders = client.folders(slug)
			let (first, list) = try await (page, folders)
			guard slug == self.slug else { return }
			chats = first.chats
			more = first.next
			self.folders = list
			running = Set(first.chats.filter(\.running).map(\.id)).union(running)
		} catch {
			problem = error.localizedDescription
		}
	}

	/// The next page of chats, as the list scrolls to its end.
	func loadMore() async {
		guard let slug, let after = more else { return }
		do {
			let page = try await client.chats(slug, after: after)
			guard slug == self.slug, after == more else { return }
			let known = Set(chats.map(\.id))
			chats += page.chats.filter { !known.contains($0.id) }
			more = page.next
		} catch {
			problem = error.localizedDescription
		}
	}

	/// Every page of chats, to search through them all.
	func loadAll() async {
		while let before = more {
			await loadMore()
			if more == before { return }
		}
	}

	func reloadBell() async {
		if let bell = try? await client.bell() { self.bell = bell }
	}

	/// Opens another of the person's profiles.
	func open(profile: String) async {
		guard profile != slug, profiles.contains(where: { $0.slug == profile }) else { return }
		slug = profile
		defaults.set(profile, forKey: profileKey)
		(chats, more, folders, running) = ([], nil, [], [])
		listenToRunning()
		await reloadProfile()
	}

	// MARK: Keeping current

	/**
	 * `/api/events` says when a profile changed (a name, an avatar, a member's picture) or has a
	 * new notification; `/running` lists the chats nolune works in whenever that changes, and a
	 * chat that starts or stops has usually changed otherwise too (a title, its place in the list).
	 * Each comes back after it drops, sooner while the app is in front.
	 */
	private func listen() {
		streams.forEach { $0.cancel() }
		streams = [
			Task { [weak self] in
				await self?.follow("api/events") { family, data in
					struct Event: Decodable { let type: String }
					guard let event = try? JSONDecoder().decode(Event.self, from: data) else { return }
					switch event.type {
					case "profiles": await family.reloadProfiles()
					case "notifications": await family.reloadBell()
					default: break
					}
				}
			}
		]
		listenToRunning()
	}

	private func listenToRunning() {
		runningStream?.cancel()
		guard let slug else { return }
		runningStream = Task { [weak self] in
			await self?.follow("api/p/\(slug)/running") { family, data in
				struct Running: Decodable { let running: [String] }
				guard let list = try? JSONDecoder().decode(Running.self, from: data), slug == family.slug else { return }
				let now = Set(list.running)
				guard now != family.running else { return }
				family.running = now
				await family.reloadProfile()
			}
		}
	}

	/// Reads a stream until the task is cancelled, connecting again each time it ends.
	private func follow(_ path: String, _ handle: @escaping @MainActor (Family, Data) async -> Void) async {
		var wait: UInt64 = 1
		while !Task.isCancelled {
			do {
				for try await data in client.events(path) {
					wait = 1
					await handle(self, data)
				}
			} catch {
				// Dropped: the network, or the computer went to sleep. It's tried again.
			}
			guard !Task.isCancelled else { return }
			try? await Task.sleep(nanoseconds: wait * 1_000_000_000)
			wait = min(wait * 2, 30)
		}
	}

	private func reloadProfiles() async {
		guard let profiles = try? await client.profiles() else { return }
		self.profiles = profiles
		if let me = try? await client.me() { self.me = me }
		if let slug, !profiles.contains(where: { $0.slug == slug }) {
			// Left the profile, or it was deleted.
			self.slug = profiles.first?.slug
			listenToRunning()
			await reloadProfile()
		}
	}

	/// Back to the front: the streams may have dropped while the app was away.
	private func becameActive() {
		Task {
			await reload()
			listen()
		}
	}

	/// The token Apple gave the app, to this session, so its notifications come here.
	private func register(_ token: String) {
		guard registered != token else { return }
		registered = token
		Task {
			do {
				try await client.registerPush(token, sandbox: Push.sandbox)
			} catch {
				if registered == token { registered = nil }
			}
		}
	}

	// MARK: Chats

	/// The person's own name, as everyone in their profiles sees it.
	func rename(me name: String) async -> String? {
		do {
			let kept = try await client.renameMe(to: name)
			me = try? await client.me()
			Haptics.success()
			return kept
		} catch {
			problem = error.localizedDescription
			Haptics.failure()
			return nil
		}
	}

	/// Runs something the person did, saying what went wrong; reloads when it did.
	private func act(_ work: () async throws -> Void) async -> Bool {
		do {
			try await work()
			Haptics.success()
			return true
		} catch {
			problem = error.localizedDescription
			Haptics.failure()
			await reloadProfile()
			return false
		}
	}

	@discardableResult
	func rename(_ chat: ChatSummary, to title: String) async -> Bool {
		await act {
			let kept = try await client.renameChat(chat.id, to: title)
			if let i = chats.firstIndex(where: { $0.id == chat.id }) { chats[i].title = kept }
		}
	}

	@discardableResult
	func delete(_ chat: ChatSummary) async -> Bool {
		chats.removeAll { $0.id == chat.id }
		return await act { try await client.deleteChat(chat.id) }
	}

	@discardableResult
	func move(_ chat: ChatSummary, to folder: String?) async -> Bool {
		if let i = chats.firstIndex(where: { $0.id == chat.id }) { chats[i].folderId = folder }
		return await act { try await client.moveChat(chat.id, to: folder) }
	}

	/// A chat by its id, among those loaded.
	func chat(_ id: String) -> ChatSummary? {
		chats.first { $0.id == id }
	}

	// MARK: Folders

	/// Makes a folder, and moves a chat into it if one was given.
	@discardableResult
	func createFolder(named name: String, moving chat: ChatSummary? = nil) async -> Folder? {
		guard let slug else { return nil }
		var made: Folder?
		_ = await act {
			let folder = try await client.createFolder(slug, named: name)
			folders.append(folder)
			made = folder
			if let chat {
				try await client.moveChat(chat.id, to: folder.id)
				if let i = chats.firstIndex(where: { $0.id == chat.id }) { chats[i].folderId = folder.id }
			}
		}
		return made
	}

	@discardableResult
	func rename(_ folder: Folder, to name: String) async -> Bool {
		guard let slug else { return false }
		return await act {
			let kept = try await client.renameFolder(slug, folder.id, to: name)
			if let i = folders.firstIndex(where: { $0.id == folder.id }) { folders[i] = kept }
		}
	}

	@discardableResult
	func delete(_ folder: Folder) async -> Bool {
		guard let slug else { return false }
		folders.removeAll { $0.id == folder.id }
		for i in chats.indices where chats[i].folderId == folder.id { chats[i].folderId = nil }
		return await act { try await client.deleteFolder(slug, folder.id) }
	}

	// MARK: The bell

	/// Opening the bell: what's in it now counts as seen.
	func markBellSeen() async {
		guard bell.unseen > 0 else { return }
		let now = (bell.items.map(\.createdAt).max() ?? 0)
		bell.seenAt = max(bell.seenAt, now)
		try? await client.markBellSeen()
	}

	@discardableResult
	func dismiss(_ item: Bell.Item) async -> Bool {
		bell.items.removeAll { $0.id == item.id }
		return await act { try await client.dismiss(item.id) }
	}

	@discardableResult
	func clearBell() async -> Bool {
		bell.items = []
		return await act { try await client.clearBell() }
	}

	/// Continues a notification as a chat, opening the profile it's in.
	func continueInChat(_ item: Bell.Item) async -> ChatPlace? {
		do {
			let place = try await client.continueInChat(item.id)
			if place.slug != slug { await open(profile: place.slug) } else { await reloadProfile() }
			await reloadBell()
			return place
		} catch {
			problem = error.localizedDescription
			return nil
		}
	}
}

/// The taps the app's actions answer with.
@MainActor
enum Haptics {
	static func success() {
		UINotificationFeedbackGenerator().notificationOccurred(.success)
	}

	static func failure() {
		UINotificationFeedbackGenerator().notificationOccurred(.error)
	}

	static func tap() {
		UIImpactFeedbackGenerator(style: .light).impactOccurred()
	}
}
