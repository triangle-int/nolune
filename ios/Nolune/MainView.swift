import SwiftUI

/**
 * The family's nolune, natively: the sidebar (SidebarView) and what it opens beside it on an
 * iPad, or pushed on an iPhone: a new chat (NewChatView), a chat (ChatView), settings. Screens that
 * aren't native yet are their web pages under the app's navigation (WebScreen); a link in one to a
 * chat or another screen opens it here.
 */
struct MainView: View {
	@ObservedObject var family: Family
	let signOut: () -> Void
	let connectElsewhere: () -> Void
	@ObservedObject private var model = AppModel.shared
	@Environment(\.horizontalSizeClass) private var sizeClass
	@State private var selection: Destination?
	@State private var bell: BellSheet?
	/// The folder a new chat starts in, from a folder page's "New chat in" link.
	@State private var newChatFolder: String?

	/// The bell, with a notification to expand in it.
	private struct BellSheet: Identifiable {
		let id = UUID()
		let expand: String?
	}

	var body: some View {
		Group {
			if #available(iOS 17.0, *) {
				FollowingSplitView(selection: selection) { sidebar } detail: { detail }
			} else {
				NavigationSplitView { sidebar } detail: { detail }
			}
		}
		.sheet(item: $bell) { sheet in
			BellView(family: family, expand: sheet.expand) { place in
				selection = .chat(place.conversationId)
			}
		}
		.alert(
			family.problem ?? "",
			isPresented: Binding(get: { family.problem != nil }, set: { if !$0 { family.problem = nil } })
		) {
			Button("OK", role: .cancel) {}
		}
		.task {
			await family.start()
			if sizeClass == .regular, selection == nil { selection = .newChat }
			openPending()
		}
		.onDisappear { family.stop() }
		.onReceive(model.$pending.compactMap { $0 }.receive(on: DispatchQueue.main)) { _ in
			openPending()
		}
		.onChange(of: family.slug) { _ in
			// Another profile: a chat or folder that was open was the last one's.
			switch selection {
			case .chat?, .folder?: selection = sizeClass == .regular ? .newChat : nil
			default: break
			}
		}
	}

	private var sidebar: some View {
		SidebarView(
			family: family,
			selection: $selection,
			showBell: { bell = BellSheet(expand: nil) },
			signOut: signOut,
			connectElsewhere: connectElsewhere
		)
	}

	private var detail: some View {
		NavigationStack {
			if let selection {
				// Again for another profile: a profile's page is that profile's.
				screen(selection)
					.id([family.slug ?? "", String(describing: selection), newChatFolder ?? ""])
			} else {
				Text("Pick a chat, or start a new one.")
					.foregroundStyle(.secondary)
			}
		}
	}

	@ViewBuilder private func screen(_ destination: Destination) -> some View {
		let slug = family.slug ?? ""
		switch destination {
		case .newChat:
			NewChatView(family: family, folder: newChatFolder) { id in
				newChatFolder = nil
				selection = .chat(id)
			}
			.navigationTitle("New chat")
			.navigationBarTitleDisplayMode(.inline)
		case .chat(let id):
			ChatView(
				id: id,
				family: family,
				open: { url in open(url, from: destination) },
				deleted: { selection = sizeClass == .regular ? .newChat : nil }
			)
			.navigationBarTitleDisplayMode(.inline)
		case .folder(let id) where family.can("folders"):
			FolderView(
				family: family,
				slug: slug,
				id: id,
				open: { selection = $0 },
				newChat: {
					newChatFolder = id
					selection = .newChat
				},
				deleted: { selection = sizeClass == .regular ? .newChat : nil }
			)
		case .folder(let id):
			web("/p/\(slug)/f/\(id)", for: destination)
				.navigationTitle(family.folders.first { $0.id == id }?.name ?? "")
		case .page(.memory) where family.can("memory"):
			MemoryView(family: family, slug: slug) { selection = $0 }
		case .page(.automations) where family.can("automations"):
			AutomationsView(family: family, slug: slug) { selection = .chat($0) }
		case .page(.skills) where family.can("skills"):
			SkillsView(family: family, slug: slug)
		case .page(.people) where family.can("profile"):
			ProfileView(family: family, slug: slug) {
				selection = nil
				Task { await family.reload() }
			}
		case .page(.images) where family.can("images"):
			ImagesView(family: family, slug: slug) { selection = .chat($0) }
		case .page(let page):
			web(page.path(in: slug), for: destination)
				.navigationTitle(page.title)
		case .settings:
			SettingsView(family: family, signOut: signOut)
		case .web(let path):
			web(path, for: destination)
		}
	}

	private func web(_ path: String, for destination: Destination) -> some View {
		WebScreen(
			origin: family.client.origin,
			path: path,
			open: { url in open(url, from: destination) },
			signedOut: signOut
		)
		.ignoresSafeArea(.container, edges: .bottom)
		.navigationBarTitleDisplayMode(.inline)
	}

	/// A link on a web screen, to something shown natively: true when it opens here.
	private func open(_ url: URL, from current: Destination) -> Bool {
		guard Address.sameOrigin(url, family.client.origin), let found = Destination.of(path: url.path) else {
			return false
		}
		let slug = found.slug ?? family.slug
		let folder = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "folder" }?.value
		guard found.destination != current || slug != family.slug || folder != nil else { return false }
		Task {
			if found.destination == .newChat { newChatFolder = folder }
			if let slug, slug != family.slug { await family.open(profile: slug) }
			if case .chat(let id) = found.destination, family.chat(id) == nil { await family.reloadProfile() }
			selection = found.destination
		}
		return true
	}

	/**
	 * A page to open: a tapped notification's (`?notification=<id>`), which opens the bell with it,
	 * or a link's.
	 */
	private func openPending() {
		guard let path = model.takePending(), let components = URLComponents(string: path) else { return }
		if let id = components.queryItems?.first(where: { $0.name == "notification" })?.value {
			bell = BellSheet(expand: id)
			Task { await family.reloadBell() }
			return
		}
		guard let found = Destination.of(path: components.path) else {
			selection = .web(path)
			return
		}
		Task {
			if let slug = found.slug, slug != family.slug { await family.open(profile: slug) }
			selection = found.destination
		}
	}
}

/**
 * The split view, showing what's selected on an iPhone even when the sidebar has no row for it:
 * a chat in a folder that's closed, one a notification continued in, one not loaded yet.
 */
@available(iOS 17.0, *)
private struct FollowingSplitView<Sidebar: View, Detail: View>: View {
	let selection: Destination?
	@ViewBuilder let sidebar: Sidebar
	@ViewBuilder let detail: Detail
	@State private var column = NavigationSplitViewColumn.sidebar

	var body: some View {
		NavigationSplitView(preferredCompactColumn: $column) {
			sidebar
		} detail: {
			detail
		}
		.onChange(of: selection) { _, selected in
			column = selected == nil ? .sidebar : .detail
		}
	}
}
