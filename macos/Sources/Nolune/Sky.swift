import SwiftUI

/**
 * The welcome's sky, as IntroSky.svelte draws it on the web: deep space with stars coming out and
 * drifting past, one shooting star, the eight avatar colors circling the wordmark like planets,
 * then melting into a glow behind the questions.
 *
 * The onboarding sets `stage` and friends; `draw` reads them each frame, like the web's `tick`.
 */
final class Sky {
	enum Stage: Equatable {
		case dark, stars, orbit, aurora, gone
	}

	var stage: Stage = .dark
	/// Deep space behind everything; turned off, it gives way to the page like a sunrise.
	var space = true
	/// Jump to the stage's resting state instead of moving there (skipped, or reduced motion).
	var still = false
	/// Where the planets start from (the wordmark's dots), and what they circle.
	var origins: [CGPoint] = []
	var center: CGPoint?
	/// A swell in the glow that fades by itself: access granted.
	var bloom: Double = 0

	private struct Star {
		/// Across the screen at depth 1, from -1 to 1.
		var x: Double
		var y: Double
		/// Depth: 1 far away, near 0 about to pass the camera.
		var z: Double
		var size: Double
		var color: Color
		/// When it comes out, in seconds after the stars begin.
		var appears: Double
		var twinkle: Double
		var phase: Double
	}

	private struct Orb {
		var x: Double = 0
		var y: Double = 0
		var r: Double = 0
		var alpha: Double = 0
	}

	private struct Nebula {
		var x: Double
		var y: Double
		var r: Double
		var color: Color
		var alpha: Double
	}

	private static let nebulae = [
		Nebula(x: 0.22, y: 0.3, r: 0.55, color: Color(hex: 0x2A3A8A), alpha: 0.2),
		Nebula(x: 0.78, y: 0.62, r: 0.6, color: Color(hex: 0x5A2A7A), alpha: 0.16),
		Nebula(x: 0.55, y: 0.18, r: 0.45, color: Color(hex: 0x1D5A6A), alpha: 0.14)
	]
	/// The galaxy's band across the sky, tilted; many of the stars crowd along it.
	private static let band = -0.42
	private static let starColors: [Color] = ([0xFFFFFF, 0xFFFFFF, 0xFFFFFF, 0xFFF1D6, 0xD6E4FF, 0xFFE1C2] as [UInt32])
		.map { Color(hex: $0) }
	/// How fast the camera drifts into the stars, in depth per second: slow, like floating.
	private static let drift = 0.022
	/// And how fast the whole field turns.
	private static let turnRate = 0.006
	private static let tilt = -11 * Double.pi / 180

	private var stars: [Star] = []
	private var orbs = Array(repeating: Orb(), count: Theme.avatars.count)
	private var from = Array(repeating: Orb(), count: Theme.avatars.count)
	/// Where each planet has just been, for its trail.
	private var trails = Array(repeating: [CGPoint](), count: Theme.avatars.count)
	private var current: Stage = .dark
	private var since: TimeInterval = 0
	private var starsSince: TimeInterval?
	private var last: TimeInterval?
	/// How far deep space has given way to the page (0: all space, 1: all page).
	private var dawn: Double = 0
	private var turn: Double = 0
	private var shooting: (at: Double, x: Double, y: Double, dx: Double, dy: Double)?
	private var size = CGSize.zero
	private var width: Double { Double(size.width) }
	private var height: Double { Double(size.height) }

	// MARK: Frame

	func draw(_ context: inout GraphicsContext, size: CGSize, now: TimeInterval) {
		if stars.isEmpty || size != self.size {
			self.size = size
			makeStars()
		}
		let dt = min(0.1, now - (last ?? now))
		last = now
		if stage != current {
			if current == .orbit { trails = trails.map { _ in [CGPoint]() } }
			current = stage
			since = now
			from = orbs
		}
		if current != .dark, starsSince == nil { starsSince = now }
		let t = now - since
		let skyTime = starsSince.map { now - $0 } ?? 0
		let out = current == .gone ? max(0, 1 - t / 0.8) : 1

		// The camera floats forward; stars that pass it come back far away.
		if !still {
			turn += Sky.turnRate * dt
			for i in stars.indices {
				stars[i].z -= Sky.drift * dt
				if stars[i].z < 0.06 {
					let appears = stars[i].appears
					stars[i] = newStar(appears: appears, z: 1)
				}
			}
		}
		// Space gives way to the page over a second and a half, like a sunrise.
		dawn = still ? (space ? 0 : 1) : clamp(dawn + (space ? -1 : 1) * dt / 1.5)
		bloom = max(0, bloom - dt / 3)

		context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(Theme.background))
		if current == .dark {
			context.fill(Path(CGRect(origin: .zero, size: size)), with: .color(Theme.space))
			return
		}
		drawSpace(&context, t: skyTime, fadeOut: out)

