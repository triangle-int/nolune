import SwiftUI

/**
 * Memory, natively (#137): whether nolune learns from chats, what it saved from them lately (to
 * undo), the pinned Core note, the members' cards, and the profile's notes by category. A note
 * opens to read, edit, move (or merge) and forget, as the web page does.
 */
struct MemoryView: View {
	@Environment(\.palette) private var palette
	@ObservedObject var family: Family
	let slug: String
	/// Opens another screen: the person's own card, which is the web's for now.
	let open: (Destination) -> Void
	@State private var memory: MemoryOverview?
	@State private var problem: String?
	@State private var allChanges = false

	private var client: Client { family.client }

	var body: some View {
		List {
			Group {
				if let memory {
					Section {
						Toggle("Learn from chats", isOn: Binding(get: { memory.learnFromChats }, set: { learn($0) }))
					} footer: {
						Text("When a chat has been quiet for a couple of minutes, nolune reads it over and saves what's worth remembering. That's one short extra request to the chat's model each time. When this is off, nolune saves only what it thinks of while chatting.")
					}
					if !memory.recent.isEmpty {
						Section {
							ForEach(allChanges ? memory.recent : Array(memory.recent.prefix(4))) { change in
								ChangeRow(change: change, me: family.me?.id, title: title(of: change.note)) { await undo(change) }
							}
							if !allChanges, memory.recent.count > 4 {
								Button("Show all \(memory.recent.count)") { allChanges = true }
							}
						} header: {
							Text("Saved from chats")
						} footer: {
							Text("What nolune noted by itself after chats went quiet, in the last two weeks.")
						}
					}
					Section {
						noteRow(core(memory), pinned: true)
					} header: {
						Text("Core")
					}
					if !memory.cards.isEmpty {
						Section {
							ForEach(memory.cards) { card in
								cardRow(card)
							}
						} header: {
							Text("Cards")
						} footer: {
							Text("Each member's card says what they told nolune about themselves that they'd tell anyone. It goes with them into every profile they're in, and every chat starts with it. Only they change it.")
						}
					}
					ForEach(groups(memory)) { group in
						Section(group.title) {
							ForEach(group.notes) { note in noteRow(note) }
						}
					}
				} else if problem == nil {
					ProgressView()
						.frame(maxWidth: .infinity)
				}
			}
			.listRowBackground(palette.muted)
		}
		.pageBackground(palette)
		.navigationTitle("Memory")
		.refreshable { await load() }
		.task { await load() }
		.alert(problem ?? "", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })) {
			Button("OK", role: .cancel) {}
		}
	}

	// MARK: Rows

	private func noteRow(_ note: MemoryOverview.Note, pinned: Bool = false) -> some View {
		NavigationLink {
			NoteView(note: note, slug: slug, client: client, memory: memory, pinned: pinned) { await load() }
		} label: {
			VStack(alignment: .leading, spacing: 3) {
				HStack(spacing: 6) {
					if pinned { Image(systemName: "pin.fill").font(.caption).foregroundStyle(Palette.plain.warning) }
					Text(verbatim: pinned ? String(localized: "Core") : MemoryCategory.title(of: note))
						.font(.body.weight(.medium))
				}
				Text(subtitle(note, pinned: pinned))
					.font(.caption)
					.foregroundStyle(.secondary)
					.lineLimit(1)
			}
		}
	}

	private func cardRow(_ card: MemoryOverview.Card) -> some View {
		VStack(alignment: .leading, spacing: 6) {
			HStack {
				PersonPicture(url: picture(card.ownerId), name: card.owner)
					.frame(width: 24, height: 24)
				Text(card.mine ? String(localized: "Your card") : String(localized: "\(card.owner)'s card"))
					.font(.body.weight(.medium))
				Spacer()
				if card.mine {
					Button {
						open(.page(.card))
					} label: {
						Image(systemName: "pencil")
					}
					.buttonStyle(.borderless)
					.accessibilityLabel(Text("Edit your card"))
				}
			}
			if let file = card.file, !file.facts.isEmpty {
				MarkdownView(text: file.text)
					.font(.subheadline)
			} else {
				Text(card.mine ? String(localized: "Your card is empty so far. Tell nolune about yourself in any chat, like \"I'm vegetarian\", and it goes on your card, for all your profiles.") : String(localized: "\(card.owner)'s card is empty so far."))
					.font(.subheadline)
					.foregroundStyle(.secondary)
			}
		}
		.padding(.vertical, 4)
	}

	// MARK: Helpers

	/// The Core note, even before it has anything in it.
	private func core(_ memory: MemoryOverview) -> MemoryOverview.Note {
		memory.files.first { $0.path == memory.core.path }
			?? MemoryOverview.Note(path: memory.core.path, text: "", facts: [], updatedAt: 0)
	}

	private struct NoteGroup: Identifiable {
		let title: String
		let notes: [MemoryOverview.Note]

		var id: String { title }
	}

	/// The notes besides Core, by category, as the page orders them.
	private func groups(_ memory: MemoryOverview) -> [NoteGroup] {
		let notes = memory.files.filter { $0.path != memory.core.path }
		var groups: [NoteGroup] = []
		for category in MemoryCategory.allCases where category != .core {
			let inIt = notes.filter { MemoryCategory.of($0.path) == category }
			if !inIt.isEmpty { groups.append(NoteGroup(title: category.name, notes: inIt)) }
		}
		let unsorted = notes.filter { MemoryCategory.of($0.path) == nil }
		if !unsorted.isEmpty { groups.append(NoteGroup(title: String(localized: "Unsorted"), notes: unsorted)) }
		return groups
	}

	private func subtitle(_ note: MemoryOverview.Note, pinned: Bool) -> String {
		var parts: [String] = []
		if pinned {
			parts.append(String(localized: "pinned, in every new chat"))
		} else {
			parts.append(note.path)
			if let member = memory?.members.first(where: { $0.note == note.path }) {
				parts.append(String(localized: "member: \(member.name)"))
			}
		}
		if !note.facts.isEmpty { parts.append(String(localized: "\(note.facts.count) memories")) }
		if note.updatedAt > 0 { parts.append(String(localized: "updated \(note.updated.formatted(.relative(presentation: .named)))")) }
		return parts.joined(separator: " · ")
	}

	private func title(of path: String) -> String {
		memory?.files.first { $0.path == path }.map(MemoryCategory.title(of:)) ?? path
	}

	private func picture(_ person: String) -> URL? {
		guard let path = family.profile?.members.first(where: { $0.id == person })?.picture else { return nil }
		return URL(string: path, relativeTo: client.origin)?.absoluteURL
	}

	// MARK: Doing

	private func load() async {
		do {
			memory = try await client.memory(slug)
		} catch {
			// The screen went away while it loaded: it loads again when it's back.
			if !error.isCancellation { problem = error.localizedDescription }
		}
	}

	private func learn(_ on: Bool) {
		Task {
			do {
				try await client.learnFromChats(slug, on)
				await load()
			} catch {
				problem = error.localizedDescription
			}
		}
	}

	private func undo(_ change: MemoryOverview.Change) async {
		do {
			if let reason = try await client.undoChange(slug, change.id) {
				switch reason {
				case "undone": problem = String(localized: "It was already undone.")
				case "owner": problem = String(localized: "Only \(change.card?.owner ?? "") can undo this.")
				default: problem = String(localized: "It changed since, so it can't be undone here. Edit the note on the Memory page.")
				}
			}
			Haptics.success()
			await load()
		} catch {
			problem = error.localizedDescription
		}
	}
}

