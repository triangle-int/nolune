import AppKit
import ServiceManagement

/**
 * The gateway runs with the app: the app starts `Nolune --gateway` (Gateway.swift), its own
 * executable, as its child when it opens, and stops it when it quits. The app opens at login, so
 * nolune is up whenever it's in the menu bar, and only then. As the app's child the gateway counts
 * as the app for Full Disk Access.
 */
@MainActor
enum Service {
	static let label = "dev.nolune.gateway"
	private static var keeper: Process?

	/// Starts the gateway, or, running already, has it start again now.
	static func start() {
		// Run from a checkout (`swift run`) the gateway is yours: `nolune start` or `pnpm dev`.
		guard Runtime.shared.isBundled, let executable = Bundle.main.executableURL else { return }
		if let keeper, keeper.isRunning { return restart() }
		clearLaunchAgents()
		let process = Process()
		process.executableURL = executable
		process.arguments = ["--gateway"]
		process.standardInput = FileHandle.nullDevice
		do {
			try process.run()
			keeper = process
		} catch {
			NSLog("nolune: couldn't start the gateway: \(error.localizedDescription)")
		}
	}

	static func restart() {
		guard let keeper, keeper.isRunning else { return start() }
		kill(keeper.processIdentifier, SIGHUP)
	}

	/// When the app quits. Waits a few seconds for the gateway to stop, so the app opened again
	/// finds its port free.
	static func stop() {
		guard let keeper, keeper.isRunning else { return }
		keeper.terminate()
		let deadline = Date().addingTimeInterval(5)
		while keeper.isRunning, Date() < deadline { usleep(50_000) }
	}

	/// The app opens at login, and the gateway with it. People can turn it off under Login Items.
	static func openAtLogin() {
		if SMAppService.mainApp.status != .enabled { try? SMAppService.mainApp.register() }
	}

	/**
	 * A LaunchAgent with the gateway's label, from `nolune service install` or an earlier build of
	 * the app, would run a second gateway that fights this one for the port: the app's takes over.
	 */
	private static func clearLaunchAgents() {
		let target = "gui/\(getuid())/\(label)"
		if launchctl(["print", target]) == 0 { launchctl(["bootout", target]) }
		let plist = FileManager.default.homeDirectoryForCurrentUser
			.appendingPathComponent("Library/LaunchAgents/\(label).plist")
		try? FileManager.default.removeItem(at: plist)
	}

	@discardableResult
	private static func launchctl(_ arguments: [String]) -> Int32 {
		let process = Process()
		process.executableURL = URL(fileURLWithPath: "/bin/launchctl")
		process.arguments = arguments
		process.standardOutput = FileHandle.nullDevice
		process.standardError = FileHandle.nullDevice
		do {
			try process.run()
			process.waitUntilExit()
			return process.terminationStatus
		} catch {
			return -1
		}
	}

	/// Whether the gateway answers on this computer: any HTTP response will do.
	nonisolated static func isUp() async -> Bool {
		var request = URLRequest(url: Runtime.shared.localURL)
		request.timeoutInterval = 2
		request.cachePolicy = .reloadIgnoringLocalCacheData
		guard let answer = try? await URLSession.shared.data(for: request) else { return false }
		return answer.1 is HTTPURLResponse
	}

	/// Waits for the gateway to answer, up to `timeout` seconds.
	nonisolated static func waitUntilUp(timeout: TimeInterval = 45) async -> Bool {
		let deadline = Date().addingTimeInterval(timeout)
		while Date() < deadline {
			if await isUp() { return true }
			try? await Task.sleep(nanoseconds: 700_000_000)
		}
		return false
	}
}
