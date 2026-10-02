import AppKit
import SwiftUI

/**
 * `nolune --snapshot <folder>`: draws the intro at fixed moments (in the window, and bursting out
 * of it over a desktop), each screen after it, and the menu bar extra to PNGs, without doing
 * anything (no CLI, no service, no Full Disk Access checks). CI keeps them, to see the app without
 * running it.
 */
enum Snapshot {
	/// Set while snapshotting: the moment the sky and the intro are drawn at, in seconds.
	static var clock: Double?
	/// Whether the sky's stages follow the intro's timings, or stay as posed.
	static var intro = false
	static var active: Bool { clock != nil }

	@MainActor
	static func run(into folder: URL) -> Never {
		_ = NSApplication.shared
		NSApp.setActivationPolicy(.prohibited)
		Theme.registerFonts()
		try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
		Task { @MainActor in
			await render(into: folder)
			exit(0)
		}
		RunLoop.main.run()
		exit(0)
	}

	@MainActor
	private static func render(into folder: URL) async {
		let size = OnboardingView.size
		let moments: [(String, Double)] = [
			("01-gather", IntroTiming.bang - 0.15),
			("02-bang", IntroTiming.bang + 0.12),
			("03-burst", IntroTiming.bang + 0.45),
			("04-colors", IntroTiming.colors + 0.6),
			("05-dawn", IntroTiming.dawn + 0.4)
		]
		intro = true
		for (name, t) in moments {
			clock = t
			let onboarding = Onboarding()
			onboarding.pose(.intro)
			await save(OnboardingView(onboarding: onboarding), size: size, as: "intro-\(name)", in: folder)
		}

		// And out of the window, over a desktop: the same sparks at each moment.
		let screen = CGRect(x: 0, y: 0, width: 1440, height: 900)
		let window = CGRect(
			x: screen.midX - size.width / 2, y: screen.midY - size.height / 2,
			width: size.width, height: size.height
		)
		let scene = OutburstScene(window: window, screen: screen)
		let outside: [(String, Double)] = [
			("01-gather", IntroTiming.bang - 0.15),
			("02-bang", IntroTiming.bang + 0.1),
			("03-burst", IntroTiming.bang + 0.4),
			("04-sparks", IntroTiming.bang + 0.9),
			("05-embers", IntroTiming.bang + 1.6)
		]
		for (name, t) in outside {
			clock = t
			let onboarding = Onboarding()
			onboarding.pose(.intro, burstsOut: true)
			let desktop = Desktop(onboarding: onboarding, scene: scene, screen: screen, t: t)
			await save(desktop, size: screen.size, as: "desktop-\(name)", in: folder)
		}

		intro = false
		clock = 30
		let screens: [(String, @MainActor (Onboarding) -> Void)] = [
			("08-welcome", { $0.pose(.welcome) }),
			("09-account", {
				$0.pose(.account)
				$0.name = "Tim"
				$0.email = "tim@example.com"
				$0.password = "ember7-quartz-42abcd-nova9k"
			}),
			("10-address", {
				$0.pose(.address)
				$0.relay.pose(name: "smiths")
			}),
			("11-files", { $0.pose(.files) }),
			("12-files-granted", {
				$0.pose(.files, granted: true)
				$0.sky.bloom = 1
			}),
			("13-starting", { $0.pose(.service) }),
			("14-ready", {
				$0.pose(.service, service: .ready)
				$0.relay.pose(name: "smiths", url: "https://smiths.\(RelaySetup.domain)")
			})
		]
		for (name, pose) in screens {
			let onboarding = Onboarding()
			pose(onboarding)
			await save(OnboardingView(onboarding: onboarding), size: size, as: name, in: folder)
		}

		// A stand-in for a profile picture: a moon rising in a dusk sky.
		let picture = NSImage(size: NSSize(width: 64, height: 64), flipped: false) { rect in
			NSGradient(starting: NSColor(Color(hex: 0xFE9042)), ending: NSColor(Color(hex: 0x3B2A6B)))?
				.draw(in: rect, angle: 90)
			NSColor(Color(hex: 0xFCEDD2)).setFill()
			NSBezierPath(ovalIn: NSRect(x: 30, y: 30, width: 20, height: 20)).fill()
			return true
		}
		let people = [
			Runtime.Person(name: "Tim", email: "tim@example.com", isAdmin: true),
			Runtime.Person(name: "Anna", email: "anna@example.com", isAdmin: false, picture: picture),
			Runtime.Person(name: "Grandma", email: "grandma@example.com", isAdmin: false)
		]
		// The menu follows the system's appearance: both. Dark with the relay's address, light with
		// this Mac's and the way to get one.
		let relayed = GatewayStatus()
		relayed.pose(running: true, people: people, relay: URL(string: "https://smiths.\(RelaySetup.domain)"))
		await save(
			StatusMenu(status: relayed).background(Color(hex: 0x2A2A2A)).environment(\.colorScheme, .dark),
			size: nil, as: "15-menu-dark", in: folder
		)
		let local = GatewayStatus()
		local.pose(running: true, people: people)
		await save(
			StatusMenu(status: local).background(Color(hex: 0xF2F2F2)).environment(\.colorScheme, .light),
			size: nil, as: "16-menu-light", in: folder
		)
		// With a new release out: the row that downloads it.
		let outdated = GatewayStatus()
		outdated.pose(
			running: true, people: people,
			relay: URL(string: "https://smiths.\(RelaySetup.domain)"),
			update: Runtime.Update(
				version: "0.4.0",
				page: URL(string: "https://github.com/triangle-int/nolune/releases/tag/v0.4.0")!,
				download: URL(string: "https://github.com/triangle-int/nolune/releases/download/v0.4.0/nolune-macos-apple-silicon.dmg")
			)
		)
		await save(
			StatusMenu(status: outdated).background(Color(hex: 0x2A2A2A)).environment(\.colorScheme, .dark),
			size: nil, as: "17-menu-update-dark", in: folder
		)
	}

