import SwiftUI

/**
 * A new chat (#136): a greeting for the time of day, the person's chips (made from the profile's
 * memory, or the general ones), and the composer, with the folder it starts in, its model and
 * reasoning, and how its commands run. Sending starts the chat with that message, then opens it.
 */
struct NewChatView: View {
	@ObservedObject var family: Family
	/// Opens the chat it started.
	let started: (String) -> Void
	@StateObject private var attachments: Attachments
	@State private var text = ""
	@State private var folder: String?
	@State private var preset: String?
	@State private var effort = UserDefaults.standard.string(forKey: NewChatView.effortKey) ?? "medium"
	@State private var commands: String?
	@State private var sending = false
	@State private var suggestions: [Suggestion] = []
	@State private var greeting = ""
	@State private var technical = false

	/// The reasoning level new chats start with, as the person last picked it on this device.
	private static let effortKey = "effort"

	init(family: Family, folder: String? = nil, started: @escaping (String) -> Void) {
		_family = ObservedObject(wrappedValue: family)
		_folder = State(initialValue: folder)
		self.started = started
		_attachments = StateObject(wrappedValue: Attachments(client: family.client, slug: family.slug ?? ""))
	}

	private var options: ChatOptions? { family.options }
	private var chosenPreset: ChatOptions.Preset? {
		let id = preset ?? options?.defaultPresetId
		return options?.presets.first { $0.id == id } ?? options?.presets.first
	}

	var body: some View {
		ScrollView {
			VStack(spacing: 18) {
				AvatarView(avatar: family.profile?.avatar ?? "probe")
					.frame(width: 56, height: 56)
				Text(verbatim: greeting)
					.font(.title2.weight(.semibold))
					.multilineTextAlignment(.center)
			}
			.padding(.horizontal, 24)
			.padding(.top, 80)
			.frame(maxWidth: .infinity)
		}
		.scrollDismissesKeyboard(.interactively)
		.safeAreaInset(edge: .bottom, spacing: 0) {
			VStack(alignment: .leading, spacing: 8) {
				if text.isEmpty, !suggestions.isEmpty {
					ScrollView(.horizontal, showsIndicators: false) {
						HStack(spacing: 8) {
							ForEach(suggestions, id: \.self) { suggestion in
								Button {
									text = suggestion.text
								} label: {
									HStack(spacing: 6) {
										LucideIcon(node: suggestion.icon)
										Text(verbatim: suggestion.label)
											.lineLimit(1)
									}
									.font(.subheadline)
									.padding(.horizontal, 12)
									.padding(.vertical, 8)
									.background(Capsule().fill(Color(.secondarySystemBackground)))
								}
								.buttonStyle(.plain)
							}
						}
						.padding(.horizontal, 4)
					}
				}
				ComposerBar(
					text: $text,
					attachments: attachments,
					placeholder: String(localized: "Ask nolune"),
					sending: sending,
					send: send
				) {
					FolderMenu(folders: family.folders, folder: $folder)
					if let chosen = chosenPreset {
						ModelMenu(
							options: options,
							preset: chosen.id,
							presetName: chosen.name,
							effort: effort,
							technical: technical,
							choosePreset: { preset = $0.id },
							chooseEffort: { level in
								effort = level
								UserDefaults.standard.set(level, forKey: Self.effortKey)
							}
						)
					}
					if let options {
						CommandMenu(mode: commands ?? options.commandMode, fallback: options.commandMode, admin: family.me?.isAdmin == true) { mode in
							commands = mode == options.commandMode ? nil : mode
						}
					}
				}
				Text("nolune can make mistakes, and it can change files on this computer.")
					.font(.caption2)
					.foregroundStyle(.secondary)
					.frame(maxWidth: .infinity)
					.multilineTextAlignment(.center)
			}
			.padding(.horizontal, 12)
			.padding(.vertical, 6)
			.frame(maxWidth: 780)
			.frame(maxWidth: .infinity)
			.background(.bar)
		}
		.environment(\.noluneClient, family.client)
		.task {
			if greeting.isEmpty {
				let name = family.me?.name.split(separator: " ").first.map(String.init) ?? ""
				greeting = Self.greeting(name: name)
			}
			technical = await Preferences.read(family.client.origin).technical
			if family.options == nil { await family.loadOptions() }
			guard let options = family.options, let slug = family.slug else { return }
			suggestions = options.suggestions
			if options.suggestionsStale, let fresh = try? await family.client.suggestions(slug), !fresh.isEmpty {
				suggestions = fresh
			}
		}
	}

	private func send() {
		let message = text.trimmingCharacters(in: .whitespacesAndNewlines)
		sending = true
		Task {
			let chat = Client.NewChat(
				preset: chosenPreset?.id,
				effort: effort,
				folder: folder,
				text: message.isEmpty ? nil : message,
				uploads: attachments.ready,
				commands: commands
			)
			if let summary = await family.startChat(chat) {
				text = ""
				attachments.sent()
				started(summary.id)
			}
			sending = false
		}
	}

	/**
	 * One of the web's greetings (src/lib/greeting.ts): for the part of the day, morning from 5:00,
	 * afternoon from 12:00, evening from 18:00, night from 23:00, or for any time.
	 */
	static func greeting(name: String, hour: Int = Calendar.current.component(.hour, from: Date())) -> String {
		guard !name.isEmpty else { return String(localized: "What can I help with?") }
		let part: [String]
		switch hour {
		case 5..<12:
			part = [
				String(localized: "Good morning, \(name)!"),
				String(localized: "Good morning, \(name). Where do we start today?"),
				String(localized: "Coffee first, or straight to it, \(name)?"),
				String(localized: "Rise and shine, \(name)! What’s on today’s list?")
			]
		case 12..<18:
			part = [
				String(localized: "Good afternoon, \(name)!"),
				String(localized: "Good afternoon, \(name). How’s the day going?"),
				String(localized: "What can I take off your plate this afternoon, \(name)?"),
				String(localized: "Need a hand this afternoon, \(name)?")
			]
		case 18..<23:
			part = [
				String(localized: "Good evening, \(name)!"),
				String(localized: "Good evening, \(name). How was your day?"),
				String(localized: "Anything left for today, \(name)?"),
				String(localized: "What’s the plan for tonight, \(name)?")
			]
		default:
			part = [
				String(localized: "Up late, \(name)?"),
				String(localized: "Burning the midnight oil, \(name)?"),
				String(localized: "Can’t sleep, \(name)? I’m here."),
				String(localized: "It’s getting late, \(name). What do you need?")
			]
		}
		let anytime = [
			String(localized: "What can I help with, \(name)?"),
			String(localized: "What’s on your mind, \(name)?"),
			String(localized: "What are we working on, \(name)?"),
			String(localized: "Good to see you, \(name). What’s up?")
		]
		return (part + anytime).randomElement() ?? ""
	}
}
