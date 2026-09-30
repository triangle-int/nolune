import AppKit
import SwiftUI

/// One executable, three jobs: the app people open, the gateway's supervisor that launchd runs in
/// the background (`--gateway`), and a one-shot Full Disk Access check (`--probe-disk-access`).
@main
enum Main {
	static func main() {
		let arguments = CommandLine.arguments
		if arguments.contains("--gateway") { Gateway.run() }
		if arguments.contains("--probe-disk-access") { exit(DiskAccess.canReadProtectedFiles() ? 0 : 1) }
		NoluneApp.main()
	}
}

struct NoluneApp: App {
	@NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
	@AppStorage(Onboarding.doneKey) private var onboarded = false

	var body: some Scene {
		// The menu bar extra comes in once the onboarding is done; the onboarding's own window is
		// AppKit's (AppDelegate), so the app can open without one.
		MenuBarExtra(isInserted: $onboarded) {
			StatusMenu(status: delegate.status)
		} label: {
			Image(nsImage: MenuIcon.image)
		}
		.menuBarExtraStyle(.window)
	}
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
	let status: GatewayStatus
	private var window: NSWindow?
	private var closing: NSObjectProtocol?

	override init() {
		status = GatewayStatus()
		super.init()
	}

	func applicationDidFinishLaunching(_ notification: Notification) {
		Theme.registerFonts()
		if Relocation.needed {
			Relocation.offer()
			return
		}
		if !UserDefaults.standard.bool(forKey: Onboarding.doneKey) { showOnboarding() }
	}

	/// Opened again (from Finder, Launchpad or Spotlight) once it's set up: to nolune in the browser.
	func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
		if window == nil, UserDefaults.standard.bool(forKey: Onboarding.doneKey) {
			NSWorkspace.shared.open(Runtime.shared.origin)
		}
		return true
	}

	func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

	private func showOnboarding() {
		let onboarding = Onboarding()
		onboarding.onFinish = { [weak self] in self?.closeOnboarding() }
		let window = NSWindow(
			contentRect: NSRect(origin: .zero, size: OnboardingView.size),
			styleMask: [.titled, .closable, .miniaturizable, .fullSizeContentView],
			backing: .buffered,
			defer: false
		)
		window.titlebarAppearsTransparent = true
		window.titleVisibility = .hidden
		window.isMovableByWindowBackground = true
		window.appearance = NSAppearance(named: .darkAqua)
		window.backgroundColor = NSColor(Theme.space)
		window.isReleasedWhenClosed = false
		window.contentView = NSHostingView(rootView: OnboardingView(onboarding: onboarding))
		window.center()
		self.window = window

		// A regular app while it asks questions; a menu bar one after.
		NSApp.setActivationPolicy(.regular)
		window.makeKeyAndOrderFront(nil)
		NSApp.activate(ignoringOtherApps: true)
		closing = NotificationCenter.default.addObserver(
			forName: NSWindow.willCloseNotification, object: window, queue: .main
		) { [weak self] _ in
			Task { @MainActor in self?.windowClosed() }
		}
	}

	private func closeOnboarding() {
		window?.close()
	}

	private func windowClosed() {
		window = nil
		NSApp.setActivationPolicy(.accessory)
		// Closed halfway: nothing runs yet, so there's nothing to keep the app open for.
		if !UserDefaults.standard.bool(forKey: Onboarding.doneKey) { NSApp.terminate(nil) }
	}
}
