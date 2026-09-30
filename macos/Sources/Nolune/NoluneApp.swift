import AppKit
import SwiftUI

/// One executable, three jobs: the app people open, the gateway's keeper that the app runs as its
/// child (`--gateway`), and a one-shot Full Disk Access check (`--probe-disk-access`).
/// And `--snapshot <folder>`, which draws the screens to PNGs (Snapshot.swift).
@main
enum Main {
	static func main() {
		let arguments = CommandLine.arguments
		if arguments.contains("--gateway") { Gateway.run() }
		if arguments.contains("--probe-disk-access") { exit(DiskAccess.canReadProtectedFiles() ? 0 : 1) }
		if let flag = arguments.firstIndex(of: "--snapshot"), flag + 1 < arguments.count {
			Snapshot.run(into: URL(fileURLWithPath: arguments[flag + 1], isDirectory: true))
		}
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
			StatusMenu(status: delegate.status, openFromAnywhere: { delegate.showAddress() })
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
		// The gateway runs while the app does: from the onboarding's last step the first time.
		if UserDefaults.standard.bool(forKey: Onboarding.doneKey) { Service.start() } else { showOnboarding() }
	}

	func applicationWillTerminate(_ notification: Notification) {
		Service.stop()
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
		present(NSHostingView(rootView: OnboardingView(onboarding: onboarding)))
		// Set before the intro starts, which waits for this to return.
		if let window { onboarding.outburst = Outburst(around: window) }
	}

	/// A window of the onboarding's size and sky, with `content` in it.
	private func present(_ content: NSView) {
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
		window.contentView = content
		// The content goes under the title bar, so the design's size is the whole window's.
		window.setFrame(NSRect(origin: .zero, size: OnboardingView.size), display: false)
		window.center()
		self.window = window

		// A regular app while it asks questions; a menu bar one after.
		NSApp.setActivationPolicy(.regular)
		window.makeKeyAndOrderFront(nil)
		NSApp.activate(ignoringOtherApps: true)
		if let closing { NotificationCenter.default.removeObserver(closing) }
		closing = NotificationCenter.default.addObserver(
			forName: NSWindow.willCloseNotification, object: window, queue: .main
		) { [weak self] _ in
			Task { @MainActor in self?.windowClosed() }
		}
	}

	private func closeOnboarding() {
		window?.close()
	}

	/**
	 * The onboarding's address step on its own, from the menu bar: for a Mac set up without the
	 * relay. Once there's an address, the gateway starts again to connect to it.
	 */
	func showAddress() {
		if let window {
			window.makeKeyAndOrderFront(nil)
			NSApp.activate(ignoringOtherApps: true)
			return
		}
		let relay = RelaySetup()
		// The onboarding's sky, as it is behind its questions.
		let sky = Sky()
		sky.still = true
		sky.space = false
		sky.stage = .aurora
		let screen = ZStack {
			SkyView(sky: sky)
				.opacity(0.6)
			AddressScreen(
				relay: relay,
				enable: { [weak self] in
					guard await relay.enable() else { return }
					Service.restart()
					self?.window?.close()
				},
				skipTitle: "Not now",
				skip: { [weak self] in self?.window?.close() }
			)
		}
		.frame(width: OnboardingView.size.width, height: OnboardingView.size.height)
		.background(Theme.space)
		.ignoresSafeArea()
		.preferredColorScheme(.dark)
		present(NSHostingView(rootView: screen))
	}

	private func windowClosed() {
		window = nil
		NSApp.setActivationPolicy(.accessory)
		// Closed halfway: nothing runs yet, so there's nothing to keep the app open for.
		if !UserDefaults.standard.bool(forKey: Onboarding.doneKey) { NSApp.terminate(nil) }
	}
}
