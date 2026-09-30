import AppKit

/**
 * Full Disk Access. macOS has no way to ask for it: people turn it on in System Settings. What
 * the app can do is take them to the right list, show up in it, and notice the moment it's on.
 */
enum DiskAccess {
	/// The privacy database: only a process with Full Disk Access can open it.
	private static var protectedFile: URL {
		FileManager.default.homeDirectoryForCurrentUser
			.appendingPathComponent("Library/Application Support/com.apple.TCC/TCC.db")
	}

	static func canReadProtectedFiles() -> Bool {
		guard let handle = try? FileHandle(forReadingFrom: protectedFile) else { return false }
		try? handle.close()
		return true
	}

	/**
	 * Checks in a new process each time (`--probe-disk-access`): a running process may not see a
	 * grant until it's relaunched, and System Settings' "Quit & Reopen" would cut the onboarding
	 * short. Children count as the app, so the answer is the app's.
	 */
	static func isGranted() async -> Bool {
		guard let executable = Bundle.main.executableURL else { return canReadProtectedFiles() }
		return await withCheckedContinuation { continuation in
			let process = Process()
			process.executableURL = executable
			process.arguments = ["--probe-disk-access"]
			process.standardOutput = FileHandle.nullDevice
			process.standardError = FileHandle.nullDevice
			process.terminationHandler = { continuation.resume(returning: $0.terminationStatus == 0) }
			do {
				try process.run()
			} catch {
				continuation.resume(returning: false)
			}
		}
	}

	/**
	 * Trying a protected file is what usually adds the app to the Full Disk Access list, switched
	 * off, so there's a switch to flip. Usually: it's how macOS behaves, not something it promises,
	 * so the onboarding also offers the app's icon to drag into the list.
	 */
	static func appearInList() {
		_ = canReadProtectedFiles()
	}

	static func openSettings() {
		let pane = "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"
		if let url = URL(string: pane) { NSWorkspace.shared.open(url) }
	}
}