		switch current {
		case .orbit:
			let k = still ? 1 : easeOut(t / 1.8)
			drawOrbits(&context, alpha: 0.07 * (still ? 1 : clamp((t - 0.6) / 2)))
			for i in orbs.indices {
				let target = onEllipse(i, t: t)
				let start = origins.isEmpty ? CGPoint(x: mid.x, y: mid.y) : origins[i % origins.count]
				let tx = Double(target.x)
				let ty = Double(target.y)
				let sx = Double(start.x)
				let sy = Double(start.y)
				orbs[i].x = sx + (tx - sx) * k
				orbs[i].y = sy + (ty - sy) * k
				orbs[i].r = 2.5 + (1.5 + Double(i % 3) * 1.2) * k
				orbs[i].alpha = min(1, t * 4)
				// The trail: where it just was, fainter the longer ago.
				trails[i].append(CGPoint(x: orbs[i].x, y: orbs[i].y))
				if trails[i].count > 9 { trails[i].removeFirst() }
				for (age, point) in trails[i].dropLast().reversed().enumerated() {
					var ghost = orbs[i]
					ghost.x = point.x
					ghost.y = point.y
					ghost.alpha *= 0.55 * pow(0.78, Double(age))
					drawOrb(&context, ghost, color: Theme.avatars[i], solid: 0)
				}
				drawOrb(&context, orbs[i], color: Theme.avatars[i], solid: k)
			}
		case .aurora, .gone:
			let k = still ? 1 : easeOut(t / 2.4)
			for i in orbs.indices {
				let target = inAurora(i, t: t)
				let start = from[i]
				orbs[i].x = start.x + (target.x - start.x) * k
				orbs[i].y = start.y + (target.y - start.y) * k
				orbs[i].r = start.r + (target.r - start.r) * k
				orbs[i].alpha = (start.alpha + (target.alpha - start.alpha) * k) * out
				var shown = orbs[i]
				shown.alpha *= 1 + bloom * 1.4
				shown.r *= 1 + bloom * 0.35
				// The planets melt into the glow.
				drawOrb(&context, shown, color: Theme.avatars[i], solid: 1 - k)
			}
		case .dark, .stars:
			break
		}
	}

	// MARK: Pieces

	private var mid: (x: Double, y: Double) {
		if let center { return (Double(center.x), Double(center.y)) }
		return (width / 2, height * 0.42)
	}

	private var radii: (rx: Double, ry: Double) {
		let rx = min(width * 0.47, 460)
		return (rx, rx * 0.3)
	}

	/// Planet `i`'s orbit, as a share of the widest: the first closest in.
	private func orbitOf(_ i: Int) -> Double {
		0.55 + 0.45 * Double(i) / Double(orbs.count - 1)
	}

	private func onEllipse(_ i: Int, t: Double) -> CGPoint {
		let scale = orbitOf(i)
		let (rx, ry) = radii
		// Closer in goes faster, as planets do; each starts somewhere else around.
		let a = Double(i) * 2.4 + t * 0.26 * pow(scale, -1.5)
		let ex: Double = cos(a) * rx * scale
		let ey: Double = sin(a) * ry * scale
		let center = mid
		let x: Double = center.x + ex * cos(Sky.tilt) - ey * sin(Sky.tilt)
		let y: Double = center.y + ex * sin(Sky.tilt) + ey * cos(Sky.tilt)
		return CGPoint(x: x, y: y)
	}

	/// The orbits themselves, faint, like a map of them.
	private func drawOrbits(_ context: inout GraphicsContext, alpha: Double) {
		guard alpha > 0.001 else { return }
		let (rx, ry) = radii
		let center = mid
		var c = context
		c.opacity = alpha
		c.translateBy(x: center.x, y: center.y)
		c.rotate(by: .radians(Sky.tilt))
		for i in orbs.indices {
			let s = orbitOf(i)
			let rect = CGRect(x: -rx * s, y: -ry * s, width: 2 * rx * s, height: 2 * ry * s)
			c.stroke(Path(ellipseIn: rect), with: .color(.white), lineWidth: 1)
		}
	}

	/// Each color's place in the glow: spread wide and low behind the middle, drifting.
	private func inAurora(_ i: Int, t: Double) -> Orb {
		let (rx, _) = radii
		let center = mid
		let n = Double(i)
		let spread: Double = (n / Double(orbs.count - 1) - 0.5) * 1.5 * rx
		let side: Double = i % 2 == 1 ? -1 : 1
		let x: Double = center.x + spread + sin(t * 0.25 + n * 1.3) * 30
		let y: Double = center.y + side * 36 + cos(t * 0.2 + n) * 22
		let r: Double = min(width, 1100) * (0.07 + Double(i % 3) * 0.018)
		return Orb(x: x, y: y, r: r, alpha: 0.22)
	}

	private func drawOrb(_ context: inout GraphicsContext, _ orb: Orb, color: Color, solid: Double) {
		guard orb.alpha > 0.001, orb.r > 0.1 else { return }
		let center = CGPoint(x: orb.x, y: orb.y)
		let reach = orb.r * 2.6
		let glow = Gradient(stops: [
			.init(color: color, location: 0),
			.init(color: color.opacity(0.45), location: 0.3),
			.init(color: color.opacity(0), location: 1)
		])
		var c = context
		c.opacity = min(1, orb.alpha)
		c.fill(
			Path(ellipseIn: CGRect(x: orb.x - reach, y: orb.y - reach, width: reach * 2, height: reach * 2)),
			with: .radialGradient(glow, center: center, startRadius: 0, endRadius: reach)
		)
		if solid > 0 {
			// A planet: a solid body in its glow.
			let r = orb.r * 0.7
			c.opacity = min(1, orb.alpha * solid)
			c.fill(Path(ellipseIn: CGRect(x: orb.x - r, y: orb.y - r, width: r * 2, height: r * 2)), with: .color(color))
		}
	}

	private func drawSpace(_ context: inout GraphicsContext, t: Double, fadeOut: Double) {
		let whole = Path(CGRect(origin: .zero, size: size))
		let spaceAmount = 1 - easeInOut(dawn)
		let extent = max(width, height)
		if spaceAmount > 0.001 {
			var c = context
			c.opacity = spaceAmount
			c.fill(whole, with: .color(Theme.space))

			// The band: a long, faint glow the crowded stars sit in.
			var band = context
			band.translateBy(x: width / 2, y: height / 2)
			band.rotate(by: .radians(Sky.band + turn))
			band.scaleBy(x: 1, y: 0.16)
			band.opacity = spaceAmount * 0.12 * clamp(t / 5) * fadeOut
			let bandColor = Color(red: 150 / 255, green: 165 / 255, blue: 220 / 255)
			let r = extent * 0.75
			band.fill(
				Path(ellipseIn: CGRect(x: -r, y: -r, width: r * 2, height: r * 2)),
				with: .radialGradient(
					Gradient(colors: [bandColor, bandColor.opacity(0)]),
					center: .zero, startRadius: 0, endRadius: r
				)
			)

			for n in Sky.nebulae {
				let x = (n.x + sin(t * 0.03 + n.r) * 0.03) * width
				let y = (n.y + cos(t * 0.025 + n.r) * 0.03) * height
				var cloud = context
				cloud.opacity = spaceAmount * n.alpha * clamp(t / 6) * fadeOut
				cloud.fill(
					whole,
					with: .radialGradient(
						Gradient(colors: [n.color, n.color.opacity(0)]),
						center: CGPoint(x: x, y: y), startRadius: 0, endRadius: n.r * extent
					)
				)
			}
		}

		// Stars stay, fewer, once space gives way to the dark page.
		let shown = (0.35 + 0.65 * spaceAmount) * fadeOut
		guard shown > 0.001 else { return }
		let cx = width / 2
		let cy = height / 2
		let half = extent / 2
		let cosTurn = cos(turn)
		let sinTurn = sin(turn)
		for s in stars {
			let come = still ? 1 : clamp((t - s.appears) / 1.2)
			if come <= 0 { continue }
			let px: Double = (s.x * cosTurn - s.y * sinTurn) / s.z
			let py: Double = (s.x * sinTurn + s.y * cosTurn) / s.z
			let x: Double = cx + px * half
			let y: Double = cy + py * half
			if x < -4 || x > width + 4 || y < -4 || y > height + 4 { continue }
			let near = 1 - s.z
			let twinkle: Double = s.twinkle > 0 ? 0.7 + 0.3 * sin(t * s.twinkle + s.phase) : 1
			let alpha: Double = shown * come * twinkle * (0.45 + 0.55 * near)
			let r: Double = s.size * (0.6 + near * 1.1)
			var c = context
			c.opacity = alpha
			if r >= 1.6 {
				// The brightest have a soft halo.
				let reach = r * 4
				c.fill(
					Path(CGRect(x: x - reach, y: y - reach, width: reach * 2, height: reach * 2)),
					with: .radialGradient(
						Gradient(colors: [s.color.opacity(0.35), s.color.opacity(0)]),
						center: CGPoint(x: x, y: y), startRadius: 0, endRadius: reach
					)
				)
			}
			let dot = r < 0.9
				? Path(CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2))
				: Path(ellipseIn: CGRect(x: x - r, y: y - r, width: r * 2, height: r * 2))
			c.fill(dot, with: .color(s.color))
		}

		// One shooting star, a few seconds in.
		if shooting == nil, !still, t > 5.5, t < 6 {
			shooting = (t, width * 0.18, height * 0.14, width * 0.34, height * 0.16)
		}
		if let s = shooting {
			let k = (t - s.at) / 0.9
			if k >= 0, k <= 1 {
				let head = easeInOut(k)
				let x = s.x + s.dx * head
				let y = s.y + s.dy * head
				let tx = x - s.dx * 0.18
				let ty = y - s.dy * 0.18
				var streak = Path()
				streak.move(to: CGPoint(x: tx, y: ty))
				streak.addLine(to: CGPoint(x: x, y: y))
				var c = context
				c.opacity = shown * sin(Double.pi * k)
				c.stroke(
					streak,
					with: .linearGradient(
						Gradient(colors: [.white.opacity(0), .white.opacity(0.9)]),
						startPoint: CGPoint(x: tx, y: ty), endPoint: CGPoint(x: x, y: y)
					),
					lineWidth: 1.4
				)
			}
		}
	}

	// MARK: Stars

	/// They come out over the first few seconds, the brightest first.
	private func makeStars() {
		let count = min(1500, Int(width * height / 1000))
		let made = (0..<count).map { _ in newStar(appears: 0, z: 0.15 + Double.random(in: 0..<0.85)) }
			.sorted { $0.size > $1.size }
		stars = made.enumerated().map { i, star in
			var star = star
			star.appears = Double(i) / Double(max(1, count)) * 4.5 + Double.random(in: 0..<0.8)
			return star
		}
	}

	/// Somewhere on screen at depth `z`, as the camera's turned now: crowded along the band.
	private func newStar(appears: Double, z: Double) -> Star {
		let half = max(width, height) / 2
		var sx: Double
		var sy: Double
		if Double.random(in: 0..<1) < 0.45 {
			let along = Double.random(in: -1..<1) * 1.2
			let bell: Double = Double.random(in: 0..<1) + Double.random(in: 0..<1) + Double.random(in: 0..<1)
			let across = (bell - 1.5) * 0.12
			sx = along * cos(Sky.band) - across * sin(Sky.band)
			sy = along * sin(Sky.band) + across * cos(Sky.band)
		} else {
			sx = Double.random(in: -1..<1) * width * 0.55 / half
			sy = Double.random(in: -1..<1) * height * 0.55 / half
		}
		// Undo the camera's turn, so it lands where it was meant to.
		let cosTurn = cos(-turn)
		let sinTurn = sin(-turn)
		return Star(
			x: (sx * cosTurn - sy * sinTurn) * z,
			y: (sx * sinTurn + sy * cosTurn) * z,
			z: z,
			size: 0.4 + pow(Double.random(in: 0..<1), 3) * 1.6,
			color: Sky.starColors.randomElement() ?? .white,
			appears: appears,
			twinkle: Double.random(in: 0..<1) < 0.4 ? 1.5 + Double.random(in: 0..<3) : 0,
			phase: Double.random(in: 0..<(2 * Double.pi))
		)
	}
}