extension MemoryCategory {
	/// Its name, as the memory page has it.
	var name: String {
		switch self {
		case .core: return String(localized: "Core")
		case .people: return String(localized: "People")
		case .home: return String(localized: "Home")
		case .health: return String(localized: "Health")
		case .plans: return String(localized: "Plans")
		case .routines: return String(localized: "Routines")
		case .pets: return String(localized: "Pets")
		case .places: return String(localized: "Places")
		case .projects: return String(localized: "Projects")
		case .other: return String(localized: "Other")
		}
	}
}

/// Something the note-taker saved from a chat: what, where, when, and Undo.
private struct ChangeRow: View {
	let change: MemoryOverview.Change
	let me: String?
	let title: String
	let undo: () async -> Void
	@State private var working = false

	var body: some View {
		HStack(alignment: .top, spacing: 10) {
			Image(systemName: change.op == "replace" ? "arrow.triangle.2.circlepath" : "plus.circle")
				.foregroundStyle(.secondary)
			VStack(alignment: .leading, spacing: 3) {
				Text(verbatim: change.fact)
					.strikethrough(change.undone != nil)
				if let before = change.before {
					Text("was: \(before)")
						.font(.caption)
						.foregroundStyle(.secondary)
				}
				Text(verbatim: [title, from, change.created.formatted(.relative(presentation: .named))].joined(separator: " · "))
					.font(.caption)
					.foregroundStyle(.secondary)
			}
			Spacer(minLength: 4)
			if change.undone == nil {
				Button {
					Task {
						working = true
						await undo()
						working = false
					}
				} label: {
					if working { ProgressView() } else { Text("Undo") }
				}
				.buttonStyle(.bordered)
				.font(.caption)
			} else {
				Text("Undone")
					.font(.caption)
					.foregroundStyle(.secondary)
			}
		}
	}

