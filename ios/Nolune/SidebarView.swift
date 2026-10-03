import SwiftUI

/**
 * The family's nolune's navigation, as the web sidebar is (#130): the open profile, a new chat,
 * the profile's pages, its folders with their chats, and its other chats, most recently active
 * first, with a sign on the ones nolune is working in. Chats rename, move and delete from a swipe
 * or their menu; the toolbar switches profiles and has the bell and the account.
 */
struct SidebarView: View {
	@Environment(\.palette) private var palette
	@ObservedObject var family: Family
	@Binding var selection: Destination?
	let showBell: () -> Void
	let signOut: () -> Void
	let connectElsewhere: () -> Void

	@State private var search = ""
	@State private var expanded: Set<String> = []
	@State private var prompt: Prompt?
	@State private var typed = ""
	@State private var deletingChat: ChatSummary?
	@State private var deletingFolder: Folder?
	@State private var moving: ChatSummary?

	/// A name to type: a chat's, a folder's, or a new folder's (with a chat to move into it).
	private enum Prompt {
		case renameChat(ChatSummary)
		case renameFolder(Folder)
		case newFolder(moving: ChatSummary?)

		var title: String {
			switch self {
			case .renameChat: return String(localized: "Rename chat")
			case .renameFolder: return String(localized: "Rename folder")
			case .newFolder: return String(localized: "New folder")
			}
		}

		var field: String {
			switch self {
			case .renameChat: return String(localized: "Chat name")
			case .renameFolder, .newFolder: return String(localized: "Folder name")
			}
		}
	}

	var body: some View {
		List(selection: $selection) {
			Group {
				if search.isEmpty {
					browsing
				} else {
					results
				}
			}
			.listRowBackground(Color.clear)
			.listRowSeparator(.hidden)
		}
		.scrollContentBackground(.hidden)
		.background(palette.sidebar)
		.listStyle(.sidebar)
		.navigationTitle(family.profile?.name ?? "nolune")
		.toolbarTitleMenu { profileMenu }
		.toolbar { toolbar }
		.searchable(text: $search, prompt: Text("Search chats"))
		.task(id: search.isEmpty) {
			// Searching looks through every chat, not only the pages already loaded.
			if !search.isEmpty { await family.loadAll() }
		}
		.refreshable {
			await family.reloadProfile()
			await family.reloadBell()
		}
		.overlay {
			if !family.loaded { ProgressView() }
		}
		.alert(
			prompt?.title ?? "",
			isPresented: Binding(get: { prompt != nil }, set: { if !$0 { prompt = nil } })
		) {
			TextField(prompt?.field ?? "", text: $typed)
			Button("Save", action: submitPrompt)
			Button("Cancel", role: .cancel) {}
		}
		.confirmationDialog(
			"Delete chat?",
			isPresented: Binding(get: { deletingChat != nil }, set: { if !$0 { deletingChat = nil } }),
			titleVisibility: .visible,
			presenting: deletingChat
		) { chat in
			Button("Delete", role: .destructive) {
				if selection == .chat(chat.id) { selection = nil }
				Task { await family.delete(chat) }
			}
		} message: { chat in
			Text("This deletes \(name(of: chat)) for everyone in the profile.")
		}
		.confirmationDialog(
			"Delete folder?",
			isPresented: Binding(get: { deletingFolder != nil }, set: { if !$0 { deletingFolder = nil } }),
			titleVisibility: .visible,
			presenting: deletingFolder
		) { folder in
			Button("Delete", role: .destructive) {
				if selection == .folder(folder.id) { selection = nil }
				Task { await family.delete(folder) }
			}
		} message: { folder in
			Text("\(folder.name) is deleted for everyone in the profile. Its chats move back to your chat list, without its instructions and files. The files are moved to ~/.nolune/trash.")
		}
		.confirmationDialog(
			"Move to folder",
			isPresented: Binding(get: { moving != nil }, set: { if !$0 { moving = nil } }),
			titleVisibility: .visible,
			presenting: moving
		) { chat in
			moveButtons(chat)
		}
	}

	// MARK: The list

