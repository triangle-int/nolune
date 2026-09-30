import Foundation

/**
 * Where nolune lives: the Node binary and the npm package bundled in the app, and the data in
 * `~/.nolune` (or `NOLUNE_HOME`), shared with a `nolune` installed from npm.
 *
 * Run outside an app bundle (`swift run` in a checkout), it uses the checkout's CLI from source
 * and the first `node` it finds, so the onboarding can be worked on without building the app.
 */
final class Runtime {
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

	/// Host, port and public URL, as `nolune setup` and `nolune config` keep them.
	struct Config: Decodable {
		var host: String?
		var port: Int?
		var origin: String?
	}

	var config: Config {
		guard let data = try? Data(contentsOf: home.appendingPathComponent("config.json")),
			let config = try? JSONDecoder().decode(Config.self, from: data)
		else { return Config() }
		return config
	}

	static let defaultPort = 5780
	var port: Int { config.port ?? Runtime.defaultPort }

	/// Where people open it: the configured public URL, else this computer.
	var origin: URL {
		if let origin = config.origin, let url = URL(string: origin) { return url }
		return localURL
	}

	/// The gateway on this computer, whatever the public URL: for checking it's up.
	var localURL: URL { URL(string: "http://localhost:\(port)")! }

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
