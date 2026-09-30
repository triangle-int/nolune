import AppKit
import SwiftUI

/**
 * The big bang, out of the window. While the point of light gathers, the desktop dims around the
 * window and specks of light stream in from all over the screen, spiralling into it. When it
 * bursts, the window takes the kick, the flash lights up the room, the shock waves roll out across
 * the screen, and sparks fly out past the window's edges, cooling and falling as they burn out.
 *
 * Drawn on a see-through window over each screen, above the onboarding window, that clicks pass
 * through. The onboarding window draws the gathering light itself (IntroView) and leaves the rest
 * to this.
 */
@MainActor
final class Outburst {
	/// How long the window rocks after the bang, in seconds.
	private static let kick = 0.6

	private weak var window: NSWindow?
	private var overlays: [NSWindow] = []
	private var rumbling: Task<Void, Never>?
	private var finishing: Task<Void, Never>?

	init(around window: NSWindow) {
		self.window = window
	}

	/**
	 * Puts the see-through windows up, drawing from the intro's `start`. False when there's no
	 * screen to draw on: the onboarding window draws the whole bang itself then.
	 */
	func begin(at start: Date) -> Bool {
		guard overlays.isEmpty, let window, window.isVisible, let home = window.screen else { return false }
		let scene = OutburstScene(window: window.frame, screen: home.frame)
		overlays = NSScreen.screens.map { screen in
			let overlay = NSWindow(contentRect: screen.frame, styleMask: [.borderless], backing: .buffered, defer: false)
			overlay.isOpaque = false
			overlay.backgroundColor = .clear
			overlay.hasShadow = false
			overlay.ignoresMouseEvents = true
			// Over the menu bar and the Dock too.
			overlay.level = .statusBar
			overlay.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]
			overlay.isExcludedFromWindowsMenu = true
			overlay.animationBehavior = .none
			// Swift owns it: closed, AppKit would release it a second time.
			overlay.isReleasedWhenClosed = false
			overlay.contentView = NSHostingView(rootView: OutburstView(scene: scene, screen: screen.frame, start: start))
			overlay.setFrame(screen.frame, display: false)
			overlay.orderFrontRegardless()
			return overlay
		}
		rumble(scene, from: start)
		finishing = Task { [weak self] in
			let left = start.addingTimeInterval(OutburstScene.length).timeIntervalSinceNow
			try? await Task.sleep(nanoseconds: UInt64(max(0, left) * 1_000_000_000))
			if !Task.isCancelled { self?.close() }
		}
		return true
	}

	/// Skipped: it fades away at once, and the window settles.
	func end() {
		rumbling?.cancel()
		finishing?.cancel()
		let fading = overlays
		overlays = []
		guard !fading.isEmpty else { return }
		NSAnimationContext.beginGrouping()
		NSAnimationContext.current.duration = 0.3
		for overlay in fading { overlay.animator().alphaValue = 0 }
		NSAnimationContext.endGrouping()
		Task {
			try? await Task.sleep(nanoseconds: 350_000_000)
			for overlay in fading { overlay.close() }
		}
	}

	private func close() {
		for overlay in overlays { overlay.close() }
		overlays = []
	}

	/**
	 * Rocks the window: a tremble while the light gathers, a kick when it bursts. It bursts where
	 * the window was, and stays there as the window's knocked about; moved by hand meanwhile, the
	 * window rocks around where it's put.
	 */
	private func rumble(_ scene: OutburstScene, from start: Date) {
		rumbling = Task { [weak self] in
			guard let first = self?.window?.frame.origin else { return }
			var rest = first
			var placed = first
			while !Task.isCancelled, let window = self?.window {
				let t = Date().timeIntervalSince(start)
				guard t < IntroTiming.bang + Outburst.kick else { break }
				let now = window.frame.origin
				if abs(now.x - placed.x) > 1 || abs(now.y - placed.y) > 1 {
					rest.x += now.x - placed.x
					rest.y += now.y - placed.y
				}
				let shift = Outburst.shake(at: t)
				let target = CGPoint(x: rest.x + shift.x, y: rest.y + shift.y)
				if target != now { window.setFrameOrigin(target) }
				placed = window.frame.origin
				scene.hole = window.frame
				if t < IntroTiming.bang {
					scene.center = CGPoint(x: rest.x + window.frame.width / 2, y: rest.y + window.frame.height / 2)
				}
				try? await Task.sleep(nanoseconds: 16_000_000)
			}
			if let window = self?.window {
				window.setFrameOrigin(rest)
				scene.hole = window.frame
			}
		}
	}

	/// How far the window's knocked at `t`: trembling more and more as the light gathers, then
	/// kicked hard by the bang and settling. Whole points, so it lands back where it was.
	private static func shake(at t: Double) -> CGPoint {
		let amount: Double
		if t >= IntroTiming.bang {
			amount = 12 * pow(1 - clamp((t - IntroTiming.bang) / kick), 2)
		} else if t >= IntroTiming.gather {
			amount = 2.5 * pow((t - IntroTiming.gather) / (IntroTiming.bang - IntroTiming.gather), 2)
		} else {
			amount = 0
		}
		let x = amount * (0.7 * sin(t * 83 + 1.3) + 0.3 * sin(t * 131))
		let y = amount * (0.7 * sin(t * 67) + 0.3 * sin(t * 149 + 0.4))
		return CGPoint(x: x.rounded(), y: y.rounded())
	}
}