	@ViewBuilder private var browsing: some View {
		Section {
			Label("New chat", systemImage: "square.and.pencil")
				.tag(Destination.newChat)
			ForEach(WebPage.profilePages, id: \.self) { page in
				Label(page.title, systemImage: page.symbol)
					.tag(Destination.page(page))
			}
		}
		Section("Folders") {
			ForEach(family.folders) { folder in
				DisclosureGroup(isExpanded: expandedBinding(folder.id)) {
					let inside = chats(in: folder.id)
					if inside.isEmpty {
						Text("No chats in this folder yet.")
							.font(.footnote)
							.foregroundStyle(.secondary)
					}
					ForEach(inside) { chat in row(chat) }
				} label: {
					Label(folder.name, systemImage: "folder")
						.tag(Destination.folder(folder.id))
						.contextMenu { folderMenu(folder) }
				}
			}
			Button {
				ask(.newFolder(moving: nil), typed: "")
			} label: {
				Label("New folder", systemImage: "folder.badge.plus")
			}
		}
		Section("Chats") {
			let loose = chats(in: nil)
			if loose.isEmpty, family.loaded {
				Text("Your chats will show up here.")
					.font(.footnote)
					.foregroundStyle(.secondary)
			}
			ForEach(loose) { chat in row(chat) }
			if family.more != nil {
				ProgressView()
					.frame(maxWidth: .infinity)
					.task { await family.loadMore() }
			}
		}
	}

	@ViewBuilder private var results: some View {
		let folders = family.folders.filter { $0.name.localizedCaseInsensitiveContains(search) }
		let chats = family.chats.filter { name(of: $0).localizedCaseInsensitiveContains(search) }
		if folders.isEmpty, chats.isEmpty {
			Text("No chats found.")
				.foregroundStyle(.secondary)
		}
		if !folders.isEmpty {
			Section("Folders") {
				ForEach(folders) { folder in
					Label(folder.name, systemImage: "folder")
						.tag(Destination.folder(folder.id))
				}
			}
		}
		if !chats.isEmpty {
			Section("Chats") {
				ForEach(chats) { chat in row(chat) }
			}
		}
	}

	private func row(_ chat: ChatSummary) -> some View {
		ChatRow(title: name(of: chat), running: family.running.contains(chat.id))
			.tag(Destination.chat(chat.id))
			.swipeActions(edge: .trailing, allowsFullSwipe: false) {
				Button(role: .destructive) {
					deletingChat = chat
				} label: {
					Label("Delete", systemImage: "trash")
				}
				Button {
					moving = chat
				} label: {
					Label("Move to folder", systemImage: "folder")
				}
				.tint(.indigo)
				Button {
					ask(.renameChat(chat), typed: chat.title)
				} label: {
					Label("Rename", systemImage: "pencil")
				}
				.tint(.gray)
			}
			.contextMenu { chatMenu(chat) }
	}

	@ViewBuilder private func chatMenu(_ chat: ChatSummary) -> some View {
		Button {
			ask(.renameChat(chat), typed: chat.title)
		} label: {
			Label("Rename", systemImage: "pencil")
		}
		Menu {
			moveButtons(chat)
		} label: {
			Label("Move to folder", systemImage: "folder")
		}
		Divider()
		Button(role: .destructive) {
			deletingChat = chat
		} label: {
			Label("Delete", systemImage: "trash")
		}
	}

	@ViewBuilder private func moveButtons(_ chat: ChatSummary) -> some View {
		if chat.folderId != nil {
			Button("No folder") { Task { await family.move(chat, to: nil) } }
		}
		ForEach(family.folders.filter { $0.id != chat.folderId }) { folder in
			Button(folder.name) {
				Task {
					if await family.move(chat, to: folder.id) { expanded.insert(folder.id) }
				}
			}
		}
		Button("New folder…") { ask(.newFolder(moving: chat), typed: "") }
	}

	@ViewBuilder private func folderMenu(_ folder: Folder) -> some View {
		Button {
			ask(.renameFolder(folder), typed: folder.name)
		} label: {
			Label("Rename", systemImage: "pencil")
		}
		Button(role: .destructive) {
			deletingFolder = folder
		} label: {
			Label("Delete", systemImage: "trash")
		}
	}

	// MARK: The toolbar

