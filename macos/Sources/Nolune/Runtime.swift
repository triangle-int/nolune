import Foundation

/**
 * Where nolune lives: the Node binary and the npm package bundled in the app, and the data in
 * `~/.nolune` (or `NOLUNE_HOME`), shared with a `nolune` installed from npm.
 *
 * Run outside an app bundle (`swift run` in a checkout), it uses the checkout's CLI from source
 * and the first `node` it finds, so the onboarding can be worked on without building the app.
 */
final class Runtime: @unchecked Sendable {
	static let shared = Runtime()

	let home: URL
	/// Node and the arguments that run the CLI before its own: `node <cli.js>`.
	let node: URL
	let cli: [String]

	private init() {
		let environment = ProcessInfo.processInfo.environment
		if let custom = environment["NOLUNE_HOME"], !custom.isEmpty {
			home = URL(fileURLWithPath: custom, isDirectory: true)
		} else {
			home = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".nolune")
		}

		let bundled = Bundle.main.url(forAuxiliaryExecutable: "node")
		let package = Bundle.main.resourceURL?.appendingPathComponent("app/node_modules/nolune")
		if let bundled, let package, FileManager.default.fileExists(atPath: bundled.path) {
			node = bundled
			cli = [package.appendingPathComponent("dist/cli.js").path]
		} else {
			// A checkout: macos/Sources/Nolune/Runtime.swift is four levels down.
			let checkout = URL(fileURLWithPath: #filePath)
				.deletingLastPathComponent().deletingLastPathComponent()
				.deletingLastPathComponent().deletingLastPathComponent()
			node = Runtime.findNode(environment)
			cli = ["--no-warnings", checkout.appendingPathComponent("packages/cli/src/index.ts").path]
		}
	}

	var isBundled: Bool { Bundle.main.bundleURL.pathExtension == "app" }
	var logFile: URL { home.appendingPathComponent("logs/gateway.log") }

	/// Host, port and public URL, as `nolune setup` and `nolune config` keep them, the relay's
	/// address, as `nolune relay enable` does, and whether the gateway looks for new releases
	/// (`nolune config set update-check`).
	struct Config: Decodable {
		var host: String?
		var port: Int?
		var origin: String?
		var relay: Relay?
		var updateCheck: Bool?

		struct Relay: Decodable {
			var url: String
		}
	}

	var config: Config {
		guard let data = try? Data(contentsOf: home.appendingPathComponent("config.json")),
			let config = try? JSONDecoder().decode(Config.self, from: data)
		else { return Config() }
		return config
	}

	static let defaultPort = 5780
	var port: Int { config.port ?? Runtime.defaultPort }

	/// Where people open it: the relay's address while there is one, else the configured public
	/// URL, else this computer (`publicOrigin` in packages/core/src/config.ts).
	var origin: URL {
		let config = self.config
		if let relay = config.relay, let url = URL(string: relay.url) { return url }
		if let origin = config.origin, let url = URL(string: origin) { return url }
		return localURL
	}

	/// The gateway on this computer, whatever the public URL: for checking it's up.
	var localURL: URL { URL(string: "http://localhost:\(port)")! }

	/// This app's version, from its Info.plist. Nil from `swift run`, which has none.
	static var version: String? {
		guard let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String,
			version.first?.isNumber == true
		else { return nil }
		return version
	}

	/// A newer nolune than this app: its version, its page with the notes, and this Mac's disk image.
	struct Update: Equatable {
		var version: String
		var page: URL
		var download: URL?
	}

	/// `latest-release.json`, which the gateway writes once a day (packages/core/src/updates.ts).
	private struct SavedRelease: Decodable {
		var latest: Release?

		struct Release: Decodable {
			var version: String
			var url: String
			/// The disk images, by Node's names for the architecture: arm64 and x64.
			var downloads: [String: String]?
		}
	}

	/// Where the release's links have to point, as the gateway checks too: the app opens them.
	private static let releases = "https://github.com/triangle-int/nolune/releases/"

