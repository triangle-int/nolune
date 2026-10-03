import SwiftUI

/**
 * The family's nolune, natively: the sidebar (SidebarView) and what it opens beside it on an
 * iPad, or pushed on an iPhone. Screens that aren't native yet are their web pages under the
 * app's navigation (WebScreen); a link in one to a chat or another screen opens it here.
 */
struct MainView: View {
	@ObservedObject var family: Family
	let signOut: () -> Void
	let connectElsewhere: () -> Void
	@ObservedObject private var model = AppModel.shared
	@Environment(\.horizontalSizeClass) private var sizeClass
	@State private var selection: Destination?
	@State private var bell: BellSheet?

	/// The bell, with a notification to expand in it.
	private struct BellSheet: Identifiable {
		let id = UUID()
		let expand: String?
	}

	var body: some View {
		NavigationSplitView {
			SidebarView(
				family: family,
				selection: $selection,
				showBell: { bell = BellSheet(expand: nil) },
				signOut: signOut,
				connectElsewhere: connectElsewhere
			)
		} detail: {
			NavigationStack {
				if let selection {
					// Again for another profile: a profile's page is that profile's.
					screen(selection)
						.id([family.slug ?? "", String(describing: selection)])
				} else {
					Text("Pick a chat, or start a new one.")
						.foregroundStyle(.secondary)
				}
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

	@ViewBuilder private func screen(_ destination: Destination) -> some View {
		let slug = family.slug ?? ""
		switch destination {
		case .newChat:
			web("/p/\(slug)", for: destination)
				.navigationTitle("New chat")
		case .chat(let id):
			web("/p/\(slug)/c/\(id)", for: destination)
				.navigationTitle(chatTitle(id))
		case .folder(let id):
			web("/p/\(slug)/f/\(id)", for: destination)
				.navigationTitle(family.folders.first { $0.id == id }?.name ?? "")
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

	private func chatTitle(_ id: String) -> String {
		guard let chat = family.chat(id) else { return "" }
		return chat.title.isEmpty ? String(localized: "New chat") : chat.title
	}

	/// A link on a web screen, to something shown natively: true when it opens here.
	private func open(_ url: URL, from current: Destination) -> Bool {
		guard Address.sameOrigin(url, family.client.origin), let found = Destination.of(path: url.path) else {
			return false
		}
		let slug = found.slug ?? family.slug
		guard found.destination != current || slug != family.slug else { return false }
		Task {
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