/// One screen's see-through window: the outburst, redrawn every frame.
private struct OutburstView: View {
	let scene: OutburstScene
	/// The screen it covers, in AppKit's coordinates.
	let screen: CGRect
	let start: Date

	var body: some View {
		TimelineView(.animation) { timeline in
			Canvas { context, _ in
				scene.draw(&context, screen: screen, t: timeline.date.timeIntervalSince(start))
			}
		}
		.frame(width: screen.width, height: screen.height)
		.ignoresSafeArea()
	}
}

/**
 * What the outburst draws, from the intro's clock (IntroTiming): in AppKit's screen coordinates
 * (y up), which each screen's overlay turns into its own.
 */
final class OutburstScene {
	/// Until the last spark has burned out, in seconds from the intro's start.
	static let length = IntroTiming.bang + 3.3
	/// How hard the sparks fall as they burn out, in points per second squared.
	private static let gravity = 90.0

	/// Where it bursts: the middle of the window, where the light gathers.
	var center: CGPoint
	/// The window, which the dimming leaves alone.
	var hole: CGRect
	/// The window's size, the flash's scale inside it.
	private let extent: Double
	/// Past the farthest corner of the window's screen: how far the shock waves go.
	private let reach: Double
	private let specks: [Speck]
	private let sparks: [Spark]

	/// A speck of light drawn in while the light gathers.
	private struct Speck {
		var angle: Double
		/// Where it sets off, from the middle.
		var distance: Double
		/// When it sets off and when it's in, in seconds from the intro's start.
		var from: Double
		var to: Double
		/// How far round it turns on the way in.
		var swirl: Double
		var size: Double
		var color: Color
	}

	/// A spark out of the bang.
	private struct Spark {
		var angle: Double
		/// How far it flies before it stops, from the middle.
		var distance: Double
		/// How long after the bang it flies, and how long it burns.
		var delay: Double
		var life: Double
		var size: Double
		var color: Color
		/// Sideways a little as it goes.
		var curl: Double
		var flicker: Double
		var phase: Double
	}