	/// The newest release, as the gateway last heard from GitHub, when it's newer than this app.
	var update: Update? {
		guard config.updateCheck != false, let current = Runtime.version,
			let data = try? Data(contentsOf: home.appendingPathComponent("latest-release.json")),
			let latest = (try? JSONDecoder().decode(SavedRelease.self, from: data))?.latest,
			Runtime.isNewer(latest.version, than: current),
			let page = Runtime.releaseLink(latest.url)
		else { return nil }
		#if arch(arm64)
		let arch = "arm64"
		#else
		let arch = "x64"
		#endif
		let download: URL? = latest.downloads?[arch].flatMap { Runtime.releaseLink($0) }
		return Update(version: latest.version, page: page, download: download)
	}

	private static func releaseLink(_ text: String) -> URL? {
		text.hasPrefix(releases) ? URL(string: text) : nil
	}

	/// Whether version `a` comes after `b`, both `major.minor.patch`: 0.10.0 is after 0.9.0.
	static func isNewer(_ a: String, than b: String) -> Bool {
		a.compare(b, options: .numeric) == .orderedDescending
	}

	/// The environment the CLI and the gateway run with.
	func environment() -> [String: String] {
		var environment = ProcessInfo.processInfo.environment
		environment["NOLUNE_HOME"] = home.path
		return environment
	}

	struct Output {
		var status: Int32
		var stdout: String
		var stderr: String
		var succeeded: Bool { status == 0 }

		/// What went wrong, as the CLI said it (`nolune: ...`), for showing people.
		var problem: String {
			let text = (stderr.isEmpty ? stdout : stderr).trimmingCharacters(in: .whitespacesAndNewlines)
			let line = text.split(separator: "\n").last.map(String.init) ?? "Something went wrong."
			return line.hasPrefix("nolune: ") ? String(line.dropFirst(8)) : line
		}
	}

	/// Runs `nolune <arguments>` and waits for it, off the main thread.
	func run(_ arguments: [String]) async -> Output {
		await withCheckedContinuation { continuation in
			DispatchQueue.global(qos: .userInitiated).async {
				continuation.resume(returning: self.runNow(arguments))
			}
		}
	}

	private func runNow(_ arguments: [String]) -> Output {
		let process = Process()
		process.executableURL = node
		process.arguments = cli + arguments
		process.environment = environment()
		let out = Pipe()
		let err = Pipe()
		process.standardOutput = out
		process.standardError = err
		process.standardInput = FileHandle.nullDevice
		do {
			try process.run()
		} catch {
			return Output(status: -1, stdout: "", stderr: "Couldn't start Node: \(error.localizedDescription)")
		}
		// Read both at once, so neither pipe fills up and stalls the other.
		let stderr = Collected()
		let group = DispatchGroup()
		group.enter()
		DispatchQueue.global().async {
			stderr.data = err.fileHandleForReading.readDataToEndOfFile()
			group.leave()
		}
		let stdoutData = out.fileHandleForReading.readDataToEndOfFile()
		group.wait()
		process.waitUntilExit()
		return Output(
			status: process.terminationStatus,
			stdout: String(decoding: stdoutData, as: UTF8.self),
			stderr: String(decoding: stderr.data, as: UTF8.self)
		)
	}

	private final class Collected {
		var data = Data()
	}

	/// The people with accounts, from `nolune user list` (`name<TAB>email[<TAB>admin]`).
	struct Person: Identifiable {
		var name: String
		var email: String
		var isAdmin: Bool
		var id: String { email }
	}

	func people() async -> [Person]? {
		let output = await run(["user", "list"])
		guard output.succeeded else { return nil }
		return output.stdout.split(separator: "\n").compactMap { line in
			let fields = line.split(separator: "\t").map(String.init)
			guard fields.count >= 2 else { return nil }
			return Person(name: fields[0], email: fields[1], isAdmin: fields.dropFirst(2).contains("admin"))
		}
	}

	private static func findNode(_ environment: [String: String]) -> URL {
		if let custom = environment["NOLUNE_NODE"], !custom.isEmpty { return URL(fileURLWithPath: custom) }
		let path = (environment["PATH"] ?? "").split(separator: ":").map(String.init)
		for dir in path + ["/opt/homebrew/bin", "/usr/local/bin"] {
			let candidate = URL(fileURLWithPath: dir).appendingPathComponent("node")
			if FileManager.default.isExecutableFile(atPath: candidate.path) { return candidate }
		}
		return URL(fileURLWithPath: "/usr/local/bin/node")
	}
}
