import SwiftUI

/**
 * People & profile, natively (#137): the profile's name, its assistant's avatar and soul, and its
 * members, each with whose note in memory is theirs. Adding someone memory may know already asks
 * which note is theirs first, as the web does. A member leaves by removing themselves.
 */
struct ProfileView: View {
	@Environment(\.palette) private var palette
	@ObservedObject var family: Family
	let slug: String
	/// The profile is gone for this person: they left it, or deleted it.
	let left: () -> Void
	@State private var settings: ProfileSettings?
	@State private var name = ""
	@State private var soul = ""
	@State private var choosing: Choosing?
	@State private var removing: ProfileSettings.Member?
	@State private var deleting = false
	@State private var message: String?
	@State private var problem: String?

	/// Whose note to choose: someone being added, or a member's.
	private struct Choosing: Identifiable {
		let id = UUID()
		let who: String
		let member: String?
		let candidates: [PersonNote]
	}

	private var client: Client { family.client }

	var body: some View {
		Form {
			rows
				.listRowBackground(palette.muted)
		}
		.pageBackground(palette)
		.navigationTitle("People & profile")
		.refreshable { await load() }
		.task { await load() }
		.sheet(item: $choosing) { choice in
			NoteChooser(who: choice.who, linking: choice.member != nil, candidates: choice.candidates, current: settings?.members.first { $0.id == choice.member }?.note) { note in
				if let member = choice.member {
					act { try await client.linkNote(slug, member: member, note: note) }
				} else {
					add(choice.who, note: note)
				}
			}
		}
		.confirmationDialog(
			"Remove \(removing?.name ?? "")?",
			isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }),
			titleVisibility: .visible,
			presenting: removing
		) { member in
			Button("Remove", role: .destructive) { remove(member) }
		}
		.confirmationDialog("Delete \"\(settings?.name ?? "")\"?", isPresented: $deleting, titleVisibility: .visible) {
			Button("Delete", role: .destructive) {
				Task {
					do {
						try await client.deleteProfile(slug)
						left()
					} catch {
						problem = error.localizedDescription
					}
				}
			}
		} message: {
			Text("All its chats are deleted for everyone. The folder is moved to ~/.nolune/trash.")
		}
		.alert(problem ?? "", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })) {
			Button("OK", role: .cancel) {}
		}
	}

	/// The rows, apart from `body`, which the compiler then type-checks in time.
	@ViewBuilder private var rows: some View {
		if let settings {
			Section {
				TextField("Profile name", text: $name)
					.submitLabel(.done)
					.onSubmit(rename)
			} header: {
				Text("Name")
			} footer: {
				if let message { Text(verbatim: message) }
			}
			Section {
				AvatarPicker(selected: settings.avatar) { avatar in
					act { try await client.setAvatar(slug, avatar) }
				}
			} header: {
				Text("Avatar")
			} footer: {
				Text("How nolune looks in this profile's chats. Everyone here sees the same one, and nolune can change it when asked.")
			}
			Section {
				TextField("You're warm and a little playful, and you keep answers short. With the kids you explain things simply and never talk down to them. When you don't know something, you say so.", text: $soul, axis: .vertical)
					.lineLimit(4...16)
				if soul != settings.soul {
					HStack {
						Text(verbatim: "\(soul.count) / \(settings.maxSoul)")
							.font(.caption.monospacedDigit())
							.foregroundStyle(soul.count > settings.maxSoul ? Palette.plain.destructive : Color.secondary)
						Spacer()
						Button("Cancel") { soul = settings.soul }
							.buttonStyle(.borderless)
						Button("Save") { act { try await client.writeSoul(slug, soul) } }
							.buttonStyle(.borderless)
							.disabled(soul.count > settings.maxSoul)
					}
				}
			} header: {
				Text("Soul")
			} footer: {
				Text("Who nolune is for \(settings.name): its character, what it cares about, how it talks. Every chat starts with it, and nolune changes it too when you ask it to be different.")
			}
			Section {
				ForEach(settings.members) { member in
					memberRow(member)
				}
				if settings.others.isEmpty {
					Text("Everyone is already a member.")
						.foregroundStyle(.secondary)
				} else {
					Menu {
						ForEach(settings.others, id: \.self) { other in
							Button(other) { add(other) }
						}
					} label: {
						Label("Choose someone to add", systemImage: "person.badge.plus")
					}
				}
			} header: {
				Text("Members")
			} footer: {
				Text("Everyone here sees and writes in the same chats, and can ask nolune for anything.")
			}
			Section {
				Button("Delete this profile", role: .destructive) { deleting = true }
			} footer: {
				Text("Deletes all its chats for everyone. The folder is moved to ~/.nolune/trash.")
			}
		} else if problem == nil {
			ProgressView()
				.frame(maxWidth: .infinity)
		}
	}

	private func memberRow(_ member: ProfileSettings.Member) -> some View {
		HStack(spacing: 10) {
			PersonPicture(url: picture(member.id), name: member.name)
				.frame(width: 32, height: 32)
			VStack(alignment: .leading, spacing: 2) {
				Text(verbatim: member.name)
				Group {
					if let note = member.note {
						Text(member.exists ? String(localized: "Note: \(note)") : String(localized: "Note: \(note), started when there's something to write"))
					} else if !member.candidates.isEmpty {
						Text("nolune may have a note about them already")
					}
				}
				.font(.caption)
				.foregroundStyle(.secondary)
			}
			Spacer()
			if !member.candidates.isEmpty {
				Button(member.note == nil ? "Choose note" : "Change note") {
					choosing = Choosing(who: member.name, member: member.id, candidates: member.candidates)
				}
				.font(.caption)
				.buttonStyle(.bordered)
			}
		}
		.swipeActions {
			Button(role: .destructive) {
				removing = member
			} label: {
				Label("Remove", systemImage: "person.badge.minus")
			}
		}
		.contextMenu {
			Button(role: .destructive) {
				removing = member
			} label: {
				Label("Remove", systemImage: "person.badge.minus")
			}
		}
	}

	private func picture(_ person: String) -> URL? {
		guard let path = family.profile?.members.first(where: { $0.id == person })?.picture else { return nil }
		return URL(string: path, relativeTo: client.origin)?.absoluteURL
	}

	// MARK: Doing

	private func load() async {
		do {
			let loaded = try await client.profileSettings(slug)
			settings = loaded
			name = loaded.name
			soul = loaded.soul
		} catch {
			// The screen went away while it loaded: it loads again when it's back.
			if !error.isCancellation { problem = error.localizedDescription }
		}
	}

	private func rename() {
		let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
		guard !trimmed.isEmpty, trimmed != settings?.name else { return }
		act { try await client.renameProfile(slug, to: trimmed) }
	}

	private func add(_ who: String, note: String? = nil) {
		Task {
			do {
				let added = try await client.addMember(slug, who, note: note)
				if let choose = added.choose {
					choosing = Choosing(who: choose.who, member: nil, candidates: choose.candidates)
					return
				}
				message = added.message
				Haptics.success()
				await load()
				await family.reload()
			} catch {
				problem = error.localizedDescription
			}
		}
	}

	private func remove(_ member: ProfileSettings.Member) {
		Task {
			do {
				try await client.removeMember(slug, member.id)
				if member.id == family.me?.id {
					left()
					return
				}
				await load()
				await family.reload()
			} catch {
				problem = error.localizedDescription
			}
		}
	}

	private func act(_ work: @escaping () async throws -> Void) {
		Task {
			do {
				try await work()
				Haptics.success()
				await load()
				await family.reload()
			} catch {
				problem = error.localizedDescription
				Haptics.failure()
			}
		}
	}
}

