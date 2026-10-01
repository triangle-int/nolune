import Foundation

/**
 * What someone types or pastes to connect: a name on nolune's relay (`smiths`), an address
 * (`smiths.nolune.family`, `nolune.example.com`), or a link from their nolune, like the invite
 * someone sent them, whose page opens first. Addresses are https, but for a computer on this
 * network (`http://mac-mini.local:5780`, localhost), where the app may use plain http.
 */
struct Address: Equatable {
	/// Where nolune's relay gives families their addresses (packages/relay).
	static let relayDomain = "nolune.family"

	let origin: URL
	/// A page to open there first, with its query: the link's.
	let path: String?

	init(origin: URL, path: String? = nil) {
		self.origin = origin
		self.path = path
	}

	init?(_ text: String) {
		let text = text.trimmingCharacters(in: .whitespacesAndNewlines)
		guard !text.isEmpty, text.rangeOfCharacter(from: .whitespacesAndNewlines) == nil else { return nil }

		// Just a name: the relay's.
		if text.rangeOfCharacter(from: CharacterSet(charactersIn: ".:/")) == nil {
			let name = text.lowercased()
			guard Address.isRelayName(name), let url = URL(string: "https://\(name).\(Address.relayDomain)") else {
				return nil
			}
			self.init(origin: url)
			return
		}

		let withScheme = text.contains("://") ? text : "https://\(text)"
		guard let link = URLComponents(string: withScheme),
			let given = link.scheme?.lowercased(), given == "https" || given == "http",
			let host = link.host?.lowercased(), host.contains(".") || host == "localhost"
		else { return nil }
		var origin = URLComponents()
		origin.scheme = given == "http" && Address.isLocal(host) ? "http" : "https"
		origin.host = host
		origin.port = link.port
		guard let url = origin.url else { return nil }

		var path = link.percentEncodedPath
		if let query = link.percentEncodedQuery, !query.isEmpty { path += "?\(query)" }
		self.init(origin: url, path: Address.isPath(path) && path != "/" ? path : nil)
	}

	/// The address as people read it: `smiths.nolune.family`, and the scheme only when it's http.
	var display: String {
		Address.display(origin)
	}

	static func display(_ origin: URL) -> String {
		let host = origin.host ?? origin.absoluteString
		let port = origin.port.map { ":\($0)" } ?? ""
		return origin.scheme == "http" ? "http://\(host)\(port)" : "\(host)\(port)"
	}

	/// Whether the address is a name on the relay, rather than one of the family's own.
	var isOnRelay: Bool {
		origin.host?.hasSuffix(".\(Address.relayDomain)") == true
	}

	/// A name the relay gives (`isValidName` in packages/relay/src/protocol.ts).
	static func isRelayName(_ name: String) -> Bool {
		name.range(of: "^[a-z0-9][a-z0-9-]{1,30}[a-z0-9]$", options: .regularExpression) != nil
			&& !name.contains("--")
	}

	/// localhost, a `.local` name, or a private IPv4 address: a computer on this network.
	static func isLocal(_ host: String) -> Bool {
		if host == "localhost" || host.hasSuffix(".local") { return true }
		let parts = host.split(separator: ".", omittingEmptySubsequences: false)
		let numbers = parts.compactMap { UInt8($0) }
		guard parts.count == 4, numbers.count == 4 else { return false }
		switch (numbers[0], numbers[1]) {
		case (10, _), (127, _), (192, 168): return true
		case (172, 16...31): return true
		default: return false
		}
	}

	/// A path on the same address, never a way off it (`//elsewhere.com`).
	static func isPath(_ path: String) -> Bool {
		path.hasPrefix("/") && !path.hasPrefix("//") && !path.hasPrefix("/\\")
	}

	static func sameOrigin(_ a: URL, _ b: URL) -> Bool {
		a.scheme?.lowercased() == b.scheme?.lowercased()
			&& a.host?.lowercased() == b.host?.lowercased()
			&& port(of: a) == port(of: b)
	}

	private static func port(of url: URL) -> Int? {
		url.port ?? (url.scheme == "https" ? 443 : url.scheme == "http" ? 80 : nil)
	}
}

// MARK: Is there a nolune there?

extension Address {
	enum Problem: Error, Equatable {
		/// No such name on the relay, or no such address.
		case nothingThere
		/// It couldn't be reached: no network, or nothing answers.
		case unreachable(String)
		/// Something answered, with an error.
		case refused(Int)
	}

	/**
	 * Whether a nolune answers at the address: its sign-in page loads. The relay's page for a
	 * computer that's off or asleep counts, since it comes back by itself.
	 */
	func check(session: URLSession = .shared) async -> Problem? {
		var request = URLRequest(url: origin.appendingPathComponent("login"), timeoutInterval: 15)
		request.cachePolicy = .reloadIgnoringLocalCacheData
		do {
			let (_, response) = try await session.data(for: request)
			let status = (response as? HTTPURLResponse)?.statusCode ?? 0
			switch status {
			case 200..<400, 503: return nil
			case 404: return .nothingThere
			default: return .refused(status)
			}
		} catch let error as URLError where error.code == .cannotFindHost || error.code == .dnsLookupFailed {
			return .nothingThere
		} catch {
			return .unreachable(error.localizedDescription)
		}
	}

	func describe(_ problem: Problem) -> String {
		switch problem {
		case .nothingThere:
			return String(localized: "There's no nolune at \(display). Check the name with whoever set it up.")
		case .unreachable(let detail):
			return String(localized: "Couldn't reach \(display): \(detail)")
		case .refused(let status):
			return String(localized: "\(display) answered with an error (\(status)).")
		}
	}
}
