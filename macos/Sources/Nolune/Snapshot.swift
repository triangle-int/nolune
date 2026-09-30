import AppKit
import SwiftUI

/**
 * `nolune --snapshot <folder>`: draws the intro at fixed moments, each screen after it, and the
 * menu bar extra to PNGs, without doing anything (no CLI, no service, no Full Disk Access checks).
 * CI keeps them, to see the app without running it.
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
			("01-stars", 3.5),
			("02-star", IntroTiming.draw + 0.5),
			("03-sweep", IntroTiming.draw + 1.9),
			("04-typing", IntroTiming.typing + 0.3),
			("05-burst", IntroTiming.orbit + 0.35),
			("06-orbit", IntroTiming.orbit + 4),
			("07-aurora", IntroTiming.aurora + 1.2)
		]
		intro = true
		for (name, t) in moments {
			clock = t
			let onboarding = Onboarding()
			onboarding.pose(.intro)
			await save(OnboardingView(onboarding: onboarding), size: size, as: "intro-\(name)", in: folder)
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
			("10-files", { $0.pose(.files) }),
			("11-files-granted", {
				$0.pose(.files, granted: true)
				$0.sky.bloom = 1
			}),
			("12-starting", { $0.pose(.service) }),
			("13-login-items", { $0.pose(.service, service: .needsApproval) }),
			("14-ready", { $0.pose(.service, service: .ready) })
		]
		for (name, pose) in screens {
			let onboarding = Onboarding()
			pose(onboarding)
			await save(OnboardingView(onboarding: onboarding), size: size, as: name, in: folder)
		}

		let status = GatewayStatus()
		status.pose(
			running: true,
			people: [
				Runtime.Person(name: "Tim", email: "tim@example.com", isAdmin: true),
				Runtime.Person(name: "Anna", email: "anna@example.com", isAdmin: false),
				Runtime.Person(name: "Grandma", email: "grandma@example.com", isAdmin: false)
			]
		)
		let menu = StatusMenu(status: status)
			.background(Color(hex: 0x2A2A2A))
			.preferredColorScheme(.dark)
		await save(menu, size: nil, as: "15-menu", in: folder)
	}

	/**
	 * Twice: with ImageRenderer, which draws SwiftUI faithfully but leaves AppKit controls (text
	 * fields, spinners) out, and from an offscreen window, which has those.
	 */
	@MainActor
	private static func save<V: View>(_ view: V, size: CGSize?, as name: String, in folder: URL) async {
		let hosting = NSHostingView(rootView: view)
		let frame = CGRect(origin: .zero, size: size ?? hosting.fittingSize)
		hosting.frame = frame
		let window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
		window.appearance = NSAppearance(named: .darkAqua)
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

	/// The sky as it would be at `clock`: a fresh one, run up to then frame by frame.
	static func drawSky(_ posed: Sky, _ context: inout GraphicsContext, size: CGSize, clock: Double, intro: Bool) {
		let sky = Sky()
		sky.still = posed.still
		sky.space = posed.space
		sky.stage = posed.stage
		sky.bloom = posed.bloom
		let layout = IntroLayout(size: size)
		var unseen = context
		unseen.clip(to: Path())
		var t = intro ? 0 : max(0, clock - 0.2)
		while t < clock {
			if intro { stage(sky, at: t, layout: layout) }
			sky.draw(&unseen, size: size, now: t)
			t += 1.0 / 30
		}
		if intro { stage(sky, at: clock, layout: layout) }
		sky.draw(&context, size: size, now: clock)
	}

	/// The sky's stages at `t` into the intro, as Onboarding.startIntro sets them.
	private static func stage(_ sky: Sky, at t: Double, layout: IntroLayout) {
		if t >= IntroTiming.dawn { sky.space = false }
		if t >= IntroTiming.aurora {
			sky.stage = .aurora
		} else if t >= IntroTiming.orbit {
			sky.origins = layout.dotCenters
			sky.center = layout.center
			sky.stage = .orbit
		} else {
			sky.stage = .stars
		}
	}
}