	@ToolbarContentBuilder private var toolbar: some ToolbarContent {
		ToolbarItem(placement: .navigationBarLeading) {
			Menu {
				profileMenu
			} label: {
				if let avatar = family.profile?.avatar {
					AvatarView(avatar: avatar)
						.frame(width: 26, height: 26)
				} else {
					Image(systemName: "person.2")
				}
			}
			.accessibilityLabel(Text("Profiles"))
		}
		ToolbarItemGroup(placement: .navigationBarTrailing) {
			Button(action: showBell) {
				Image(systemName: family.bell.unseen > 0 ? "bell.badge" : "bell")
			}
			.accessibilityLabel(
				family.bell.unseen > 0 ? Text("Notifications, \(family.bell.unseen) new") : Text("Notifications")
			)
			Menu {
				accountMenu
			} label: {
				Image(systemName: "person.crop.circle")
			}
			.accessibilityLabel(Text("Account"))
			Button {
				selection = .newChat
			} label: {
				Image(systemName: "square.and.pencil")
			}
			.accessibilityLabel(Text("New chat"))
		}
	}

	@ViewBuilder private var profileMenu: some View {
		Section("Profiles") {
			ForEach(family.profiles) { profile in
				Button {
					guard profile.slug != family.slug else { return }
					selection = nil
					Task { await family.open(profile: profile.slug) }
				} label: {
					if profile.slug == family.slug {
						Label(profile.name, systemImage: "checkmark")
					} else {
						Text(profile.name)
					}
				}
			}
		}
		Button {
			selection = .page(.profiles)
		} label: {
			Label("New profile", systemImage: "plus")
		}
	}

	@ViewBuilder private var accountMenu: some View {
		if let me = family.me {
			Section(me.name) {}
		}
		Button {
			selection = .settings
		} label: {
			Label("Settings", systemImage: "gearshape")
		}
		Button {
			selection = .page(.profiles)
		} label: {
			Label(WebPage.profiles.title, systemImage: WebPage.profiles.symbol)
		}
		Button {
			selection = .page(.card)
		} label: {
			Label(WebPage.card.title, systemImage: WebPage.card.symbol)
		}
		if family.me?.isAdmin == true {
			Section {
				ForEach([WebPage.modelsAndKeys, .services, .adminPeople], id: \.self) { page in
					Button {
						selection = .page(page)
					} label: {
						Label(page.title, systemImage: page.symbol)
					}
				}
			}
		}
		Section {
			Button(action: connectElsewhere) {
				Label("Connect to another nolune", systemImage: "arrow.left.arrow.right")
			}
			Button(role: .destructive, action: signOut) {
				Label("Log out", systemImage: "rectangle.portrait.and.arrow.right")
			}
		}
	}

	// MARK: Helpers

	private func chats(in folder: String?) -> [ChatSummary] {
		family.chats.filter { $0.folderId == folder }
	}

	private func name(of chat: ChatSummary) -> String {
		chat.title.isEmpty ? String(localized: "New chat") : chat.title
	}

	private func expandedBinding(_ folder: String) -> Binding<Bool> {
		Binding(
			get: { expanded.contains(folder) },
			set: { open in
				if open { expanded.insert(folder) } else { expanded.remove(folder) }
			}
		)
	}

	private func ask(_ prompt: Prompt, typed text: String) {
		typed = text
		self.prompt = prompt
	}

	private func submitPrompt() {
		let text = typed.trimmingCharacters(in: .whitespacesAndNewlines)
		guard let prompt, !text.isEmpty else { return }
		Task {
			switch prompt {
			case .renameChat(let chat):
				await family.rename(chat, to: text)
			case .renameFolder(let folder):
				await family.rename(folder, to: text)
			case .newFolder(let chat):
				if let folder = await family.createFolder(named: text, moving: chat) {
					expanded.insert(folder.id)
				}
			}
		}
	}
}

/// A chat in the sidebar: its title, and a sign while nolune works in it.
struct ChatRow: View {
	let title: String
	let running: Bool

	var body: some View {
		HStack(spacing: 8) {
			Text(title)
				.lineLimit(1)
			Spacer(minLength: 0)
			if running {
				ProgressView()
					.controlSize(.small)
			}
		}
		.accessibilityElement(children: .combine)
		.accessibilityValue(running ? Text("nolune is working") : Text(verbatim: ""))
	}
}
