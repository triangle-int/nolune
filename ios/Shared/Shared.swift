import Foundation
import Security

/**
 * What the app shares with its widgets and its share extension (#138), through their app group:
 * the family's nolune and its session (in the group's keychain, so the share extension sends as
 * the person), the person's profiles, and the bell's latest notifications for the widgets. The app
 * writes it whenever it has them newer; signing out takes the session away.
 */
enum Shared {
	static let group = "group.dev.nolune.app"

	private static var defaults: UserDefaults? { UserDefaults(suiteName: group) }

	// MARK: The session

	/// The family's nolune, and the cookies a request to it carries.
	struct Session: Codable, Equatable {
		let origin: URL
		/// The `Cookie` header: `name=value; name=value`.
		let cookies: String
	}

	private static let sessionQuery: [String: Any] = [
		kSecClass as String: kSecClassGenericPassword,
		kSecAttrService as String: "dev.nolune.session",
		kSecAttrAccount as String: "session",
		kSecAttrAccessGroup as String: group
	]

	static var session: Session? {
		get {
			var query = sessionQuery
			query[kSecReturnData as String] = true
			query[kSecMatchLimit as String] = kSecMatchLimitOne
			var found: CFTypeRef?
			guard SecItemCopyMatching(query as CFDictionary, &found) == errSecSuccess, let data = found as? Data else { return nil }
			return try? JSONDecoder().decode(Session.self, from: data)
		}
		set {
			SecItemDelete(sessionQuery as CFDictionary)
			guard let newValue, let data = try? JSONEncoder().encode(newValue) else { return }
			var item = sessionQuery
			item[kSecValueData as String] = data
			item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
			SecItemAdd(item as CFDictionary, nil)
		}
	}

	// MARK: Profiles

	struct Profile: Codable, Hashable, Identifiable {
		let slug: String
		let name: String
		let avatar: String

		var id: String { slug }
	}

	/// The person's profiles, the one they last had open first.
	static var profiles: [Profile] {
		get { read([Profile].self, "profiles") ?? [] }
		set { write(newValue, "profiles") }
	}

	// MARK: The bell

	struct Bell: Codable, Equatable {
		struct Item: Codable, Hashable, Identifiable {
			let id: String
			let title: String
			let body: String
			let createdAt: Date
			let profile: Profile
			let new: Bool

			/// What the app opens for it: the bell, with it in full.
			var path: String { "/p/\(profile.slug)?notification=\(id)" }
		}

		var items: [Item]
		var unseen: Int

		static let empty = Bell(items: [], unseen: 0)
	}

	static var bell: Bell {
		get { read(Bell.self, "bell") ?? .empty }
		set { write(newValue, "bell") }
	}

	// MARK: Opening the app

	/// An address that opens the app on a page of the family's nolune: `nolune://open?path=/p/smiths`.
	static func open(_ path: String) -> URL {
		// The path's own query stays in it: `&`, `=` and `+` encoded too, as not every Foundation does.
		let allowed = CharacterSet.urlQueryAllowed.subtracting(CharacterSet(charactersIn: "&=+#"))
		let value = path.addingPercentEncoding(withAllowedCharacters: allowed) ?? path
		return URL(string: "nolune://open?path=\(value)") ?? URL(string: "nolune://open")!
	}

	/// The page an address made by `open` leads to.
	static func path(of url: URL) -> String? {
		guard url.scheme == "nolune", url.host == "open" else { return nil }
		return URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first { $0.name == "path" }?.value
	}

	private static func read<T: Decodable>(_ type: T.Type, _ key: String) -> T? {
		guard let data = defaults?.data(forKey: key) else { return nil }
		return try? JSONDecoder().decode(type, from: data)
	}

	private static func write<T: Encodable>(_ value: T, _ key: String) {
		guard let data = try? JSONEncoder().encode(value) else { return }
		defaults?.set(data, forKey: key)
	}
}