/// The profile's assistant: one of nolune's avatars.
private struct AvatarPicker: View {
	let selected: String
	let pick: (String) -> Void

	private static let avatars = ["probe", "campfire", "lantern", "planet", "quantum", "comet", "moon", "satellite"]

	var body: some View {
		LazyVGrid(columns: Array(repeating: GridItem(.flexible()), count: 4), spacing: 12) {
			ForEach(Self.avatars, id: \.self) { avatar in
				Button {
					pick(avatar)
				} label: {
					AvatarView(avatar: avatar)
						.frame(width: 44, height: 44)
						.padding(8)
						.background(
							Circle()
								.strokeBorder(avatar == selected ? Palette.plain.primary : Color.clear, lineWidth: 2)
						)
				}
				.buttonStyle(.plain)
				.accessibilityLabel(Text(verbatim: avatar))
				.accessibilityAddTraits(avatar == selected ? .isSelected : [])
			}
		}
		.padding(.vertical, 6)
	}
}

/// Which people note is someone's, with a look at what each says; or a new one.
private struct NoteChooser: View {
	@Environment(\.palette) private var palette
	let who: String
	let linking: Bool
	let candidates: [PersonNote]
	let current: String?
	let choose: (String) -> Void
	@Environment(\.dismiss) private var dismiss
	@State private var picked = "new"

	var body: some View {
		NavigationStack {
			List {
				rows
					.listRowBackground(palette.muted)
			}
			.pageBackground(palette)
			.navigationTitle(linking ? String(localized: "Which note is about \(who)?") : String(localized: "Does nolune know \(who) already?"))
			.navigationBarTitleDisplayMode(.inline)
			.toolbar {
				ToolbarItem(placement: .cancellationAction) {
					Button("Cancel") { dismiss() }
				}
				ToolbarItem(placement: .confirmationAction) {
					Button(linking ? String(localized: "Link") : String(localized: "Add \(who)")) {
						choose(picked)
						dismiss()
					}
				}
			}
		}
		.onAppear { picked = current ?? candidates.first?.path ?? "new" }
	}

	/// The rows, apart from `body`, which the compiler then type-checks in time.
	@ViewBuilder private var rows: some View {
		Section {
			ForEach(candidates) { note in
				option(note.path) {
					VStack(alignment: .leading, spacing: 3) {
						HStack {
							Text(verbatim: note.title ?? note.path)
								.font(.body.weight(.medium))
							if note.path == current {
								Text("now")
									.font(.caption)
									.foregroundStyle(.secondary)
							}
						}
						Text(verbatim: note.path)
							.font(.caption.monospaced())
							.foregroundStyle(.secondary)
						if !note.aliases.isEmpty {
							Text("Also called: \(note.aliases.joined(separator: ", "))")
								.font(.caption)
								.foregroundStyle(.secondary)
						}
						ForEach(note.facts.prefix(3), id: \.self) { fact in
							Text(verbatim: "• \(fact)")
								.font(.caption)
								.foregroundStyle(.secondary)
						}
					}
				}
			}
			option("new") {
				Text("Someone else: start a new note")
			}
		} footer: {
			if picked != "new" {
				Text("\(who) will be able to read everything in this profile's memory, this note too. Check that it holds nothing meant to be kept from them, like a surprise.")
			}
		}
	}

	private func option<Content: View>(_ value: String, @ViewBuilder label: () -> Content) -> some View {
		Button {
			picked = value
		} label: {
			HStack(alignment: .top) {
				label()
					.foregroundStyle(Palette.plain.foreground)
				Spacer()
				if picked == value {
					Image(systemName: "checkmark")
						.foregroundStyle(Palette.plain.primary)
				}
			}
		}
	}
}
