import Foundation

/**
 * `nolune --gateway`: what launchd runs in the background (Contents/Library/LaunchAgents). It
 * starts the gateway, `node cli.js start`, as its own child and waits on it.
 *
 * The app, not Node, is what launchd starts, so macOS counts the gateway and every command it
 * runs as the app's: one Full Disk Access switch, named nolune, covers them, and it stays on
 * across updates and Node versions.
 */
enum Gateway {
	static func run() -> Never {
		let runtime = Runtime.shared
		let log = openLog(runtime.logFile)

		let process = Process()
		process.executableURL = runtime.node
		process.arguments = runtime.cli + ["start"]
		process.environment = runtime.environment()
		try? FileManager.default.createDirectory(at: runtime.home, withIntermediateDirectories: true)
		process.currentDirectoryURL = runtime.home
		process.standardInput = FileHandle.nullDevice
		process.standardOutput = log
		process.standardError = log
		// Exits as the gateway does; launchd starts it again (KeepAlive).
		process.terminationHandler = { exit($0.terminationStatus) }

		// Stopping (launchctl, logout, a restart) passes on to the gateway, which stops what it runs.
		var sources: [DispatchSourceSignal] = []
		for sig in [SIGTERM, SIGINT, SIGHUP] {
			signal(sig, SIG_IGN)
			let source = DispatchSource.makeSignalSource(signal: sig, queue: .main)
			source.setEventHandler { if process.isRunning { kill(process.processIdentifier, sig) } }
			source.resume()
			sources.append(source)
		}

		do {
			try process.run()
		} catch {
			log.write(Data("nolune: couldn't start the gateway: \(error.localizedDescription)\n".utf8))
			exit(1)
		}
		withExtendedLifetime(sources) { dispatchMain() }
	}

	/// The gateway's log, where `nolune service logs` reads it, appended to.
	private static func openLog(_ url: URL) -> FileHandle {
		let manager = FileManager.default
		try? manager.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
		if !manager.fileExists(atPath: url.path) { _ = manager.createFile(atPath: url.path, contents: nil) }
		guard let handle = try? FileHandle(forWritingTo: url) else { return FileHandle.standardError }
		handle.seekToEndOfFile()
		return handle
	}
}