	/// `window` and the `screen` it's on, in AppKit's coordinates.
	init(window: CGRect, screen: CGRect) {
		center = CGPoint(x: window.midX, y: window.midY)
		hole = window
		extent = Double(max(window.width, window.height))
		let across = max(abs(Double(screen.minX - window.midX)), abs(Double(screen.maxX - window.midX)))
		let down = max(abs(Double(screen.minY - window.midY)), abs(Double(screen.maxY - window.midY)))
		let reach = max(600, hypot(across, down))
		self.reach = reach
		specks = OutburstScene.makeSpecks(window: window, reach: reach)
		sparks = OutburstScene.makeSparks(window: window, reach: reach)
	}

	func draw(_ context: inout GraphicsContext, screen: CGRect, t: Double) {
		let size = screen.size
		let whole = CGRect(origin: .zero, size: size)
		let center = CGPoint(x: self.center.x - screen.minX, y: screen.maxY - self.center.y)
		let since = t - IntroTiming.bang

		// The lights go down around the window while the light gathers, and come up in the flash.
		let dim: Double = since < 0
			? 0.55 * pow(clamp((t - IntroTiming.gather) / (IntroTiming.bang - IntroTiming.gather)), 2)
			: 0.55 * (1 - clamp(since / 0.12))
		if dim > 0.001 {
			let window = CGRect(x: hole.minX - screen.minX, y: screen.maxY - hole.maxY, width: hole.width, height: hole.height)
			var around = Path(whole)
			around.addRoundedRect(in: window, cornerSize: CGSize(width: 10, height: 10))
			var shade = context
			shade.opacity = dim
			shade.fill(around, with: .color(.black), style: FillStyle(eoFill: true))
		}
		guard since >= 0 else {
			drawSpecks(&context, center: center, t: t)
			return
		}

		// The whole room lights up for a moment.
		let wash = 0.22 * pow(1 - clamp(since / 0.6), 2)
		if wash > 0.001 {
			var light = context
			light.opacity = wash
			light.fill(Path(whole), with: .color(BigBang.warm))
		}
		BigBang.shockWaves(&context, center: center, radius: reach * 1.25, length: 2, since: since)
		drawSparks(&context, center: center, since: since, bounds: whole.insetBy(dx: -40, dy: -40))
		BigBang.flash(&context, center: center, extent: extent * 1.5, since: since)
	}

	// MARK: Specks

	/// Slow at first, far out on the desktop, then rushing in: a whirlpool into the light.
	private func drawSpecks(_ context: inout GraphicsContext, center: CGPoint, t: Double) {
		for s in specks {
			let k = (t - s.from) / (s.to - s.from)
			guard k > 0, k < 1 else { continue }
			var streak = Path()
			streak.move(to: position(s, k: k - 0.08, center: center))
			streak.addLine(to: position(s, k: k, center: center))
			var c = context
			c.opacity = clamp(k / 0.25) * (0.3 + 0.7 * k)
			c.stroke(streak, with: .color(s.color), style: StrokeStyle(lineWidth: s.size, lineCap: .round))
		}
	}

	private func position(_ s: Speck, k: Double, center: CGPoint) -> CGPoint {
		let k = clamp(k)
		let distance = s.distance * (1 - k * k * k)
		let angle = s.angle + s.swirl * k
		return CGPoint(x: Double(center.x) + cos(angle) * distance, y: Double(center.y) + sin(angle) * distance)
	}

	private static func makeSpecks(window: CGRect, reach: Double) -> [Speck] {
		(0..<140).map { _ in
			let angle = Double.random(in: 0..<(2 * Double.pi))
			let out = edge(of: window, toward: angle)
			// Most of them in just before it bursts.
			let to = IntroTiming.bang - 0.03 - pow(Double.random(in: 0..<1), 1.6) * 0.5
			let from = max(IntroTiming.gather - 0.1, to - 0.5 - Double.random(in: 0..<0.4))
			let white = Double.random(in: 0..<1) < 0.7
			return Speck(
				angle: angle,
				distance: out + 30 + Double.random(in: 0..<1) * max(150, reach * 0.9 - out),
				from: from,
				to: to,
				swirl: 0.9 + Double.random(in: 0..<0.6),
				size: 0.8 + pow(Double.random(in: 0..<1), 2) * 1.4,
				color: (white ? Sky.starColors : Theme.avatars).randomElement() ?? .white
			)
		}
	}