	/**
	 * Twice: with ImageRenderer, which draws SwiftUI faithfully but leaves AppKit controls (text
	 * fields, spinners) out, and from an offscreen window, which has those.
	 */
	@MainActor
	private static func save<V: View>(_ view: V, size: CGSize?, as name: String, in folder: URL) async {
		print("snapshot: \(name)")
		fflush(stdout)
		let hosting = NSHostingView(rootView: view)
		let frame = CGRect(origin: .zero, size: size ?? hosting.fittingSize)
		hosting.frame = frame
		let window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
		// Swift owns it: closed, AppKit would release it a second time.
		window.isReleasedWhenClosed = false
		window.appearance = NSAppearance(named: name.hasSuffix("-light") ? .aqua : .darkAqua)
		window.contentView = hosting
		hosting.layoutSubtreeIfNeeded()
		try? await Task.sleep(nanoseconds: 500_000_000)

		let renderer = ImageRenderer(content: view.frame(width: frame.width, height: frame.height))
		renderer.scale = 2
		if let image = renderer.cgImage {
			write(NSBitmapImageRep(cgImage: image), to: folder.appendingPathComponent("\(name).png"))
		}
		if let rep = hosting.bitmapImageRepForCachingDisplay(in: hosting.bounds) {
			hosting.cacheDisplay(in: hosting.bounds, to: rep)
			write(rep, to: folder.appendingPathComponent("\(name)-window.png"))
		}
		window.close()
	}

	private static func write(_ rep: NSBitmapImageRep, to url: URL) {
		try? rep.representation(using: .png, properties: [:])?.write(to: url)
	}

	/// The onboarding window in the middle of a stand-in desktop, and the outburst over them.
	private struct Desktop: View {
		let onboarding: Onboarding
		let scene: OutburstScene
		let screen: CGRect
		let t: Double

		var body: some View {
			ZStack {
				LinearGradient(
					colors: [Color(hex: 0x23406E), Color(hex: 0x5B4B8A), Color(hex: 0xC98B73)],
					startPoint: .topLeading, endPoint: .bottomTrailing
				)
				OnboardingView(onboarding: onboarding)
					.frame(width: OnboardingView.size.width, height: OnboardingView.size.height)
					.clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
					.shadow(color: .black.opacity(0.45), radius: 30, y: 14)
				Canvas { context, _ in
					scene.draw(&context, screen: screen, t: t)
				}
			}
			.frame(width: screen.width, height: screen.height)
		}
	}

	/// The sky as it would be at `clock`: a fresh one, run up to then frame by frame.
	static func drawSky(_ posed: Sky, _ context: inout GraphicsContext, size: CGSize, clock: Double, intro: Bool) {
		let sky = Sky()
		sky.still = posed.still
		sky.space = posed.space
		sky.stage = posed.stage
		sky.bloom = posed.bloom
		let layout = IntroLayout(size: size)
		var t = intro ? 0 : max(0, clock - 0.2)
		while t < clock {
			if intro { stage(sky, at: t, layout: layout) }
			sky.advance(size: size, now: t)
			t += 1.0 / 30
		}
		if intro { stage(sky, at: clock, layout: layout) }
		sky.draw(&context, size: size, now: clock)
	}

	/// The sky's stages at `t` into the intro, as Onboarding.startIntro sets them.
	private static func stage(_ sky: Sky, at t: Double, layout: IntroLayout) {
		if t >= IntroTiming.dawn { sky.space = false }
		if t >= IntroTiming.colors {
			sky.center = layout.center
			sky.stage = .aurora
		} else if t >= IntroTiming.bang {
			sky.burst = true
			sky.stage = .stars
		} else {
			sky.stage = .dark
		}
	}
}
