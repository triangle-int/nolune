import ActivityKit
import Foundation
import UIKit

/**
 * Live Activities (#138): a reply nolune writes, on the lock screen and in the Dynamic Island
 * (ReplyActivity in Shared/, drawn by NoluneWidgets). One starts when the person sends a message
 * or runs an automation, and its token from Apple goes to the nolune (`POST /api/c/<id>/activity`),
 * which sends what nolune does through the relay until the reply is done (live-activities.ts in
 * core). While the chat is open here, the app keeps it current itself, and ends it as the reply is
 * done: right away when the person is looking, or after a while on the lock screen.
 */
@MainActor
enum LiveActivities {
	/// The most of the reply's first words an ended activity shows, as the gateway's.
	private static let maxStep = 160
	/// A reply that hasn't started this long after the activity did isn't coming, as the gateway has it.
	private static let startWithin: TimeInterval = 60
	/// An ended activity stays on the lock screen this long, as the gateway's.
	private static let linger: TimeInterval = 15 * 60

	/**
	 * The activities whose chat was seen with the message unanswered or nolune working since they
	 * started: once neither, the reply is done.
	 */
	private static var started: Set<String> = []
	/// When the app asked for each, by its id: activities don't say.
	private static var requested: [String: Date] = [:]

	static func start(chat: String, title: String, profile: Profile, client: Client) {
		guard #available(iOS 16.2, *), ActivityAuthorizationInfo().areActivitiesEnabled else { return }
		// One for a chat: a message sent while nolune works goes on in the same.
		guard activity(for: chat) == nil else { return }
		let attributes = ReplyActivity(chat: chat, slug: profile.slug, profile: profile.name, avatar: profile.avatar)
		let state = ReplyActivity.ContentState(title: title, step: "", running: true)
		do {
			let activity = try Activity.request(
				attributes: attributes,
				content: ActivityContent(state: state, staleDate: nil),
				pushType: .token
			)
			follow(activity, client: client)
		} catch {
			print("nolune: no Live Activity: \(error.localizedDescription)")
		}
	}

	/**
	 * The chat as it is now, open here: what its activity shows, and its end once the reply is done.
	 * A chat without one is left alone.
	 */
	static func update(chat: String, state: ChatState) {
		guard #available(iOS 16.2, *), let activity = activity(for: chat) else { return }
		if state.running || state.lastReply == nil { started.insert(activity.id) }
		let content = ReplyActivity.ContentState(
			title: state.title,
			step: state.running ? state.runningCommand ?? "" : firstWords(state.lastReply?.text ?? ""),
			running: state.running
		)
		let done = !state.running && started.contains(activity.id) && (state.lastReply != nil || state.error != nil)
		// Or it never started (a message queued behind another), or did before the app saw it.
		let gone = !state.running && Date().timeIntervalSince(requested[activity.id] ?? .distantPast) > startWithin
		if done || gone {
			started.remove(activity.id)
			let looking = UIApplication.shared.applicationState == .active
			Task {
				await activity.end(
					ActivityContent(state: content, staleDate: nil),
					dismissalPolicy: looking ? .immediate : .after(Date().addingTimeInterval(linger))
				)
			}
			return
		}
		guard content != activity.content.state else { return }
		Task { await activity.update(ActivityContent(state: content, staleDate: nil)) }
	}

	@available(iOS 16.2, *)
	private static func activity(for chat: String) -> Activity<ReplyActivity>? {
		Activity<ReplyActivity>.activities.first { $0.attributes.chat == chat && $0.activityState == .active }
	}

	/// Gives the nolune the activity's token, each time Apple changes it, and forgets it once it's over.
	@available(iOS 16.2, *)
	private static func follow(_ activity: Activity<ReplyActivity>, client: Client) {
		let chat = activity.attributes.chat
		requested[activity.id] = Date()
		Task {
			for await data in activity.pushTokenUpdates {
				try? await client.followActivity(chat, token: hex(data), sandbox: Push.sandbox)
			}
		}
		Task {
			for await state in activity.activityStateUpdates where state == .ended || state == .dismissed {
				requested[activity.id] = nil
				started.remove(activity.id)
				if let token = activity.pushToken { await client.forgetActivity(chat, token: hex(token)) }
				break
			}
		}
	}

	private static func hex(_ data: Data) -> String {
		data.map { String(format: "%02x", $0) }.joined()
	}

	/// The reply's first words, as text on a line.
	private static func firstWords(_ markdown: String) -> String {
		let words = Markdown.plain(markdown)
			.split(whereSeparator: \.isWhitespace)
			.joined(separator: " ")
		guard words.count > maxStep else { return words }
		return String(words.prefix(maxStep - 1)).trimmingCharacters(in: .whitespaces) + "…"
	}
}
