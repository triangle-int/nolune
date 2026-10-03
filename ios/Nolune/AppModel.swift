import Foundation

/// Which nolune the app opens, and what it opens there next.
@MainActor
final class AppModel: ObservableObject {
	static let shared = AppModel()

	private static let originKey = "origin"
	private static let previousKey = "previous"
	private static let nativeKey = "native"
	private static let openKey = "open"

	/// The family's nolune, as the person connected to it; nil until they have (ConnectView).
	@Published private(set) var origin: URL?
	/// The one they left for another, to offer going back to.
	@Published private(set) var previous: URL?
	/// A page to open there next: a link's, like an invite, or a tapped notification's.
	@Published var pending: String?
	/// The token Apple gave the app for notifications, in hex, once it has (Push.swift).
	@Published var deviceToken: String?
	/// Counts the times the app came back to the front, for the page to check it's still there.
	@Published private(set) var activations = 0
	/**
	 * Whether the native screens are on (NativeController.swift), as they are unless launched with
	 * `-native NO`, which opens today's web app instead.
	 */
	let native: Bool

	private let defaults: UserDefaults

	/// The app's is `shared`; tests make their own, with defaults of their own.
	init(defaults: UserDefaults = .standard) {
		self.defaults = defaults
		origin = defaults.string(forKey: Self.originKey).flatMap(URL.init(string:))
		previous = defaults.string(forKey: Self.previousKey).flatMap(URL.init(string:))
		native = defaults.object(forKey: Self.nativeKey) == nil || defaults.bool(forKey: Self.nativeKey)
		// A page to open first, as `-open /p/smiths/c/<id>` (CI's screens).
		pending = defaults.string(forKey: Self.openKey).flatMap { Address.isPath($0) ? $0 : nil }
	}

	func connect(to address: Address) {
		pending = address.path
		origin = address.origin
		defaults.set(address.origin.absoluteString, forKey: Self.originKey)
	}

	/// Back to the first screen, to connect to another nolune.
	func disconnect() {
		guard let origin else { return }
		previous = origin
		pending = nil
		self.origin = nil
		defaults.set(origin.absoluteString, forKey: Self.previousKey)
		defaults.removeObject(forKey: Self.originKey)
	}

	/// The page to open first, which is then no longer pending.
	func takePending() -> String? {
		defer { pending = nil }
		return pending
	}

	/// A tapped notification: its page, when it came from the nolune the app has open.
	func open(path: String?, from source: URL?) {
		guard let origin, let path, Address.isPath(path) else { return }
		if let source, !Address.sameOrigin(source, origin) { return }
		pending = path
	}

	func becameActive() {
		activations += 1
	}
}
