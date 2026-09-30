import Foundation
import ServiceManagement

/**
 * The gateway in the background: a LaunchAgent inside the app
 * (Contents/Library/LaunchAgents/dev.nolune.gateway.plist) that runs `nolune --gateway` at login
 * and keeps it running. Registered with SMAppService, so it shows as nolune in Login Items.
 *
 * It keeps the label `nolune service install` uses, so `nolune service restart|status|logs` and
 * the agent restarting itself work the same with the app.
 */
enum Service {
	static let label = "dev.nolune.gateway"
	static var agent: SMAppService { .agent(plistName: "\(label).plist") }

	enum State {
		case running
		/// Turned off under Login Items: people have to allow it there.
		case needsApproval
		case failed(String)
	}

	/// Starts the gateway now and at every login, and the menu bar extra with it.
	static func start() -> State {
		removeCommandLineAgent()
		do {
			if agent.status != .enabled { try agent.register() }
		} catch {
			if agent.status == .requiresApproval { return .needsApproval }
			return .failed(error.localizedDescription)
		}
		// The menu bar extra comes back at login too. Not needed for the gateway, so no fuss.
		if SMAppService.mainApp.status != .enabled { try? SMAppService.mainApp.register() }
		return agent.status == .requiresApproval ? .needsApproval : .running
	}

	static func openLoginItems() {
		SMAppService.openSystemSettingsLoginItems()
	}

	static func restart() {
		launchctl(["kickstart", "-k", "gui/\(getuid())/\(label)"])
	}

	/**
	 * `nolune service install` from an npm install left its own LaunchAgent with the same label.
	 * The app's takes over: two would fight over the port.
	 */
	private static func removeCommandLineAgent() {
		let plist = FileManager.default.homeDirectoryForCurrentUser
			.appendingPathComponent("Library/LaunchAgents/\(label).plist")
		guard FileManager.default.fileExists(atPath: plist.path) else { return }
		launchctl(["bootout", "gui/\(getuid())/\(label)"])
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
	static func isUp() async -> Bool {
		var request = URLRequest(url: Runtime.shared.localURL)
		request.timeoutInterval = 2
		request.cachePolicy = .reloadIgnoringLocalCacheData
		guard let answer = try? await URLSession.shared.data(for: request) else { return false }
		return answer.1 is HTTPURLResponse
	}

	/// Waits for the gateway to answer, up to `timeout` seconds.
	static func waitUntilUp(timeout: TimeInterval = 45) async -> Bool {
		let deadline = Date().addingTimeInterval(timeout)
		while Date() < deadline {
			if await isUp() { return true }
			try? await Task.sleep(nanoseconds: 700_000_000)
		}
		return false
	}
}