	private var from: String {
		guard let chat = change.conversation else { return String(localized: "from a deleted chat") }
		return String(localized: "from \(chat.title.isEmpty ? String(localized: "a chat") : chat.title)")
	}
}

/// A note: to read, edit, move or merge, and forget.
private struct NoteView: View {
	@Environment(\.palette) private var palette
	let note: MemoryOverview.Note
	let slug: String
	let client: Client
	let memory: MemoryOverview?
	let pinned: Bool
	let changed: () async -> Void
	@Environment(\.dismiss) private var dismiss
	@State private var editing = false
	@State private var moving = false
	@State private var forgetting = false
	@State private var problem: String?

	var body: some View {
		ScrollView {
			VStack(alignment: .leading, spacing: 12) {
				if note.text.isEmpty {
					Text("Nothing yet. Put here what nolune should keep in mind in every chat: who's in the family, the languages you speak, allergies. nolune adds to it too.")
						.foregroundStyle(.secondary)
				} else if note.path.hasSuffix(".md") {
					MarkdownView(text: note.text)
				} else {
					Text(verbatim: note.text)
						.font(.callout.monospaced())
						.textSelection(.enabled)
				}
				if MemoryCategory.of(note.path) == nil {
					Label("From before memory had categories: nolune reads it, but adds nothing to it. Move it into a category to keep it growing.", systemImage: "exclamationmark.triangle")
						.font(.footnote)
						.foregroundStyle(Palette.plain.warning)
				}
			}
			.padding()
			.frame(maxWidth: .infinity, alignment: .leading)
		}
		.background(palette.background)
		.navigationTitle(pinned ? String(localized: "Core") : MemoryCategory.title(of: note))
		.navigationBarTitleDisplayMode(.inline)
		.toolbar {
			ToolbarItemGroup(placement: .primaryAction) {
				Button {
					editing = true
				} label: {
					Image(systemName: "pencil")
				}
				.accessibilityLabel(Text("Edit"))
				if !pinned {
					Menu {
						Button {
							moving = true
						} label: {
							Label("Move to", systemImage: "arrow.right.doc.on.clipboard")
						}
						Button(role: .destructive) {
							forgetting = true
						} label: {
							Label("Forget", systemImage: "eraser")
						}
					} label: {
						Image(systemName: "ellipsis.circle")
					}
				}
			}
		}
		.sheet(isPresented: $editing) {
			NoteEditor(note: note, slug: slug, client: client, maxChars: pinned ? memory?.core.maxChars : nil) {
				await changed()
				dismiss()
			}
		}
		.sheet(isPresented: $moving) {
			MoveNote(note: note, notes: memory?.files ?? []) { to in
				do {
					_ = try await client.moveNote(slug, from: note.path, to: to)
					Haptics.success()
					await changed()
					dismiss()
				} catch {
					problem = error.localizedDescription
				}
			}
		}
		.confirmationDialog("Forget \"\(MemoryCategory.title(of: note))\"?", isPresented: $forgetting, titleVisibility: .visible) {
			Button("Forget", role: .destructive) {
				Task {
					do {
						try await client.forgetNote(slug, path: note.path)
						await changed()
						dismiss()
					} catch {
						problem = error.localizedDescription
					}
				}
			}
		}
		.alert(problem ?? "", isPresented: Binding(get: { problem != nil }, set: { if !$0 { problem = nil } })) {
			Button("OK", role: .cancel) {}
		}
	}
}

/// A note's text, to change. Saved against the version it opened, so someone else's change in the
/// meantime isn't lost: then it says so, and saving again keeps this one.
private struct NoteEditor: View {
	let note: MemoryOverview.Note
	let slug: String
	let client: Client
	let maxChars: Int?
	let saved: () async -> Void
	@Environment(\.dismiss) private var dismiss
	@State private var text = ""
	@State private var basedOn: Double = 0
	@State private var saving = false
	@State private var problem: String?