	// MARK: Sparks

	/// Out past the window's edges, fast then slowing, white-hot then their colors, falling and
	/// flickering as they burn out.
	private func drawSparks(_ context: inout GraphicsContext, center: CGPoint, since: Double, bounds: CGRect) {
		for s in sparks {
			let age = since - s.delay
			guard age > 0, age < s.life else { continue }
			let k = age / s.life
			let head = position(s, age: age, center: center)
			let tail = position(s, age: max(0, age - 0.05), center: center)
			guard bounds.contains(head) || bounds.contains(tail) else { continue }
			let flicker = k > 0.4 ? 0.6 + 0.4 * sin(age * s.flicker + s.phase) : 1
			let alpha = (1 - k) * flicker
			let r = s.size * (1.2 - 0.6 * k)
			var c = context
			c.opacity = alpha
			var streak = Path()
			streak.move(to: tail)
			streak.addLine(to: head)
			c.stroke(streak, with: .color(s.color), style: StrokeStyle(lineWidth: max(0.7, r * 1.2), lineCap: .round))
			if s.size > 2 {
				// The biggest glow.
				let glow = r * 5
				var halo = context
				halo.opacity = alpha * 0.7
				halo.fill(
					Path(ellipseIn: BigBang.circle(head, glow)),
					with: .radialGradient(
						Gradient(colors: [s.color.opacity(0.55), s.color.opacity(0)]),
						center: head, startRadius: 0, endRadius: glow
					)
				)
			}
			c.fill(Path(ellipseIn: BigBang.circle(head, r)), with: .color(s.color))
			let heat = pow(1 - k, 3)
			if heat > 0.02 {
				var core = context
				core.opacity = alpha * heat
				core.fill(Path(ellipseIn: BigBang.circle(head, r * 0.6)), with: .color(.white))
			}
		}
	}

	private func position(_ s: Spark, age: Double, center: CGPoint) -> CGPoint {
		let k = clamp(age / s.life)
		let distance = s.distance * (1 - pow(1 - k, 3))
		let angle = s.angle + s.curl * k
		let fall = OutburstScene.gravity * age * age / 2
		return CGPoint(x: Double(center.x) + cos(angle) * distance, y: Double(center.y) + sin(angle) * distance + fall)
	}

	private static func makeSparks(window: CGRect, reach: Double) -> [Spark] {
		(0..<360).map { _ in
			let angle = Double.random(in: 0..<(2 * Double.pi))
			let out = edge(of: window, toward: angle)
			let white = Double.random(in: 0..<1) < 0.5
			return Spark(
				angle: angle,
				// Past the window's edge, most of them well out over the desktop, some off it.
				distance: out + 40 + pow(Double.random(in: 0..<1), 0.7) * max(200, reach * 1.15 - out),
				delay: pow(Double.random(in: 0..<1), 2) * 0.15,
				life: 1.4 + Double.random(in: 0..<1.7),
				size: 0.9 + pow(Double.random(in: 0..<1), 3) * 2.8,
				color: (white ? Sky.starColors : Theme.avatars).randomElement() ?? .white,
				curl: Double.random(in: -0.3..<0.3),
				flicker: 18 + Double.random(in: 0..<24),
				phase: Double.random(in: 0..<(2 * Double.pi))
			)
		}
	}

	/// From the window's middle to its edge, heading `angle`.
	private static func edge(of window: CGRect, toward angle: Double) -> Double {
		let across = Double(window.width) / 2 / max(0.001, abs(cos(angle)))
		let down = Double(window.height) / 2 / max(0.001, abs(sin(angle)))
		return min(across, down)
	}
}
