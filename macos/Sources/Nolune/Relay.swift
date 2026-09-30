import Foundation

/**
 * nolune's relay (packages/relay): an address like https://smiths.nolune.family that works on any
 * phone or laptop, at home or away, with nothing to set up on the router. `nolune relay enable`
 * gets one and keeps it in config.json; the gateway connects to the relay when it starts, and the
 * address is nolune's from then on (Runtime.origin).
 */
@MainActor
final class RelaySetup: ObservableObject {
	/// Where families' addresses are, to show one before there is one.
	nonisolated static let domain = "nolune.family"

	/// The name asked for; empty for a random one, like cozy-otter-42.
	@Published var name = ""
	@Published private(set) var working = false
	@Published private(set) var problem: String?
	/// The address, once there is one.
	@Published private(set) var url: String?

	init() {
		if !Snapshot.active { url = Runtime.shared.config.relay?.url }
	}

	/// Gets an address from the relay. True once there is one; `problem` says why not otherwise.
	func enable() async -> Bool {
		guard !working else { return false }
		let name = RelaySetup.clean(self.name)
		if let invalid = RelaySetup.check(name) {
			problem = invalid
			return false
		}
		working = true
		problem = nil
		let output = await Runtime.shared.run(["relay", "enable"] + (name.isEmpty ? [] : ["--name", name]))
		working = false
		guard output.succeeded, let url = Runtime.shared.config.relay?.url else {
			problem = output.problem
			return false
		}
		self.url = url
		return true
	}

	/// For `--snapshot`.
	func pose(name: String, url: String? = nil) {
		self.name = name
		self.url = url
	}

	nonisolated static func clean(_ name: String) -> String {
		name.trimmingCharacters(in: .whitespaces).lowercased()
	}

	/// What's wrong with a name, as `isValidName` in packages/relay/src/protocol.ts sees it; nil
	/// when it will do. Empty will: the relay picks one.
	nonisolated static func check(_ name: String) -> String? {
		if name.isEmpty { return nil }
		let allowed = Set("abcdefghijklmnopqrstuvwxyz0123456789-")
		if !name.allSatisfy({ allowed.contains($0) }) { return "Use Latin letters, digits and dashes." }
		if name.count < 3 { return "A name has at least 3 letters." }
		if name.count > 32 { return "A name has at most 32 letters." }
		if name.hasPrefix("-") || name.hasSuffix("-") || name.contains("--") {
			return "Dashes go between letters, one at a time."
		}
		return nil
	}
}