	var body: some View {
		NavigationStack {
			VStack(alignment: .leading, spacing: 8) {
				TextEditor(text: $text)
					.font(.body)
				if let maxChars {
					Text(verbatim: "\(text.count) / \(maxChars)")
						.font(.caption.monospacedDigit())
						.foregroundStyle(text.count > maxChars ? Palette.plain.destructive : Color.secondary)
				}
				if let problem {
					Text(verbatim: problem)
						.font(.footnote)
						.foregroundStyle(Palette.plain.destructive)
				}
			}
			.padding()
			.navigationTitle(MemoryCategory.title(of: note))
			.navigationBarTitleDisplayMode(.inline)
			.toolbar {
				ToolbarItem(placement: .cancellationAction) {
					Button("Cancel") { dismiss() }
				}
				ToolbarItem(placement: .confirmationAction) {
					Button("Save") { Task { await save() } }
						.disabled(saving || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
				}
			}
		}
		.onAppear {
			text = note.text
			basedOn = note.updatedAt
		}
	}

	private func save() async {
		saving = true
		defer { saving = false }
		do {
			try await client.writeNote(slug, path: note.path, text: text, basedOn: basedOn)
			Haptics.success()
			await saved()
			dismiss()
		} catch let refusal as Refusal where refusal.status == 409 {
			// It changed since: the next save is against what's there now.
			problem = refusal.localizedDescription
			if let now = try? await client.memory(slug).files.first(where: { $0.path == note.path }) {
				basedOn = now.updatedAt
			}
		} catch {
			problem = error.localizedDescription
		}
	}
}

/// Where a note goes: a category, someone's note, or a project's; into one that's there, it merges.
private struct MoveNote: View {
	@Environment(\.palette) private var palette
	let note: MemoryOverview.Note
	let notes: [MemoryOverview.Note]
	let move: (String) async -> Void
	@Environment(\.dismiss) private var dismiss
	@State private var target = ""
	@State private var newName = ""
	@State private var working = false

	private static let newPerson = "people/"
	private static let newProject = "projects/"

	private var path: String? {
		switch target {
		case Self.newPerson, Self.newProject:
			let name = MemoryCategory.fileName(newName)
			return name.isEmpty ? nil : "\(target)\(name).md"
		case "":
			return nil
		default:
			return target
		}
	}

	private func option(_ name: String, _ value: String) -> some View {
		Button {
			target = value
		} label: {
			HStack {
				Text(verbatim: name)
					.foregroundStyle(Palette.plain.foreground)
				Spacer()
				if target == value {
					Image(systemName: "checkmark")
						.foregroundStyle(Palette.plain.primary)
				}
			}
		}
		.disabled(value == note.path)
	}

	var body: some View {
		NavigationStack {
			Form {
				Group {
					Section {
						ForEach(MemoryCategory.notes.filter { $0 != .projects }, id: \.self) { category in
							option(category.name, "\(category.rawValue).md")
						}
					} header: {
						Text("Categories")
					} footer: {
						Text("Choose where it belongs. If that note is there already, the two become one.")
					}
					Section(MemoryCategory.people.name) {
						ForEach(notes.filter { MemoryCategory.of($0.path) == .people && $0.path != note.path }) { person in
							option(MemoryCategory.title(of: person), person.path)
						}
						option(String(localized: "Someone new…"), Self.newPerson)
						if target == Self.newPerson {
							TextField("Their name", text: $newName)
						}
					}
					Section(MemoryCategory.projects.name) {
						ForEach(notes.filter { MemoryCategory.of($0.path) == .projects && $0.path != note.path }) { project in
							option(MemoryCategory.title(of: project), project.path)
						}
						option(String(localized: "A new project…"), Self.newProject)
						if target == Self.newProject {
							TextField("Project name", text: $newName)
						}
					}
				}
				.listRowBackground(palette.muted)
			}
			.pageBackground(palette)
			.navigationTitle("Move \"\(MemoryCategory.title(of: note))\"")
			.navigationBarTitleDisplayMode(.inline)
			.toolbar {
				ToolbarItem(placement: .cancellationAction) {
					Button("Cancel") { dismiss() }
				}
				ToolbarItem(placement: .confirmationAction) {
					Button(notes.contains { $0.path == path } ? "Merge" : "Move") {
						guard let path else { return }
						Task {
							working = true
							await move(path)
							working = false
							dismiss()
						}
					}
					.disabled(path == nil || path == note.path || working)
				}
			}
		}
	}
}
