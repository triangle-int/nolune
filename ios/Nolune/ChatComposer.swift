import SwiftUI

/**
 * A chat's composer (#136): the message box, with the chat's model and reasoning, and how its
 * commands run. Switching the model or reasoning in a chat with replies asks first, as on the web:
 * the next reply can't use the chat's cache. Under it, whether the chat is reconnecting, what went
 * wrong, or the disclaimer.
 */
struct ChatComposer: View {
	@ObservedObject var chat: Conversation
	@ObservedObject var family: Family
	/// Shows the newest message, as the person writes or sends.
	let toBottom: () -> Void
	@StateObject private var attachments: Attachments
	@State private var text = ""
	@State private var sending = false
	@State private var switching: Switching?
	@State private var technical = false

	/// A model or reasoning level to switch to, once the person agrees.
	private struct Switching: Identifiable {
		let id = UUID()
		var preset: ChatOptions.Preset?
		var effort: String?
	}

	init(chat: Conversation, family: Family, toBottom: @escaping () -> Void) {
		_chat = ObservedObject(wrappedValue: chat)
		_family = ObservedObject(wrappedValue: family)
		self.toBottom = toBottom
		_attachments = StateObject(wrappedValue: Attachments(client: family.client, slug: chat.slug))
	}

	var body: some View {
		let state = chat.state
		VStack(alignment: .leading, spacing: 6) {
			if let changes = state.toolChanges {
				ToolChangesBanner(changes: changes) { Task { await chat.reloadTools() } }
			}
			ComposerBar(
				text: $text,
				attachments: attachments,
				placeholder: state.running ? String(localized: "Add something while nolune works…") : String(localized: "Ask nolune"),
				running: state.running,
				sending: sending,
				send: send,
				stop: { Task { await chat.stopWork() } },
				focused: toBottom
			) {
				if let model = state.model {
					ModelMenu(
						options: family.options,
						preset: model.presetId,
						presetName: model.presetName,
						effort: model.effort,
						technical: technical,
						choosePreset: { preset in ask(Switching(preset: preset)) },
						chooseEffort: { effort in ask(Switching(effort: effort)) }
					)
				}
				CommandMenu(mode: state.commands.current, fallback: state.commands.fallback, admin: family.me?.isAdmin == true) { mode in
					Task { await chat.setCommands(mode) }
				}
			}
			Group {
				if chat.reconnecting {
					Text("Reconnecting…")
				} else if let problem = chat.problem {
					Text(verbatim: problem)
						.foregroundStyle(.red)
				} else {
					Text("nolune can make mistakes, and it can change files on this computer.")
				}
			}
			.font(.caption2)
			.foregroundStyle(.secondary)
			.frame(maxWidth: .infinity)
			.multilineTextAlignment(.center)
		}
		.padding(.horizontal, 12)
		.padding(.top, 6)
		.padding(.bottom, 6)
		.frame(maxWidth: 780)
		.frame(maxWidth: .infinity)
		.background(.bar)
		.onChange(of: text) { typed in chat.typed(typed) }
		.task {
			if family.options == nil { await family.loadOptions() }
			technical = await Preferences.read(family.client.origin).technical
		}
		.confirmationDialog(
			switchingTitle,
			isPresented: Binding(get: { switching != nil }, set: { if !$0 { switching = nil } }),
			titleVisibility: .visible,
			presenting: switching
		) { change in
			Button(change.preset != nil ? "Switch" : "Change") { apply(change) }
		} message: { change in
			if change.preset != nil {
				Text("nolune keeps this chat in a cache, so each reply only pays for what's new. Another model can't use it: the next reply reads the whole chat again, which takes longer and costs more.")
			} else {
				Text("nolune keeps this chat in a cache, so each reply only pays for what's new. Another reasoning level can't use it: the next reply reads the whole chat again, which takes longer and costs more.")
			}
		}
	}

	private var switchingTitle: String {
		if let preset = switching?.preset { return String(localized: "Switch to \(preset.name)?") }
		if let effort = switching?.effort { return String(localized: "Change reasoning to \(Effort.label(effort))?") }
		return ""
	}

	/// Asks first in a chat with replies, whose cache the switch loses.
	private func ask(_ change: Switching) {
		if change.preset?.id == chat.state.model?.presetId, change.effort == nil { return }
		if change.effort == chat.state.model?.effort, change.preset == nil { return }
		let replied = chat.state.entries.contains { if case .reply = $0 { return true } else { return false } }
		if replied { switching = change } else { apply(change) }
	}

	private func apply(_ change: Switching) {
		Task {
			if let preset = change.preset { await chat.setPreset(preset.id) }
			if let effort = change.effort { await chat.setEffort(effort) }
		}
	}

	private func send() {
		let message = text.trimmingCharacters(in: .whitespacesAndNewlines)
		let uploads = attachments.ready
		sending = true
		Task {
			if await chat.send(message, uploads: uploads) {
				text = ""
				attachments.sent()
				toBottom()
			}
			sending = false
		}
	}
}

// MARK: - Menus

/// The reasoning levels, as the web names them.
enum Effort {
	static let all = ["low", "medium", "high", "xhigh", "max"]

	static func label(_ level: String) -> String {
		switch level {
		case "low": return String(localized: "Low")
		case "medium": return String(localized: "Medium")
		case "high": return String(localized: "High")
		case "xhigh": return String(localized: "Extra high")
		case "max": return String(localized: "Max")
		default: return level
		}
	}

	static func hint(_ level: String) -> String {
		switch level {
		case "low": return String(localized: "Fastest answers")
		case "medium": return String(localized: "Good for most things")
		case "high": return String(localized: "Thinks longer on harder tasks")
		case "xhigh": return String(localized: "Takes its time")
		case "max": return String(localized: "Slowest, for the hardest problems")
		default: return ""
		}
	}
}

