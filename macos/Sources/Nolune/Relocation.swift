import AppKit

/**
 * Opened from the DMG, or from Downloads without moving it: macOS runs the app from a read-only
 * copy, and the gateway it registers would point there. It offers to move itself to Applications
 * first, and opens from there.
 */
enum Relocation {
	static var needed: Bool {
		guard Runtime.shared.isBundled else { return false }
		let path = Bundle.main.bundlePath
		return path.hasPrefix("/Volumes/") || path.contains("/AppTranslocation/")
	}

	@MainActor
	static func offer() {
		NSApp.setActivationPolicy(.regular)
		NSApp.activate(ignoringOtherApps: true)
		let alert = NSAlert()
		alert.messageText = "Move nolune to Applications?"
		alert.informativeText = "It runs in the background from now on, so it lives in your Applications folder."
		alert.addButton(withTitle: "Move to Applications")
		alert.addButton(withTitle: "Quit")
		guard alert.runModal() == .alertFirstButtonReturn else {
			NSApp.terminate(nil)
			return
		}

		let target = URL(fileURLWithPath: "/Applications/nolune.app")
		let manager = FileManager.default
		do {
			if manager.fileExists(atPath: target.path) {
				try manager.trashItem(at: target, resultingItemURL: nil)
			}
			try manager.copyItem(at: Bundle.main.bundleURL, to: target)
		} catch {
			let failed = NSAlert()
			failed.messageText = "nolune couldn't move itself."
			failed.informativeText = "Drag it into your Applications folder, then open it from there.\n\n\(error.localizedDescription)"
			failed.runModal()
			NSApp.terminate(nil)
			return
		}
		let configuration = NSWorkspace.OpenConfiguration()
		configuration.createsNewApplicationInstance = true
		NSWorkspace.shared.openApplication(at: target, configuration: configuration) { _, _ in
			DispatchQueue.main.async { NSApp.terminate(nil) }
		}
	}
}
