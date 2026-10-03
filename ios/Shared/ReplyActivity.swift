#if canImport(ActivityKit)
import ActivityKit
import Foundation

/**
 * A reply nolune is working on, as a Live Activity on the lock screen and in the Dynamic Island
 * (#138). The app starts it when its person sends a message; the gateway updates it through the
 * relay (packages/core/src/live-activities.ts), whose `state` is this `ContentState`, field for
 * field, and the app while it's open.
 */
@available(iOS 16.1, *)
struct ReplyActivity: ActivityAttributes {
	struct ContentState: Codable, Hashable {
		/// The chat's title, empty until it has one.
		var title: String
		/// What nolune does now, in the model's words, or nothing; once it's done, the reply's first words.
		var step: String
		var running: Bool
	}

	let chat: String
	let slug: String
	/// The profile's name and its assistant.
	let profile: String
	let avatar: String

	/// What the app opens for it.
	var path: String { "/p/\(slug)/c/\(chat)" }
}
#endif