/// The model a chat runs on, when there's more than one, and how hard it thinks.
struct ModelMenu: View {
	let options: ChatOptions?
	let preset: String?
	let presetName: String
	let effort: String
	let technical: Bool
	let choosePreset: (ChatOptions.Preset) -> Void
	let chooseEffort: (String) -> Void

	var body: some View {
		Menu {
			if let options, options.presets.count > 1 {
				Section("Model") {
					ForEach(options.presets) { option in
						Button {
							choosePreset(option)
						} label: {
							if option.id == preset {
								Label(option.name, systemImage: "checkmark")
							} else {
								Text(verbatim: option.name)
							}
						}
					}
				}
			}
			Section("Reasoning") {
				ForEach(options?.efforts ?? Effort.all, id: \.self) { level in
					Button {
						chooseEffort(level)
					} label: {
						if level == effort {
							Image(systemName: "checkmark")
						}
						Text(verbatim: Effort.label(level))
						Text(verbatim: Effort.hint(level))
					}
				}
			}
		} label: {
			HStack(spacing: 4) {
				Text(verbatim: technical ? "\(presetName) · \(Effort.label(effort))" : Effort.label(effort))
					.lineLimit(1)
				Image(systemName: "chevron.up.chevron.down")
					.font(.caption2)
			}
			.font(.subheadline)
			.foregroundStyle(.secondary)
			.padding(.horizontal, 10)
			.frame(height: 34)
			.background(Capsule().strokeBorder(Color(.separator)))
		}
		.accessibilityLabel(Text("Model"))
		.accessibilityValue(Text(verbatim: "\(presetName), \(Effort.label(effort))"))
	}
}

/// How the chat's commands run: auto mode checks each first; only an admin turns that off.
struct CommandMenu: View {
	let mode: String
	let fallback: String
	let admin: Bool
	let choose: (String) -> Void

	private var locked: Bool { !admin && fallback != "unrestricted" }

	var body: some View {
		Menu {
			Section("Commands in this chat") {
				Button {
					choose("auto")
				} label: {
					if mode == "auto" { Image(systemName: "checkmark") }
					Text("Auto")
					Text("A model checks each command before it runs.")
				}
				Button {
					choose("unrestricted")
				} label: {
					if mode == "unrestricted" { Image(systemName: "checkmark") }
					Text("Unrestricted")
					Text("Commands run without a check. Not recommended.")
				}
				.disabled(locked)
			}
			if locked {
				Text("Only an admin can turn the checks off for a chat.")
			}
		} label: {
			Group {
				if mode == "unrestricted" {
					Label("Unrestricted", systemImage: "exclamationmark.shield")
						.font(.subheadline)
						.foregroundStyle(.orange)
						.padding(.horizontal, 10)
				} else {
					Image(systemName: "checkmark.shield")
						.foregroundStyle(.secondary)
						.frame(width: 34)
				}
			}
			.frame(height: 34)
			.background(Capsule().strokeBorder(Color(.separator)))
		}
		.accessibilityLabel(mode == "unrestricted" ? Text("Commands: unrestricted") : Text("Commands: auto mode"))
	}
}

/// The folder a new chat starts in.
struct FolderMenu: View {
	let folders: [Folder]
	@Binding var folder: String?

	var body: some View {
		Menu {
			Button {
				folder = nil
			} label: {
				if folder == nil {
					Label("No folder", systemImage: "checkmark")
				} else {
					Text("No folder")
				}
			}
			ForEach(folders) { option in
				Button {
					folder = option.id
				} label: {
					if folder == option.id {
						Label(option.name, systemImage: "checkmark")
					} else {
						Text(verbatim: option.name)
					}
				}
			}
		} label: {
			Group {
				if let name = folders.first(where: { $0.id == folder })?.name {
					Label(name, systemImage: "folder")
						.lineLimit(1)
						.font(.subheadline)
						.padding(.horizontal, 10)
				} else {
					Image(systemName: "folder")
						.frame(width: 34)
				}
			}
			.foregroundStyle(.secondary)
			.frame(height: 34)
			.background(Capsule().strokeBorder(Color(.separator)))
		}
		.accessibilityLabel(Text("Folder"))
	}
}

/// Skills and services that changed since the chat's tools were loaded, and loading them.
private struct ToolChangesBanner: View {
	let changes: ToolChanges
	let reload: () -> Void

	var body: some View {
		VStack(alignment: .leading, spacing: 6) {
			ForEach(lines, id: \.self) { line in
				Text(verbatim: line)
			}
			Button("Reload tools", action: reload)
				.font(.caption.weight(.semibold))
		}
		.font(.caption)
		.foregroundStyle(.secondary)
		.padding(10)
		.frame(maxWidth: .infinity, alignment: .leading)
		.background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color(.secondarySystemBackground)))
	}

	private var lines: [String] {
		func names(_ list: [String]) -> String? {
			list.isEmpty ? nil : ListFormatter.localizedString(byJoining: list)
		}
		var lines: [String] = []
		if let services = changes.services {
			if let added = names(services.added) { lines.append(String(localized: "Connected: \(added)")) }
			if let changed = names(services.changed) { lines.append(String(localized: "Tools updated: \(changed)")) }
			if let removed = names(services.removed) { lines.append(String(localized: "Disconnected: \(removed)")) }
		}
		if let skills = changes.skills {
			if let added = names(skills.added) { lines.append(String(localized: "New skills: \(added)")) }
			if let changed = names(skills.changed) { lines.append(String(localized: "Updated skills: \(changed)")) }
			if let removed = names(skills.removed) { lines.append(String(localized: "Skills no longer available: \(removed)")) }
		}
		return lines
	}
}
