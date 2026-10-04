import Foundation
import WidgetKit

/**
 * What the app leaves its widgets and share extension in their app group (Shared/Shared.swift):
 * the session, so the share extension sends as the person; their profiles, the open one first; and
 * the bell. The widgets are drawn again when it changes; signing out takes it all away.
 */
@MainActor
enum Sharing {
	/// The nolune's address and the cookies the app has for it, from the cookie storage `Client` uses.
	static func signedIn(_ origin: URL, storage: HTTPCookieStorage = .shared) {
		let cookies = storage.cookies(for: origin) ?? []
		guard let header = HTTPCookie.requestHeaderFields(with: cookies)["Cookie"], !header.isEmpty else { return }
		let session = Shared.Session(origin: origin, cookies: header)
		if Shared.session != session { Shared.session = session }
	}

	static func update(profiles: [Profile], open slug: String?) {
		let shared = (profiles.filter { $0.slug == slug } + profiles.filter { $0.slug != slug })
			.map { Shared.Profile(slug: $0.slug, name: $0.name, avatar: $0.avatar) }
		guard shared != Shared.profiles else { return }
		Shared.profiles = shared
		WidgetCenter.shared.reloadTimelines(ofKind: "ask")
	}

	/// The bell's latest notifications, as text: the widgets show a few lines of each.
	static func update(bell: Bell) {
		let shared = Shared.Bell(
			items: bell.items.prefix(5).map { item in
				Shared.Bell.Item(
					id: item.id,
					title: item.title,
					body: Markdown.plain(item.body),
					createdAt: item.created,
					profile: Shared.Profile(slug: item.profile.slug, name: item.profile.name, avatar: item.profile.avatar),
					new: item.createdAt > bell.seenAt
				)
			},
			unseen: bell.items.filter { $0.createdAt > bell.seenAt }.count
		)
		guard shared != Shared.bell else { return }
		Shared.bell = shared
		WidgetCenter.shared.reloadTimelines(ofKind: "bell")
	}

	static func signedOut() {
		Shared.session = nil
		Shared.profiles = []
		Shared.bell = .empty
		WidgetCenter.shared.reloadAllTimelines()
	}
}