// MARK: Easing, as the web has it

func clamp(_ t: Double) -> Double { min(1, max(0, t)) }
func easeOut(_ t: Double) -> Double { 1 - pow(1 - clamp(t), 3) }
func easeInOut(_ t: Double) -> Double {
	let t = clamp(t)
	return t < 0.5 ? 4 * t * t * t : 1 - pow(-2 * t + 2, 3) / 2
}

/// CSS's cubic-bezier(x1, y1, x2, y2), for the wordmark's keyframes.
func cubicBezier(_ x1: Double, _ y1: Double, _ x2: Double, _ y2: Double, _ t: Double) -> Double {
	let t = clamp(t)
	if t == 0 || t == 1 { return t }
	func sample(_ a: Double, _ b: Double, _ s: Double) -> Double {
		3 * a * s * (1 - s) * (1 - s) + 3 * b * s * s * (1 - s) + s * s * s
	}
	// Find s where x(s) = t by bisection: plenty for an animation.
	var low = 0.0
	var high = 1.0
	var s = t
	for _ in 0..<24 {
		let x = sample(x1, x2, s)
		if abs(x - t) < 1e-5 { break }
		if x < t { low = s } else { high = s }
		s = (low + high) / 2
	}
	return sample(y1, y2, s)
}

/// The sky, redrawn every frame.
struct SkyView: View {
	let sky: Sky

	var body: some View {
		Group {
			if let clock = Snapshot.clock {
				let intro = Snapshot.intro
				Canvas { context, size in
					Snapshot.drawSky(sky, &context, size: size, clock: clock, intro: intro)
				}
			} else {
				TimelineView(.animation) { timeline in
					Canvas { context, size in
						sky.draw(&context, size: size, now: timeline.date.timeIntervalSinceReferenceDate)
					}
				}
			}
		}
		.allowsHitTesting(false)
	}
}
